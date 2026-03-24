use std::sync::Arc;

// ProcessImportPayload is defined in bb-core so that ImportJobServiceImpl can
// enqueue it without depending on bb-import. Re-export it here for convenience.
pub use bb_core::import::ProcessImportPayload;
use bb_core::{CoreServices, Error, RepositoryError, jobs::JobHandler};

/// Handles `process_import` jobs by fetching the `ImportJob` and running it
/// through the acquisition pipeline.
///
/// `PipelineService::process_job` is responsible for all status transitions
/// and DB writes — the handler does not write the updated job itself.
pub struct ProcessImportHandler {
    core: Arc<CoreServices>,
}

impl ProcessImportHandler {
    pub fn new(core: Arc<CoreServices>) -> Self {
        Self { core }
    }
}

impl JobHandler for ProcessImportHandler {
    const JOB_TYPE: &'static str = "process_import";
    type Payload = ProcessImportPayload;

    async fn handle(&self, payload: ProcessImportPayload) -> Result<(), Error> {
        let job = self
            .core
            .import_job_service
            .find_by_id(payload.import_job_id)
            .await?
            .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;

        self.core.pipeline_service.process_job(job).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use bb_core::jobs::Enqueueable;

    use super::*;

    #[test]
    fn payload_serde_roundtrip() {
        let payload = ProcessImportPayload { import_job_id: 42 };
        let json = serde_json::to_value(&payload).unwrap();
        let back: ProcessImportPayload = serde_json::from_value(json).unwrap();
        assert_eq!(back.import_job_id, 42);
    }

    #[test]
    fn payload_job_type_and_priority() {
        assert_eq!(ProcessImportPayload::JOB_TYPE, "process_import");
        assert_eq!(ProcessImportPayload::DEFAULT_PRIORITY, 1);
    }
}
