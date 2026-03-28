use std::sync::Arc;

use crate::{
    ExternalServicesBuilder,
    event::{self, EventService},
    format::{FormatService, MockFormatService},
    health::{self, HealthService},
    import::{ImportJobService, service::MockImportJobService},
    jobs::{JobService, service::MockJobService},
    library::{LibraryService, MockLibraryService},
    message::{SystemMessageService, service::MockSystemMessageService},
    pipeline::{PipelineService, service::MockPipelineService},
    storage::{FileStoreService, MockFileStoreService},
};

#[must_use]
pub fn nop_format_service() -> Arc<dyn FormatService> {
    Arc::new(MockFormatService::new())
}

/// No-op `FileStoreService` for use in tests and as a placeholder.
///
/// Any unexpected call will panic, surfacing the missing expectation
/// immediately.
#[must_use]
pub fn nop_file_store() -> Arc<dyn FileStoreService> {
    Arc::new(MockFileStoreService::new())
}

#[must_use]
pub fn nop_pipeline_service() -> Arc<dyn PipelineService> {
    Arc::new(MockPipelineService::new())
}

#[must_use]
pub fn nop_library_service() -> Arc<dyn LibraryService> {
    Arc::new(MockLibraryService::new())
}

/// Returns a `MockJobService` with no expectations set.
///
/// Suitable for tests that wire up adapters but never exercise the job-queue
/// code path.
#[must_use]
pub fn nop_job_service() -> Arc<dyn JobService> {
    Arc::new(MockJobService::new())
}

/// Returns a `MockImportJobService` with no expectations set.
///
/// Suitable for tests that wire up `CoreServices` but never exercise the
/// import-job code path. Any unexpected call will panic, surfacing the
/// missing expectation immediately.
#[must_use]
pub fn nop_import_job_service() -> Arc<dyn ImportJobService> {
    Arc::new(MockImportJobService::new())
}

/// Returns a no-op `EventService` backed by a broadcast channel that nobody
/// listens on.  Suitable for tests that don't exercise real-time events.
#[must_use]
pub fn nop_event_service() -> Arc<dyn EventService> {
    event::create_event_service(16)
}

#[must_use]
pub fn nop_system_message_service() -> Arc<dyn SystemMessageService> {
    Arc::new(MockSystemMessageService::new())
}

/// Returns a no-op `HealthService` with no tasks registered.
///
/// Suitable for tests that wire up `CoreServices` but never exercise
/// health task scheduling.
#[must_use]
pub fn nop_health_service() -> Arc<dyn HealthService> {
    let (svc, _rx) = health::create_health_service();
    svc
}

/// Returns an `ExternalServicesBuilder` pre-populated with nop implementations
/// for all fields except `repository_service`, which callers must always
/// provide.
///
/// Override individual fields for the service(s) under test before calling
/// `.build()`.
#[must_use]
pub fn default_external_services_builder() -> ExternalServicesBuilder {
    ExternalServicesBuilder::default()
        .file_store(nop_file_store())
        .format_service(nop_format_service())
        .metadata_service(crate::metadata::create_metadata_service())
        .pipeline_service(nop_pipeline_service())
        .job_service(nop_job_service())
        .health_service(nop_health_service())
        .event_service(nop_event_service())
}
