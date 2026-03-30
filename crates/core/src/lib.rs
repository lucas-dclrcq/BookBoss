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
    event::{EventService, create_event_service},
    format::FormatService,
    health::{HealthCheckSubsystem, HealthService, create_health_subsystem},
    import::{BookdropScanSubsystem, ImportJobService, create_bookdrop_scan_subsystem, create_import_job_service},
    jobs::{JobService, JobWorker, create_job_service},
    library::{LibraryService, LibraryServiceImpl},
    message::{SystemMessageService, SystemMessageServiceImpl},
    metadata::{MetadataService, create_metadata_service},
    opds::{OpdsService, OpdsServiceImpl},
    pipeline::{PipelineService, PipelineServiceImpl},
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
    pub(crate) repository_service: Arc<RepositoryService>,
    pub(crate) file_store: Arc<dyn FileStoreService>,
    pub(crate) format_service: Arc<dyn FormatService>,
    /// Path to the bookdrop directory for automatic import scanning.
    #[builder(default)]
    pub(crate) bookdrop_path: Option<PathBuf>,
    /// Polling interval for the bookdrop scanner.
    #[builder(default)]
    pub(crate) scan_interval: Option<Duration>,
}

pub struct CoreServices {
    pub(crate) repository_service: Arc<RepositoryService>,
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
    /// Internal: holds the bookdrop scan subsystem until `CoreSubsystem` takes
    /// it.
    bookdrop_scan_subsystem: Mutex<Option<BookdropScanSubsystem>>,
    /// Internal: holds the health subsystem until `CoreSubsystem` takes it.
    health_subsystem: Mutex<Option<HealthCheckSubsystem>>,
}

impl CoreServices {
    pub(crate) fn new(external: ExternalServices, encryption_secret: &str) -> Self {
        let ExternalServices {
            repository_service,
            file_store,
            format_service,
            bookdrop_path,
            scan_interval,
        } = external;

        let event_service = create_event_service(64);
        let job_service = create_job_service(repository_service.clone());
        let metadata_service = create_metadata_service();
        let pipeline_service: Arc<dyn PipelineService> = Arc::new(PipelineServiceImpl::new(
            repository_service.clone(),
            file_store.clone(),
            format_service.clone(),
            metadata_service.clone(),
            event_service.clone(),
        ));
        let (health_service, health_subsystem) = create_health_subsystem(job_service.clone(), event_service.clone());
        let system_message_service: Arc<dyn SystemMessageService> = Arc::new(SystemMessageServiceImpl::new(repository_service.clone(), event_service.clone()));

        // Create the bookdrop scan subsystem if configured.
        let (import_job_service, bookdrop_scan_subsystem) = if let (Some(path), Some(interval)) = (bookdrop_path, scan_interval) {
            let (svc, subsystem) = create_bookdrop_scan_subsystem(
                repository_service.clone(),
                file_store.clone(),
                format_service.clone(),
                system_message_service.clone(),
                path,
                interval,
            );
            (svc, Some(subsystem))
        } else {
            (create_import_job_service(repository_service.clone()), None)
        };

        Self {
            repository_service: repository_service.clone(),
            auth_service: Arc::new(AuthServiceImpl::new(repository_service.clone())),
            user_service: Arc::new(UserServiceImpl::new(repository_service.clone())),
            user_setting_service: Arc::new(UserSettingServiceImpl::new(repository_service.clone())),
            book_service: Arc::new(BookServiceImpl::new(repository_service.clone(), job_service.clone())),
            import_job_service,
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
            system_message_service,
            event_service,
            bookdrop_scan_subsystem: Mutex::new(bookdrop_scan_subsystem),
            health_subsystem: Mutex::new(Some(health_subsystem)),
        }
    }

    /// Takes the bookdrop scan subsystem, if configured. Can only be called
    /// once (returns `None` on subsequent calls). Used by `CoreSubsystem`.
    fn take_bookdrop_scan_subsystem(&self) -> Option<BookdropScanSubsystem> {
        self.bookdrop_scan_subsystem.lock().expect("bookdrop_scan_subsystem mutex poisoned").take()
    }

