pub mod auth;
pub mod book;
pub mod conversion;
pub mod device;
pub mod error;
pub mod filter;
pub mod import;
pub mod jobs;
pub mod library;
pub mod opds;
pub mod pipeline;
pub mod reading;
pub mod repository;
pub mod shelf;
pub mod storage;
pub mod types;
pub mod user;

use std::{sync::Arc, time::Duration};

pub use error::{Error, ErrorKind, RepositoryError};
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemBuilder, SubsystemHandle};

use crate::{
    auth::{AuthService, AuthServiceImpl},
    book::{BookService, BookServiceImpl},
    conversion::ConversionService,
    device::{DeviceService, service::DeviceServiceImpl},
    import::{ImportJobService, ImportScanner, service::ImportJobServiceImpl},
    jobs::{JobRegistry, JobWorker},
    library::{LibraryService, LibraryServiceImpl},
    opds::{OpdsService, OpdsServiceImpl},
    pipeline::PipelineService,
    reading::{ReadingService, ReadingServiceImpl},
    repository::RepositoryService,
    shelf::{ShelfService, service::ShelfServiceImpl},
    storage::LibraryStore,
    user::{UserService, UserServiceImpl, UserSettingService, UserSettingServiceImpl},
};

#[cfg(feature = "test-support")]
pub mod test_support;

pub struct CoreServices {
    pub auth_service: Arc<dyn AuthService>,
    pub user_service: Arc<dyn UserService>,
    pub user_setting_service: Arc<dyn UserSettingService>,
    pub book_service: Arc<dyn BookService>,
    pub import_job_service: Arc<dyn ImportJobService>,
    pub import_scanner: Arc<dyn ImportScanner>,
    pub library_store: Arc<dyn LibraryStore>,
    pub library_service: Arc<dyn LibraryService>,
    pub pipeline_service: Arc<dyn PipelineService>,
    pub conversion_service: Arc<dyn ConversionService>,
    pub shelf_service: Arc<dyn ShelfService>,
    pub reading_service: Arc<dyn ReadingService>,
    pub device_service: Arc<dyn DeviceService>,
    pub opds_service: Arc<dyn OpdsService>,
}

impl CoreServices {
    pub(crate) fn new(
        repository_service: Arc<RepositoryService>,
        library_store: Arc<dyn LibraryStore>,
        pipeline_service: Arc<dyn PipelineService>,
        conversion_service: Arc<dyn ConversionService>,
        import_scanner: Arc<dyn ImportScanner>,
        encryption_secret: &str,
    ) -> Self {
        Self {
            auth_service: Arc::new(AuthServiceImpl::new(repository_service.clone())),
            user_service: Arc::new(UserServiceImpl::new(repository_service.clone())),
            user_setting_service: Arc::new(UserSettingServiceImpl::new(repository_service.clone())),
            book_service: Arc::new(BookServiceImpl::new(repository_service.clone())),
            import_job_service: Arc::new(ImportJobServiceImpl::new(repository_service.clone())),
            import_scanner,
            library_service: Arc::new(LibraryServiceImpl::new(repository_service.clone(), library_store.clone())),
            library_store,
            pipeline_service,
            conversion_service,
            shelf_service: Arc::new(ShelfServiceImpl::new(repository_service.clone())),
            reading_service: Arc::new(ReadingServiceImpl::new(repository_service.clone())),
            device_service: Arc::new(DeviceServiceImpl::new(repository_service.clone())),
            opds_service: Arc::new(OpdsServiceImpl::new(repository_service, encryption_secret)),
        }
    }
}

pub fn create_services(
    repository_service: Arc<RepositoryService>,
    library_store: Arc<dyn LibraryStore>,
    pipeline_service: Arc<dyn PipelineService>,
    conversion_service: Arc<dyn ConversionService>,
    import_scanner: Arc<dyn ImportScanner>,
    encryption_secret: &str,
) -> Result<Arc<CoreServices>, Error> {
    Ok(Arc::new(CoreServices::new(
        repository_service,
        library_store,
        pipeline_service,
        conversion_service,
        import_scanner,
        encryption_secret,
    )))
}

pub struct CoreSubsystem {
    registry: JobRegistry,
    repository_service: Arc<RepositoryService>,
    poll_interval: Duration,
}

impl IntoSubsystem<Error> for CoreSubsystem {
    async fn run(self, subsys: &mut SubsystemHandle) -> Result<(), Error> {
        let worker = JobWorker::new(
            self.registry,
            self.repository_service.repository().clone(),
            self.repository_service.job_repository().clone(),
            self.poll_interval,
        );
        subsys.start(SubsystemBuilder::new("Worker", worker.into_subsystem()));

        tracing::info!("CoreSubsystem started");

        subsys.on_shutdown_requested().await;
        tracing::info!("CoreSubsystem shutting down...");

        Ok(())
    }
}

#[must_use]
pub fn create_core_subsystem(registry: JobRegistry, repository_service: Arc<RepositoryService>, poll_interval: Duration) -> CoreSubsystem {
    CoreSubsystem {
        registry,
        repository_service,
        poll_interval,
    }
}
