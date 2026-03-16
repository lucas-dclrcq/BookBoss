use std::sync::Arc;

use bb_core::{
    Error,
    book::BookId,
    conversion::ConversionService,
    jobs::{Enqueueable, JobRepositoryExt},
    repository::{RepositoryService, read_only_transaction, transaction},
};
use serde::{Deserialize, Serialize};

/// Payload for an `enrich_epub` job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichEpubPayload {
    pub book_id: BookId,
}

impl Enqueueable for EnrichEpubPayload {
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

impl Enqueueable for ConvertKepubPayload {
    const JOB_TYPE: &'static str = "convert_kepub";
    const DEFAULT_PRIORITY: i16 = 0;
}

/// Implementation of [`ConversionService`] that enqueues jobs via the
/// standard [`JobRepository`].
pub struct ConversionServiceImpl {
    repository_service: Arc<RepositoryService>,
}

impl ConversionServiceImpl {
    #[must_use]
    pub fn new(repository_service: Arc<RepositoryService>) -> Self {
        Self { repository_service }
    }
}

#[async_trait::async_trait]
impl ConversionService for ConversionServiceImpl {
    async fn queue_enrich_epub(&self, book_id: BookId) -> Result<(), Error> {
        let job_repo = self.repository_service.job_repository().clone();
        transaction(&**self.repository_service.repository(), |tx| {
            Box::pin(async move {
                job_repo.enqueue(tx, &EnrichEpubPayload { book_id }).await?;
                Ok(())
            })
        })
        .await
    }

    async fn queue_convert_kepub(&self, book_id: BookId) -> Result<(), Error> {
        let job_repo = self.repository_service.job_repository().clone();
        transaction(&**self.repository_service.repository(), |tx| {
            Box::pin(async move {
                job_repo.enqueue(tx, &ConvertKepubPayload { book_id }).await?;
                Ok(())
            })
        })
        .await
    }

    async fn count_pending(&self) -> Result<u32, Error> {
        let job_repo = self.repository_service.job_repository().clone();
        let (enrich_count, kepub_count) = read_only_transaction(&**self.repository_service.repository(), |tx| {
            let job_repo = job_repo.clone();
            Box::pin(async move {
                let e = job_repo.count_pending_by_type(tx, EnrichEpubPayload::JOB_TYPE).await?;
                let k = job_repo.count_pending_by_type(tx, ConvertKepubPayload::JOB_TYPE).await?;
                Ok::<_, bb_core::Error>((e, k))
            })
        })
        .await?;
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
