pub mod handler;
pub mod scanner;

use std::{path::PathBuf, sync::Arc, time::Duration};

use bb_core::{
    Error,
    import::ImportStatus,
    jobs::JobRepositoryExt,
    repository::{RepositoryService, read_only_transaction, transaction},
};
pub use handler::{ProcessImportHandler, ProcessImportPayload};
pub use scanner::ScanTrigger;
use scanner::{LibraryScanner, ScanWorker, create_scan_channel};
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemBuilder, SubsystemHandle};

pub struct ImportSubsystem {
    bookdrop_path: PathBuf,
    poll_interval: Duration,
    repository_service: Arc<RepositoryService>,
    scan_trigger: ScanTrigger,
    scan_rx: tokio::sync::mpsc::Receiver<scanner::ScanCommand>,
}

impl IntoSubsystem<Error> for ImportSubsystem {
    async fn run(self, subsys: &mut SubsystemHandle) -> Result<(), Error> {
        tracing::info!("ImportSubsystem starting...");

        // Crash recovery: reset any import jobs left in-progress from a previous crash.
        let repo = self.repository_service.clone();
        let reset = transaction(&**repo.repository(), |tx| {
            let import_job_repo = repo.import_job_repository().clone();
            Box::pin(async move { import_job_repo.reset_in_progress_to_pending(tx).await })
        })
        .await?;

        if reset > 0 {
            tracing::warn!("reset {} in-progress import jobs to pending after startup", reset);
        }

        // Re-enqueue all pending import jobs. Covers both jobs reset above and
        // any that lost their queue entry (e.g. exhausted retries, manual cleanup).
        // The pipeline guards against double-processing via the status check.
        let mut enqueued = 0u64;
        let mut next_id = None;
        loop {
            let repo = self.repository_service.clone();
            let import_job_repo = repo.import_job_repository().clone();
            let ni = next_id;
            let batch = read_only_transaction(&**repo.repository(), |tx| {
                let import_job_repo = import_job_repo.clone();
                Box::pin(async move { import_job_repo.list_by_status(tx, ImportStatus::Pending, ni, None).await })
            })
            .await?;

            if batch.is_empty() {
                break;
            }

            let exhausted = batch.len() < 50;
            next_id = batch.last().map(|j| j.id + 1);

            let ids: Vec<u64> = batch.iter().map(|j| j.id).collect();
            let job_repo = repo.job_repository().clone();
            transaction(&**repo.repository(), |tx| {
                let job_repo = job_repo.clone();
                let ids = ids.clone();
                Box::pin(async move {
                    for import_job_id in ids {
                        job_repo.enqueue(tx, &ProcessImportPayload { import_job_id }).await?;
                    }
                    Ok(())
                })
            })
            .await?;

            enqueued += ids.len() as u64;

            if exhausted {
                break;
            }
        }

        if enqueued > 0 {
            tracing::info!("re-enqueued {} pending import jobs on startup", enqueued);
        }

        let repo = &self.repository_service;
        let worker = ScanWorker::new(
            self.bookdrop_path.clone(),
            self.scan_rx,
            repo.repository().clone(),
            repo.import_job_repository().clone(),
            repo.job_repository().clone(),
        );
        let scanner = LibraryScanner::new(self.poll_interval, self.scan_trigger);

        subsys.start(SubsystemBuilder::new("ScanWorker", worker.into_subsystem()));
        subsys.start(SubsystemBuilder::new("Scanner", scanner.into_subsystem()));

        tracing::info!("ImportSubsystem started");

        subsys.on_shutdown_requested().await;
        tracing::info!("ImportSubsystem shutdown");

        Ok(())
    }
}

/// Creates the `ImportSubsystem` and returns a `ScanTrigger` for on-demand
/// scans.
///
/// The caller (typically `bookboss`) should wrap the trigger in `Arc<dyn
/// ImportScanner>` and pass it to `create_services()` so the frontend can reach
/// it via `CoreServices`.
#[must_use]
pub fn create_import_subsystem(bookdrop_path: PathBuf, poll_interval: Duration, repository_service: Arc<RepositoryService>) -> (ImportSubsystem, ScanTrigger) {
    let (scan_trigger, scan_rx) = create_scan_channel();
    let subsystem = ImportSubsystem {
        bookdrop_path,
        poll_interval,
        repository_service,
        scan_trigger: scan_trigger.clone(),
        scan_rx,
    };
    (subsystem, scan_trigger)
}
