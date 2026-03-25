pub mod auth;
pub mod book;
pub mod conversion;
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
pub mod opds;
pub mod pipeline;
pub mod reading;
pub mod repository;
pub mod shelf;
pub mod storage;
pub mod types;
pub mod user;

use std::{sync::Arc, time::Duration};

use derive_builder::Builder;
pub use error::{Error, ErrorKind, RepositoryError};
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemBuilder, SubsystemHandle};

use crate::{
    auth::{AuthService, AuthServiceImpl},
    book::{BookService, BookServiceImpl},
    conversion::ConversionService,
    device::{DeviceService, service::DeviceServiceImpl},
    event::EventService,
    format::FormatService,
    health::HealthService,
    import::{ImportJobService, ImportScanner, service::ImportJobServiceImpl},
    jobs::{JobService, JobWorker},
    library::{LibraryService, LibraryServiceImpl},
    message::{SystemMessageService, SystemMessageServiceImpl},
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
    pub pipeline_service: Arc<dyn PipelineService>,
    pub conversion_service: Arc<dyn ConversionService>,
    pub job_service: Arc<dyn JobService>,
    pub health_service: Arc<dyn HealthService>,
    pub import_scanner: Arc<dyn ImportScanner>,
    pub event_service: Arc<dyn EventService>,
}

pub struct CoreServices {
    pub repository_service: Arc<RepositoryService>,
    pub auth_service: Arc<dyn AuthService>,
    pub user_service: Arc<dyn UserService>,
    pub user_setting_service: Arc<dyn UserSettingService>,
    pub book_service: Arc<dyn BookService>,
    pub import_job_service: Arc<dyn ImportJobService>,
    pub import_scanner: Arc<dyn ImportScanner>,
    pub file_store: Arc<dyn FileStoreService>,
    pub format_service: Arc<dyn FormatService>,
    pub library_service: Arc<dyn LibraryService>,
    pub pipeline_service: Arc<dyn PipelineService>,
    pub conversion_service: Arc<dyn ConversionService>,
    pub job_service: Arc<dyn JobService>,
    pub health_service: Arc<dyn HealthService>,
    pub shelf_service: Arc<dyn ShelfService>,
    pub reading_service: Arc<dyn ReadingService>,
    pub device_service: Arc<dyn DeviceService>,
    pub opds_service: Arc<dyn OpdsService>,
    pub event_service: Arc<dyn EventService>,
    pub system_message_service: Arc<dyn SystemMessageService>,
}

impl CoreServices {
    pub(crate) fn new(external: ExternalServices, encryption_secret: &str) -> Self {
        let ExternalServices {
            repository_service,
            file_store,
            format_service,
            pipeline_service,
            conversion_service,
            job_service,
            health_service,
            import_scanner,
            event_service,
        } = external;
        Self {
            repository_service: repository_service.clone(),
            auth_service: Arc::new(AuthServiceImpl::new(repository_service.clone())),
            user_service: Arc::new(UserServiceImpl::new(repository_service.clone())),
            user_setting_service: Arc::new(UserSettingServiceImpl::new(repository_service.clone())),
            book_service: Arc::new(BookServiceImpl::new(repository_service.clone())),
            import_job_service: Arc::new(ImportJobServiceImpl::new(repository_service.clone())),
            import_scanner,
            library_service: Arc::new(LibraryServiceImpl::new(repository_service.clone(), file_store.clone())),
            file_store,
            format_service,
            pipeline_service,
            conversion_service,
            job_service,
            health_service,
            shelf_service: Arc::new(ShelfServiceImpl::new(repository_service.clone())),
            reading_service: Arc::new(ReadingServiceImpl::new(repository_service.clone())),
            device_service: Arc::new(DeviceServiceImpl::new(repository_service.clone())),
            opds_service: Arc::new(OpdsServiceImpl::new(repository_service.clone(), encryption_secret)),
            system_message_service: Arc::new(SystemMessageServiceImpl::new(repository_service, event_service.clone())),
            event_service,
        }
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
    use health::{
        HealthTaskConfig,
        handlers::{
            cleanup_expired_sessions, cleanup_old_import_jobs, cleanup_old_jobs, cleanup_old_system_messages, cleanup_orphan_authors,
            cleanup_orphan_publishers, cleanup_orphan_series, ensure_enrichments, recover_enrichments, reset_stale_import_jobs, verify_file_integrity,
        },
    };
    use jobs::JobServiceExt;

    let js = &core.job_service;
    let hs = &core.health_service;

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
    job_service: Arc<dyn JobService>,
    repository_service: Arc<RepositoryService>,
    poll_interval: Duration,
    event_service: Arc<dyn EventService>,
}

impl IntoSubsystem<Error> for CoreSubsystem {
    async fn run(self, subsys: &mut SubsystemHandle) -> Result<(), Error> {
        let worker = JobWorker::new(
            self.job_service,
            self.repository_service.repository().clone(),
            self.repository_service.job_repository().clone(),
            self.poll_interval,
            self.event_service,
        );
        subsys.start(SubsystemBuilder::new("Worker", worker.into_subsystem()));

        tracing::info!("CoreSubsystem started");

        subsys.on_shutdown_requested().await;
        tracing::info!("CoreSubsystem shutting down...");

        Ok(())
    }
}

#[must_use]
pub fn create_core_subsystem(
    job_service: Arc<dyn JobService>,
    repository_service: Arc<RepositoryService>,
    poll_interval: Duration,
    event_service: Arc<dyn EventService>,
) -> CoreSubsystem {
    CoreSubsystem {
        job_service,
        repository_service,
        poll_interval,
        event_service,
    }
}
