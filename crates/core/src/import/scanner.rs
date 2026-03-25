use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use chrono::Utc;
use tokio::sync::mpsc;
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemHandle};

use crate::{Error, format::FormatService, import::ImportJobService, storage::FileStoreService};

// ── Channel types ────────────────────────────────────────────────────────────

pub(crate) enum ScanCommand {
    ScanOnce,
}

/// Cloneable handle for triggering an on-demand bookdrop scan.
///
/// `trigger()` is non-blocking: if a scan is already queued (channel full),
/// the call is silently dropped — no pile-up of redundant scans.
#[derive(Clone)]
pub(crate) struct ScanTrigger {
    tx: mpsc::Sender<ScanCommand>,
}

impl ScanTrigger {
    pub(crate) fn trigger(&self) {
        let _ = self.tx.try_send(ScanCommand::ScanOnce);
    }
}

/// Opaque wrapper around the receiving end of the scan channel.
pub(crate) struct ScanReceiver(pub(crate) mpsc::Receiver<ScanCommand>);

/// Creates a matched `(ScanTrigger, ScanReceiver)` pair.
///
/// Channel capacity is 1: at most one pending scan at a time.
pub(crate) fn create_scan_channel() -> (ScanTrigger, ScanReceiver) {
    let (tx, rx) = mpsc::channel(1);
    (ScanTrigger { tx }, ScanReceiver(rx))
}

// ── ScanWorker ───────────────────────────────────────────────────────────────

/// Executes bookdrop scans when commanded via the channel.
pub(crate) struct ScanWorker {
    bookdrop_path: PathBuf,
    scan_rx: mpsc::Receiver<ScanCommand>,
    import_job_service: Arc<dyn ImportJobService>,
    file_store: Arc<dyn FileStoreService>,
    format_service: Arc<dyn FormatService>,
}

impl ScanWorker {
    pub(crate) fn new(
        bookdrop_path: PathBuf,
        scan_rx: mpsc::Receiver<ScanCommand>,
        import_job_service: Arc<dyn ImportJobService>,
        file_store: Arc<dyn FileStoreService>,
        format_service: Arc<dyn FormatService>,
    ) -> Self {
        Self {
            bookdrop_path,
            scan_rx,
            import_job_service,
            file_store,
            format_service,
        }
    }

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

        self.import_job_service.queue_file_if_new(file_path_str, hash, file_format, detected_at).await
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
pub(crate) struct BookdropScanner {
    poll_interval: Duration,
    scan_trigger: ScanTrigger,
}

impl BookdropScanner {
    pub(crate) fn new(poll_interval: Duration, scan_trigger: ScanTrigger) -> Self {
        Self { poll_interval, scan_trigger }
    }
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
