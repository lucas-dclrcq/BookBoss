pub mod auth;
pub mod book;
pub mod device;
pub mod error;
pub mod event;
pub mod filter;
pub mod format;
pub mod health;
pub mod import;
pub mod jobs;
pub mod library;
pub mod message;
pub mod metadata;
pub mod opds;
pub mod pipeline;
pub mod reading;
pub mod repository;
pub mod shelf;
pub mod storage;
pub mod types;
pub mod user;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use derive_builder::Builder;
pub use error::{Error, ErrorKind, RepositoryError};
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemBuilder, SubsystemHandle};

use crate::{
    auth::{AuthService, AuthServiceImpl},
    book::{BookService, BookServiceImpl},
    device::{DeviceService, service::DeviceServiceImpl},
    event::EventService,
    format::FormatService,
    health::HealthService,
    import::{
        ImportJobService,
        scanner::{BookdropScanner, ScanReceiver, ScanTrigger, ScanWorker, create_scan_channel},
        service::ImportJobServiceImpl,
    },
    jobs::{JobService, JobWorker},
    library::{LibraryService, LibraryServiceImpl},
    message::{SystemMessageService, SystemMessageServiceImpl},
    metadata::MetadataService,
    opds::{OpdsService, OpdsServiceImpl},
    pipeline::PipelineService,
    reading::{ReadingService, ReadingServiceImpl},
    repository::RepositoryService,
    shelf::{ShelfService, service::ShelfServiceImpl},
    storage::FileStoreService,
    user::{UserService, UserServiceImpl, UserSettingService, UserSettingServiceImpl},
};

#[cfg(feature = "test-support")]
pub mod test_support;

/// All externally-provided adapter implementations required by `CoreServices`.
///
/// Use `ExternalServicesBuilder` to construct — all fields are required and
/// `.build()` returns an error if any are missing.
#[derive(Builder)]
#[builder(pattern = "owned")]
pub struct ExternalServices {
    pub repository_service: Arc<RepositoryService>,
    pub file_store: Arc<dyn FileStoreService>,
    pub format_service: Arc<dyn FormatService>,
    pub metadata_service: Arc<dyn MetadataService>,
    pub pipeline_service: Arc<dyn PipelineService>,
    pub job_service: Arc<dyn JobService>,
    pub health_service: Arc<dyn HealthService>,
    pub event_service: Arc<dyn EventService>,
    /// Path to the bookdrop directory for automatic import scanning.
    #[builder(default)]
    pub bookdrop_path: Option<PathBuf>,
    /// Polling interval for the bookdrop scanner.
    #[builder(default)]
    pub scan_interval: Option<Duration>,
}

pub struct CoreServices {
    pub repository_service: Arc<RepositoryService>,
    pub auth_service: Arc<dyn AuthService>,
    pub user_service: Arc<dyn UserService>,
    pub user_setting_service: Arc<dyn UserSettingService>,
    pub book_service: Arc<dyn BookService>,
    pub import_job_service: Arc<dyn ImportJobService>,
    pub file_store: Arc<dyn FileStoreService>,
    pub format_service: Arc<dyn FormatService>,
    pub metadata_service: Arc<dyn MetadataService>,
    pub library_service: Arc<dyn LibraryService>,
    pub pipeline_service: Arc<dyn PipelineService>,
    pub job_service: Arc<dyn JobService>,
    pub health_service: Arc<dyn HealthService>,
    pub shelf_service: Arc<dyn ShelfService>,
    pub reading_service: Arc<dyn ReadingService>,
    pub device_service: Arc<dyn DeviceService>,
    pub opds_service: Arc<dyn OpdsService>,
    pub event_service: Arc<dyn EventService>,
    pub system_message_service: Arc<dyn SystemMessageService>,
    /// Internal: holds the import scan wiring until `CoreSubsystem` takes it.
    import_scan_config: Mutex<Option<ImportScanConfig>>,
}

/// Internal wiring for the import scanner subsystem.
struct ImportScanConfig {
    bookdrop_path: PathBuf,
    scan_interval: Duration,
    scan_trigger: ScanTrigger,
    scan_receiver: ScanReceiver,
}

