pub mod handler;
pub mod scanner;

use std::{path::PathBuf, sync::Arc, time::Duration};

use bb_core::{Error, import::ImportJobService};
pub use handler::{ProcessImportHandler, ProcessImportPayload};
use scanner::{LibraryScanner, ScanWorker};
pub use scanner::{ScanReceiver, ScanTrigger, create_scan_trigger};
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemBuilder, SubsystemHandle};

pub struct ImportSubsystem {
    bookdrop_path: PathBuf,
    poll_interval: Duration,
    import_job_service: Arc<dyn ImportJobService>,
    scan_trigger: ScanTrigger,
    scan_rx: ScanReceiver,
}

impl IntoSubsystem<Error> for ImportSubsystem {
    async fn run(self, subsys: &mut SubsystemHandle) -> Result<(), Error> {
        tracing::info!("ImportSubsystem starting...");

        self.import_job_service.recover_on_startup().await?;

        let worker = ScanWorker::new(self.bookdrop_path.clone(), self.scan_rx.0, self.import_job_service);
        let scanner = LibraryScanner::new(self.poll_interval, self.scan_trigger);

        subsys.start(SubsystemBuilder::new("ScanWorker", worker.into_subsystem()));
        subsys.start(SubsystemBuilder::new("Scanner", scanner.into_subsystem()));

        tracing::info!("ImportSubsystem started");

        subsys.on_shutdown_requested().await;
        tracing::info!("ImportSubsystem shutdown");

        Ok(())
    }
}

/// Creates the `ImportSubsystem`.
///
/// Call `create_scan_trigger()` first to obtain the `(ScanTrigger,
/// ScanReceiver)` pair. Pass the `ScanTrigger` to `create_services()` as
/// `import_scanner`, then call this function with the `ScanReceiver` and the
/// `import_job_service` from the resulting `CoreServices`.
#[must_use]
pub fn create_import_subsystem(
    bookdrop_path: PathBuf,
    poll_interval: Duration,
    scan_trigger: ScanTrigger,
    scan_receiver: ScanReceiver,
    import_job_service: Arc<dyn ImportJobService>,
) -> ImportSubsystem {
    ImportSubsystem {
        bookdrop_path,
        poll_interval,
        import_job_service,
        scan_trigger,
        scan_rx: scan_receiver,
    }
}
