use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use bb_core::{Error, book::FileFormat, import::ImportJobService};
use chrono::Utc;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemHandle};

// ── Channel types ────────────────────────────────────────────────────────────

pub(crate) enum ScanCommand {
    ScanOnce,
}

/// Cloneable handle for triggering an on-demand bookdrop scan.
///
/// `trigger()` is non-blocking: if a scan is already queued (channel full),
/// the call is silently dropped — no pile-up of redundant scans.
#[derive(Clone)]
pub struct ScanTrigger {
    tx: mpsc::Sender<ScanCommand>,
}

impl ScanTrigger {
    pub fn trigger(&self) {
        let _ = self.tx.try_send(ScanCommand::ScanOnce);
    }
}

#[async_trait::async_trait]
impl bb_core::import::ImportScanner for ScanTrigger {
    async fn trigger_scan(&self) {
        self.trigger();
    }
}

/// Opaque wrapper around the receiving end of the scan channel.
///
/// Passed to `create_import_subsystem` after the matching `ScanTrigger` has
/// been handed to `CoreServices`. The inner `Receiver` is intentionally
/// `pub(crate)` — external code has no meaningful operation to perform on it.
pub struct ScanReceiver(pub(crate) mpsc::Receiver<ScanCommand>);

// ── ScanWorker ───────────────────────────────────────────────────────────────

/// Executes bookdrop scans when commanded via the channel or on shutdown.
pub(crate) struct ScanWorker {
    bookdrop_path: PathBuf,
    scan_rx: mpsc::Receiver<ScanCommand>,
    import_job_service: Arc<dyn ImportJobService>,
}

impl ScanWorker {
    pub(crate) fn new(bookdrop_path: PathBuf, scan_rx: mpsc::Receiver<ScanCommand>, import_job_service: Arc<dyn ImportJobService>) -> Self {
        Self {
            bookdrop_path,
            scan_rx,
            import_job_service,
        }
    }

    async fn scan_once(&self) {
        let mut entries = match tokio::fs::read_dir(&self.bookdrop_path).await {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(path = %self.bookdrop_path.display(), error = %e, "cannot read bookdrop directory");
                return;
            }
        };

        loop {
            let entry = match entries.next_entry().await {
                Ok(Some(e)) => e,
                Ok(None) => break,
                Err(e) => {
                    tracing::warn!(error = %e, "error reading watch directory entry");
                    break;
                }
            };

            let path = entry.path();

            let file_type = match entry.file_type().await {
                Ok(ft) => ft,
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "cannot stat watch directory entry");
                    continue;
                }
            };

            if !file_type.is_file() {
                continue;
            }

            let Some(format) = detect_format(&path) else {
                tracing::debug!(path = %path.display(), "skipping unrecognised file extension");
                continue;
            };

            if let Err(e) = self.process_file(&path, format).await {
                tracing::warn!(path = %path.display(), error = %e, "failed to process file — skipping");
            }
        }
    }

    async fn process_file(&self, path: &Path, format: FileFormat) -> Result<(), Error> {
        let path_owned = path.to_owned();
        let hash = tokio::task::spawn_blocking(move || hash_file(&path_owned))
            .await
            .map_err(|e| Error::Infrastructure(format!("spawn_blocking join error: {e}")))?
            .map_err(|e| Error::Infrastructure(format!("file hashing failed: {e}")))?;

        let file_path_str = path.to_string_lossy().into_owned();
        let detected_at = Utc::now();

        self.import_job_service.queue_file_if_new(file_path_str, hash, format, detected_at).await
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

// ── LibraryScanner ───────────────────────────────────────────────────────────

/// Fires a `ScanOnce` command on a fixed timer interval.
///
/// Decoupled from scan execution: the actual scan is performed by `ScanWorker`.
pub(crate) struct LibraryScanner {
    poll_interval: Duration,
    scan_trigger: ScanTrigger,
}

impl LibraryScanner {
    pub(crate) fn new(poll_interval: Duration, scan_trigger: ScanTrigger) -> Self {
        Self { poll_interval, scan_trigger }
    }
}

impl IntoSubsystem<Error> for LibraryScanner {
    async fn run(self, subsys: &mut SubsystemHandle) -> Result<(), Error> {
        tracing::info!("library scanner timer started");

        let mut counter: u32 = 0;

        loop {
            tokio::select! {
                () = subsys.on_shutdown_requested() => {
                    tracing::info!("LibraryScanner shutting down...");
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

// ── Channel factory ──────────────────────────────────────────────────────────

/// Creates a matched `(ScanTrigger, ScanReceiver)` pair.
///
/// Channel capacity is 1: at most one pending scan at a time.
///
/// Call this before building `CoreServices` so the `ScanTrigger` can be passed
/// as `import_scanner`, then pass the `ScanReceiver` to
/// `create_import_subsystem`.
pub fn create_scan_trigger() -> (ScanTrigger, ScanReceiver) {
    let (tx, rx) = mpsc::channel(1);
    (ScanTrigger { tx }, ScanReceiver(rx))
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Detect the `FileFormat` from a file path's extension. Returns `None` for
/// unrecognised or missing extensions.
fn detect_format(path: &Path) -> Option<FileFormat> {
    match path.extension()?.to_str()? {
        "epub" => Some(FileFormat::Epub),
        "mobi" => Some(FileFormat::Mobi),
        "pdf" => Some(FileFormat::Pdf),
        "cbz" => Some(FileFormat::Cbz),
        "azw3" => Some(FileFormat::Azw3),
        _ => None,
    }
}

fn hash_file(path: &Path) -> std::io::Result<String> {
    use std::{
        fs::File,
        io::{BufReader, Read},
    };

    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 65536].into_boxed_slice();

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_format_known_extensions() {
        assert_eq!(detect_format(Path::new("book.epub")), Some(FileFormat::Epub));
        assert_eq!(detect_format(Path::new("book.mobi")), Some(FileFormat::Mobi));
        assert_eq!(detect_format(Path::new("book.pdf")), Some(FileFormat::Pdf));
        assert_eq!(detect_format(Path::new("book.cbz")), Some(FileFormat::Cbz));
        assert_eq!(detect_format(Path::new("book.azw3")), Some(FileFormat::Azw3));
    }

    #[test]
    fn detect_format_unknown_and_missing() {
        assert_eq!(detect_format(Path::new("book.txt")), None);
        assert_eq!(detect_format(Path::new("book.zip")), None);
        assert_eq!(detect_format(Path::new("no_extension")), None);
    }
}