impl CoreServices {
    pub(crate) fn new(external: ExternalServices, encryption_secret: &str) -> Self {
        let ExternalServices {
            repository_service,
            file_store,
            format_service,
            metadata_service,
            pipeline_service,
            job_service,
            health_service,
            event_service,
            bookdrop_path,
            scan_interval,
        } = external;

        // Create the scan channel if bookdrop is configured.
        let (scan_trigger_for_service, import_scan_config) = if let (Some(path), Some(interval)) = (bookdrop_path, scan_interval) {
            let (trigger, receiver) = create_scan_channel();
            (
                Some(trigger.clone()),
                Some(ImportScanConfig {
                    bookdrop_path: path,
                    scan_interval: interval,
                    scan_trigger: trigger,
                    scan_receiver: receiver,
                }),
            )
        } else {
            (None, None)
        };

        Self {
            repository_service: repository_service.clone(),
            auth_service: Arc::new(AuthServiceImpl::new(repository_service.clone())),
            user_service: Arc::new(UserServiceImpl::new(repository_service.clone())),
            user_setting_service: Arc::new(UserSettingServiceImpl::new(repository_service.clone())),
            book_service: Arc::new(BookServiceImpl::new(repository_service.clone())),
            import_job_service: Arc::new(ImportJobServiceImpl::new(repository_service.clone(), scan_trigger_for_service)),
            library_service: Arc::new(LibraryServiceImpl::new(
                repository_service.clone(),
                file_store.clone(),
                format_service.clone(),
                job_service.clone(),
                event_service.clone(),
            )),
            file_store,
            format_service,
            metadata_service,
            pipeline_service,
            job_service,
            health_service,
            shelf_service: Arc::new(ShelfServiceImpl::new(repository_service.clone())),
            reading_service: Arc::new(ReadingServiceImpl::new(repository_service.clone())),
            device_service: Arc::new(DeviceServiceImpl::new(repository_service.clone())),
            opds_service: Arc::new(OpdsServiceImpl::new(repository_service.clone(), encryption_secret)),
            system_message_service: Arc::new(SystemMessageServiceImpl::new(repository_service, event_service.clone())),
            event_service,
            import_scan_config: Mutex::new(import_scan_config),
        }
    }

    /// Takes the import scan config, if configured. Can only be called once
    /// (returns `None` on subsequent calls). Used by `CoreSubsystem` to start
    /// the scanner worker and timer.
    fn take_import_scan_config(&self) -> Option<ImportScanConfig> {
        self.import_scan_config.lock().expect("import_scan_config mutex poisoned").take()
    }
}

pub fn create_services(external: ExternalServices, encryption_secret: &str) -> Result<Arc<CoreServices>, Error> {
    Ok(Arc::new(CoreServices::new(external, encryption_secret)))
}

