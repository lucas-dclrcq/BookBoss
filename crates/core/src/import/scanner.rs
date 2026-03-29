use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use chrono::Utc;
use tokio::sync::mpsc;
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemBuilder, SubsystemHandle};

use crate::{
    Error,
    format::FormatService,
    import::{ImportJobService, service::ImportJobServiceImpl},
    repository::RepositoryService,
    storage::FileStoreService,
};

// ── Channel types
// ─────────────────────────────────────────────────────────────

enum ScanCommand {
    ScanOnce,
}

/// Cloneable handle for triggering an on-demand bookdrop scan.
///
/// `trigger()` is non-blocking: if a scan is already queued (channel full),
/// the call is silently dropped — no pile-up of redundant scans.
#[derive(Clone)]
pub(super) struct ScanTrigger {
    tx: mpsc::Sender<ScanCommand>,
}

impl ScanTrigger {
    pub(super) fn trigger(&self) {
        let _ = self.tx.try_send(ScanCommand::ScanOnce);
    }
}

fn create_scan_channel() -> (ScanTrigger, mpsc::Receiver<ScanCommand>) {
    let (tx, rx) = mpsc::channel(1);
    (ScanTrigger { tx }, rx)
}

// ── ScanWorker ───────────────────────────────────────────────────────────────

/// Executes bookdrop scans when commanded via the channel.
struct ScanWorker {
    bookdrop_path: PathBuf,
    scan_rx: mpsc::Receiver<ScanCommand>,
    import_job_service: Arc<dyn ImportJobService>,
    file_store: Arc<dyn FileStoreService>,
    format_service: Arc<dyn FormatService>,
}

impl ScanWorker {
    async fn scan_once(&self) {
        let files = match self.file_store.list_files(&self.bookdrop_path).await {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(path = %self.bookdrop_path.display(), error = %e, "cannot read bookdrop directory");
                return;
            }
        };

        for path in files {
            let Some(file_format) = self.format_service.detect_format(&path) else {
                tracing::debug!(path = %path.display(), "skipping unrecognised file extension");
                continue;
            };

            if let Err(e) = self.process_file(&path, file_format).await {
                tracing::warn!(path = %path.display(), error = %e, "failed to process file — skipping");
            }
        }
    }

    async fn process_file(&self, path: &Path, file_format: crate::book::FileFormat) -> Result<(), Error> {
        let hash = bb_utils::hash::hash_file(path)
            .await
            .map_err(|e| Error::Infrastructure(format!("file hashing failed: {e}")))?;

        let file_path_str = path.to_string_lossy().into_owned();
        let detected_at = Utc::now();

        self.import_job_service.queue_file_if_new(file_path_str, hash, file_format, detected_at).await.map(|_| ())
    }
}

impl IntoSubsystem<Error> for ScanWorker {
    async fn run(mut self, subsys: &mut SubsystemHandle) -> Result<(), Error> {
        tracing::info!(directory = %self.bookdrop_path.display(), "scan worker started");

        loop {
            tokio::select! {
                () = subsys.on_shutdown_requested() => {
                    tracing::info!("ScanWorker shutting down...");
                    break;
                }
                cmd = self.scan_rx.recv() => {
                    match cmd {
                        Some(ScanCommand::ScanOnce) => self.scan_once().await,
                        None => {
                            tracing::warn!("ScanWorker channel closed — shutting down");
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

// ── BookdropScanner ──────────────────────────────────────────────────────────

/// Fires a `ScanOnce` command on a fixed timer interval.
///
/// Decoupled from scan execution: the actual scan is performed by `ScanWorker`.
struct BookdropScanner {
    poll_interval: Duration,
    scan_trigger: ScanTrigger,
}

impl IntoSubsystem<Error> for BookdropScanner {
    async fn run(self, subsys: &mut SubsystemHandle) -> Result<(), Error> {
        tracing::info!("bookdrop scanner timer started");

        let mut counter: u32 = 0;

        loop {
            tokio::select! {
                () = subsys.on_shutdown_requested() => {
                    tracing::info!("BookdropScanner shutting down...");
                    break;
                }
                () = async {} => {
                    if counter == 0 {
                        self.scan_trigger.trigger();
                    }
                    counter += 1;
                    #[expect(clippy::cast_possible_truncation, reason = "poll interval in seconds fits in u32; no sane interval exceeds ~136 years")]
                    let poll_secs = self.poll_interval.as_secs() as u32;
                    if counter >= poll_secs {
                        counter = 0;
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }

        Ok(())
    }
}

// ── BookdropScanSubsystem ────────────────────────────────────────────────────

/// Owns the bookdrop scan wiring: startup recovery, the scan worker, and the
/// periodic trigger timer.
pub(crate) struct BookdropScanSubsystem {
    import_job_service: Arc<dyn ImportJobService>,
    scan_worker: ScanWorker,
    scanner: BookdropScanner,
}

impl IntoSubsystem<Error> for BookdropScanSubsystem {
    async fn run(self, subsys: &mut SubsystemHandle) -> Result<(), Error> {
        self.import_job_service.recover_on_startup().await?;

        subsys.start(SubsystemBuilder::new("ScanWorker", self.scan_worker.into_subsystem()));
        subsys.start(SubsystemBuilder::new("BookdropScanner", self.scanner.into_subsystem()));

        tracing::info!("BookdropScanSubsystem started");

        subsys.on_shutdown_requested().await;
        tracing::info!("BookdropScanSubsystem shutting down...");

        Ok(())
    }
}

/// Creates an [`ImportJobService`] and its paired [`BookdropScanSubsystem`].
///
/// The scan channel and trigger wiring are internal implementation details —
/// callers never see `ScanTrigger` or `ScanReceiver`.
#[must_use]
pub(crate) fn create_bookdrop_scan_subsystem(
    repository_service: Arc<RepositoryService>,
    file_store: Arc<dyn FileStoreService>,
    format_service: Arc<dyn FormatService>,
    bookdrop_path: PathBuf,
    scan_interval: Duration,
) -> (Arc<dyn ImportJobService>, BookdropScanSubsystem) {
    let (trigger, scan_rx) = create_scan_channel();
    let import_job_service: Arc<dyn ImportJobService> = Arc::new(ImportJobServiceImpl::new(repository_service, Some(trigger.clone())));
    let scan_worker = ScanWorker {
        bookdrop_path,
        scan_rx,
        import_job_service: import_job_service.clone(),
        file_store,
        format_service,
    };
    let scanner = BookdropScanner {
        poll_interval: scan_interval,
        scan_trigger: trigger,
    };
    let subsystem = BookdropScanSubsystem {
        import_job_service: import_job_service.clone(),
        scan_worker,
        scanner,
    };
    (import_job_service, subsystem)
}
