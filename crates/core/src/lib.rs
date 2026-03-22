pub mod auth;
pub mod book;
pub mod conversion;
pub mod device;
pub mod error;
pub mod event;
pub mod filter;
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
    import::{ImportJobService, ImportScanner, service::ImportJobServiceImpl},
    jobs::{JobRegistry, JobWorker},
    library::{LibraryService, LibraryServiceImpl},
    message::{SystemMessageService, SystemMessageServiceImpl},
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

/// All externally-provided adapter implementations required by `CoreServices`.
///
/// Use `ExternalServicesBuilder` to construct — all fields are required and
/// `.build()` returns an error if any are missing.
#[derive(Builder)]
#[builder(pattern = "owned")]
pub struct ExternalServices {
    pub repository_service: Arc<RepositoryService>,
    pub library_store: Arc<dyn LibraryStore>,
    pub pipeline_service: Arc<dyn PipelineService>,
    pub conversion_service: Arc<dyn ConversionService>,
    pub import_scanner: Arc<dyn ImportScanner>,
    pub event_service: Arc<dyn EventService>,
}

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
    pub event_service: Arc<dyn EventService>,
    pub system_message_service: Arc<dyn SystemMessageService>,
}

impl CoreServices {
    pub(crate) fn new(external: ExternalServices, encryption_secret: &str) -> Self {
        let ExternalServices {
            repository_service,
            library_store,
            pipeline_service,
            conversion_service,
            import_scanner,
            event_service,
        } = external;
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
            opds_service: Arc::new(OpdsServiceImpl::new(repository_service.clone(), encryption_secret)),
            system_message_service: Arc::new(SystemMessageServiceImpl::new(repository_service)),
            event_service,
        }
    }
}

pub fn create_services(external: ExternalServices, encryption_secret: &str) -> Result<Arc<CoreServices>, Error> {
    Ok(Arc::new(CoreServices::new(external, encryption_secret)))
}

pub struct CoreSubsystem {
    registry: JobRegistry,
    repository_service: Arc<RepositoryService>,
    poll_interval: Duration,
    event_service: Arc<dyn EventService>,
}

impl IntoSubsystem<Error> for CoreSubsystem {
    async fn run(self, subsys: &mut SubsystemHandle) -> Result<(), Error> {
        let worker = JobWorker::new(
            self.registry,
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
    registry: JobRegistry,
    repository_service: Arc<RepositoryService>,
    poll_interval: Duration,
    event_service: Arc<dyn EventService>,
) -> CoreSubsystem {
    CoreSubsystem {
        registry,
        repository_service,
        poll_interval,
        event_service,
    }
}
