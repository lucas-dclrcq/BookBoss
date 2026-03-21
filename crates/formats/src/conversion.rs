use std::sync::Arc;

use bb_core::{
    Error,
    book::BookId,
    conversion::ConversionService,
    jobs::{Enqueueable, JobService, JobServiceExt},
};
use serde::{Deserialize, Serialize};

/// Payload for an `enrich_epub` job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichEpubPayload {
    pub book_id: BookId,
}

impl bb_core::jobs::Enqueueable for EnrichEpubPayload {
    const JOB_TYPE: &'static str = "enrich_epub";
    const DEFAULT_PRIORITY: i16 = 0;
}

/// Payload for a `convert_kepub` job.
///
/// Runs after `enrich_epub` completes — sources from the `Enriched Epub` file
/// and produces an `Enriched Kepub` file with `koboSpan` markup injected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvertKepubPayload {
    pub book_id: BookId,
}

impl bb_core::jobs::Enqueueable for ConvertKepubPayload {
    const JOB_TYPE: &'static str = "convert_kepub";
    const DEFAULT_PRIORITY: i16 = 0;
}

/// Implementation of [`ConversionService`] that enqueues jobs via
/// [`JobService`].
pub struct ConversionServiceImpl {
    job_service: Arc<dyn JobService>,
}

impl ConversionServiceImpl {
    #[must_use]
    pub fn new(job_service: Arc<dyn JobService>) -> Self {
        Self { job_service }
    }
}

#[async_trait::async_trait]
impl ConversionService for ConversionServiceImpl {
    async fn queue_enrich_epub(&self, book_id: BookId) -> Result<(), Error> {
        self.job_service.enqueue(&EnrichEpubPayload { book_id }).await
    }

    async fn queue_convert_kepub(&self, book_id: BookId) -> Result<(), Error> {
        self.job_service.enqueue(&ConvertKepubPayload { book_id }).await
    }

    async fn count_pending(&self) -> Result<u32, Error> {
        let enrich_count = self.job_service.count_pending_by_type(EnrichEpubPayload::JOB_TYPE).await?;
        let kepub_count = self.job_service.count_pending_by_type(ConvertKepubPayload::JOB_TYPE).await?;
        #[expect(clippy::cast_possible_truncation, reason = "pending conversion count; will never approach u32::MAX")]
        Ok((enrich_count + kepub_count) as u32)
    }
}

#[cfg(test)]
mod tests {
    use bb_core::jobs::Enqueueable;

    use super::{ConvertKepubPayload, EnrichEpubPayload};

    #[test]
    fn enrich_payload_serde_roundtrip() {
        let payload = EnrichEpubPayload { book_id: 42 };
        let json = serde_json::to_value(&payload).unwrap();
        let back: EnrichEpubPayload = serde_json::from_value(json).unwrap();
        assert_eq!(back.book_id, 42);
    }

    #[test]
    fn enrich_payload_job_type_and_priority() {
        assert_eq!(EnrichEpubPayload::JOB_TYPE, "enrich_epub");
        assert_eq!(EnrichEpubPayload::DEFAULT_PRIORITY, 0);
    }

    #[test]
    fn kepub_payload_serde_roundtrip() {
        let payload = ConvertKepubPayload { book_id: 99 };
        let json = serde_json::to_value(&payload).unwrap();
        let back: ConvertKepubPayload = serde_json::from_value(json).unwrap();
        assert_eq!(back.book_id, 99);
    }

    #[test]
    fn kepub_payload_job_type_and_priority() {
        assert_eq!(ConvertKepubPayload::JOB_TYPE, "convert_kepub");
        assert_eq!(ConvertKepubPayload::DEFAULT_PRIORITY, 0);
    }
}
