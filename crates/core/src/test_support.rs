use std::sync::Arc;

use crate::{
    ExternalServicesBuilder,
    conversion::{ConversionService, MockConversionService},
    event::{self, EventService},
    import::{ImportJobService, ImportScanner, scanner::MockImportScanner, service::MockImportJobService},
    jobs::{JobService, service::MockJobService},
    library::{LibraryService, MockLibraryService},
    message::{SystemMessageService, service::MockSystemMessageService},
    pipeline::{PipelineService, service::MockPipelineService},
    storage::{LibraryStore, MockLibraryStore},
};

#[must_use]
pub fn nop_conversion_service() -> Arc<dyn ConversionService> {
    Arc::new(MockConversionService::new())
}

/// No-op `LibraryStore` for use in tests and as a placeholder.
///
/// Any unexpected call will panic, surfacing the missing expectation
/// immediately.
#[must_use]
pub fn nop_library_store() -> Arc<dyn LibraryStore> {
    Arc::new(MockLibraryStore::new())
}

#[must_use]
pub fn nop_pipeline_service() -> Arc<dyn PipelineService> {
    Arc::new(MockPipelineService::new())
}

#[must_use]
pub fn nop_library_service() -> Arc<dyn LibraryService> {
    Arc::new(MockLibraryService::new())
}

#[must_use]
pub fn nop_import_scanner() -> Arc<dyn ImportScanner> {
    let mut mock = MockImportScanner::new();
    mock.expect_trigger_scan().returning(|| Box::pin(async {}));
    Arc::new(mock)
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

/// Returns an `ExternalServicesBuilder` pre-populated with nop implementations
/// for all fields except `repository_service`, which callers must always
/// provide.
///
/// Override individual fields for the service(s) under test before calling
/// `.build()`.
#[must_use]
pub fn default_external_services_builder() -> ExternalServicesBuilder {
    ExternalServicesBuilder::default()
        .library_store(nop_library_store())
        .pipeline_service(nop_pipeline_service())
        .conversion_service(nop_conversion_service())
        .import_scanner(nop_import_scanner())
        .event_service(nop_event_service())
}