/// Register core job handlers and health tasks.
///
/// Called once after `CoreServices` is built — before the subsystem event loop
/// starts. Each crate that owns handlers exposes a similar function.
pub fn before_start(core: &Arc<CoreServices>) {
    use format::handler::EnrichBookFilesHandler;
    use health::{
        HealthTaskConfig,
        handlers::{
            cleanup_expired_sessions, cleanup_old_import_jobs, cleanup_old_jobs, cleanup_old_system_messages, cleanup_orphan_authors,
            cleanup_orphan_publishers, cleanup_orphan_series, ensure_enrichments, recover_enrichments, reset_stale_import_jobs, verify_file_integrity,
        },
    };
    use import::handler::ProcessImportHandler;
    use jobs::JobServiceExt;

    let js = &core.job_service;
    let hs = &core.health_service;

    // Import pipeline handler
    js.register(ProcessImportHandler::new(core.clone()));

    // Format enrichment handler
    js.register(EnrichBookFilesHandler::new(core.clone()));

    // Health check handlers + their scheduled tasks
    js.register(recover_enrichments::RecoverEnrichmentsHandler::new(core.clone()));
    hs.register_task(HealthTaskConfig {
        name: "Recover Enrichments".into(),
        job_type: "health.recover_enrichments".into(),
        run_on_startup: true,
        interval_minutes: 60,
    });

    js.register(ensure_enrichments::EnsureEnrichmentsHandler::new(core.clone()));
    hs.register_task(HealthTaskConfig {
        name: "Ensure Enrichments".into(),
        job_type: "health.ensure_enrichments".into(),
        run_on_startup: true,
        interval_minutes: 120,
    });

    js.register(cleanup_orphan_authors::CleanupOrphanAuthorsHandler::new(core.clone()));
    hs.register_task(HealthTaskConfig {
        name: "Cleanup Orphan Authors".into(),
        job_type: "health.cleanup_orphan_authors".into(),
        run_on_startup: false,
        interval_minutes: 360,
    });

    js.register(cleanup_orphan_series::CleanupOrphanSeriesHandler::new(core.clone()));
    hs.register_task(HealthTaskConfig {
        name: "Cleanup Orphan Series".into(),
        job_type: "health.cleanup_orphan_series".into(),
        run_on_startup: false,
        interval_minutes: 360,
    });

    js.register(cleanup_orphan_publishers::CleanupOrphanPublishersHandler::new(core.clone()));
    hs.register_task(HealthTaskConfig {
        name: "Cleanup Orphan Publishers".into(),
        job_type: "health.cleanup_orphan_publishers".into(),
        run_on_startup: false,
        interval_minutes: 360,
    });

    js.register(cleanup_old_jobs::CleanupOldJobsHandler::new(core.clone()));
    hs.register_task(HealthTaskConfig {
        name: "Cleanup Old Jobs".into(),
        job_type: "health.cleanup_old_jobs".into(),
        run_on_startup: false,
        interval_minutes: 1440,
    });

    js.register(cleanup_old_import_jobs::CleanupOldImportJobsHandler::new(core.clone()));
    hs.register_task(HealthTaskConfig {
        name: "Cleanup Old Import Jobs".into(),
        job_type: "health.cleanup_old_import_jobs".into(),
        run_on_startup: false,
        interval_minutes: 1440,
    });

    js.register(cleanup_old_system_messages::CleanupOldSystemMessagesHandler::new(core.clone()));
    hs.register_task(HealthTaskConfig {
        name: "Cleanup Old System Messages".into(),
        job_type: "health.cleanup_old_system_messages".into(),
        run_on_startup: false,
        interval_minutes: 1440,
    });

    js.register(cleanup_expired_sessions::CleanupExpiredSessionsHandler::new(core.clone()));
    hs.register_task(HealthTaskConfig {
        name: "Cleanup Expired Sessions".into(),
        job_type: "health.cleanup_expired_sessions".into(),
        run_on_startup: false,
        interval_minutes: 1440,
    });

    js.register(verify_file_integrity::VerifyFileIntegrityHandler::new(core.clone()));
    hs.register_task(HealthTaskConfig {
        name: "Verify Library File Integrity".into(),
        job_type: "health.verify_file_integrity".into(),
        run_on_startup: false,
        interval_minutes: 720,
    });

    js.register(reset_stale_import_jobs::ResetStaleImportJobsHandler::new(core.clone()));
    hs.register_task(HealthTaskConfig {
        name: "Reset Stale Import Jobs".into(),
        job_type: "health.reset_stale_import_jobs".into(),
        run_on_startup: true,
        interval_minutes: 360,
    });
}

pub struct CoreSubsystem {
    core_services: Arc<CoreServices>,
    poll_interval: Duration,
}

impl IntoSubsystem<Error> for CoreSubsystem {
    async fn run(self, subsys: &mut SubsystemHandle) -> Result<(), Error> {
        let worker = JobWorker::new(
            self.core_services.job_service.clone(),
            self.core_services.repository_service.repository().clone(),
            self.core_services.repository_service.job_repository().clone(),
            self.poll_interval,
            self.core_services.event_service.clone(),
        );
        subsys.start(SubsystemBuilder::new("Worker", worker.into_subsystem()));

        // Start the import scanner subsystem if bookdrop is configured.
        if let Some(config) = self.core_services.take_import_scan_config() {
            self.core_services.import_job_service.recover_on_startup().await?;

            let worker = ScanWorker::new(
                config.bookdrop_path,
                config.scan_receiver.0,
                self.core_services.import_job_service.clone(),
                self.core_services.file_store.clone(),
                self.core_services.format_service.clone(),
            );
            let scanner = BookdropScanner::new(config.scan_interval, config.scan_trigger);

            subsys.start(SubsystemBuilder::new("ScanWorker", worker.into_subsystem()));
            subsys.start(SubsystemBuilder::new("BookdropScanner", scanner.into_subsystem()));
        }

        tracing::info!("CoreSubsystem started");

        subsys.on_shutdown_requested().await;
        tracing::info!("CoreSubsystem shutting down...");

        Ok(())
    }
}

#[must_use]
pub fn create_core_subsystem(core_services: Arc<CoreServices>, poll_interval: Duration) -> CoreSubsystem {
    CoreSubsystem { core_services, poll_interval }
}