    /// Takes the health subsystem. Can only be called once. Used by
    /// `CoreSubsystem` to start the `HealthCheckSubsystem`.
    fn take_health_subsystem(&self) -> Option<HealthCheckSubsystem> {
        self.health_subsystem.lock().expect("health_subsystem mutex poisoned").take()
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
        interval_minutes: Some(60),
    });

    js.register(ensure_enrichments::EnsureEnrichmentsHandler::new(core.clone()));
    hs.register_task(HealthTaskConfig {
        name: "Ensure Enrichments".into(),
        job_type: "health.ensure_enrichments".into(),
        run_on_startup: true,
        interval_minutes: Some(120),
    });

    js.register(cleanup_orphan_authors::CleanupOrphanAuthorsHandler::new(core.clone()));
    hs.register_task(HealthTaskConfig {
        name: "Cleanup Orphan Authors".into(),
        job_type: "health.cleanup_orphan_authors".into(),
        run_on_startup: false,
        interval_minutes: Some(360),
    });

    js.register(cleanup_orphan_series::CleanupOrphanSeriesHandler::new(core.clone()));
    hs.register_task(HealthTaskConfig {
        name: "Cleanup Orphan Series".into(),
        job_type: "health.cleanup_orphan_series".into(),
        run_on_startup: false,
        interval_minutes: Some(360),
    });

    js.register(cleanup_orphan_publishers::CleanupOrphanPublishersHandler::new(core.clone()));
    hs.register_task(HealthTaskConfig {
        name: "Cleanup Orphan Publishers".into(),
        job_type: "health.cleanup_orphan_publishers".into(),
        run_on_startup: false,
        interval_minutes: Some(360),
    });

    js.register(cleanup_old_jobs::CleanupOldJobsHandler::new(core.clone()));
    hs.register_task(HealthTaskConfig {
        name: "Cleanup Old Jobs".into(),
        job_type: "health.cleanup_old_jobs".into(),
        run_on_startup: false,
        interval_minutes: Some(1440),
    });

    js.register(cleanup_old_import_jobs::CleanupOldImportJobsHandler::new(core.clone()));
    hs.register_task(HealthTaskConfig {
        name: "Cleanup Old Import Jobs".into(),
        job_type: "health.cleanup_old_import_jobs".into(),
        run_on_startup: false,
        interval_minutes: Some(1440),
    });

    js.register(cleanup_old_system_messages::CleanupOldSystemMessagesHandler::new(core.clone()));
    hs.register_task(HealthTaskConfig {
        name: "Cleanup Old System Messages".into(),
        job_type: "health.cleanup_old_system_messages".into(),
        run_on_startup: false,
        interval_minutes: Some(1440),
    });

    js.register(cleanup_expired_sessions::CleanupExpiredSessionsHandler::new(core.clone()));
    hs.register_task(HealthTaskConfig {
        name: "Cleanup Expired Sessions".into(),
        job_type: "health.cleanup_expired_sessions".into(),
        run_on_startup: false,
        interval_minutes: Some(1440),
    });

    js.register(verify_file_integrity::VerifyFileIntegrityHandler::new(core.clone()));
    hs.register_task(HealthTaskConfig {
        name: "Verify Library File Integrity".into(),
        job_type: "health.verify_file_integrity".into(),
        run_on_startup: false,
        interval_minutes: Some(720),
    });

    js.register(reset_stale_import_jobs::ResetStaleImportJobsHandler::new(core.clone()));
    hs.register_task(HealthTaskConfig {
        name: "Reset Stale Import Jobs".into(),
        job_type: "health.reset_stale_import_jobs".into(),
        run_on_startup: true,
        interval_minutes: Some(360),
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

        // Start the health check subsystem.
        if let Some(health_subsystem) = self.core_services.take_health_subsystem() {
            subsys.start(SubsystemBuilder::new("Health", health_subsystem.into_subsystem()));
        }

        // Start the bookdrop scan subsystem if configured.
        if let Some(scan_subsystem) = self.core_services.take_bookdrop_scan_subsystem() {
            subsys.start(SubsystemBuilder::new("BookdropScan", scan_subsystem.into_subsystem()));
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
