use std::sync::Arc;

use crate::{
    Error,
    jobs::{JobHandler, JobRepositoryExt},
    message::{MessageSeverity, NewSystemMessage, SystemMessageService},
    repository::{RepositoryService, read_only_transaction, transaction},
};

pub struct EnsureEnrichmentsHandler {
    repository_service: Arc<RepositoryService>,
    system_message_service: Arc<dyn SystemMessageService>,
}

impl EnsureEnrichmentsHandler {
    #[must_use]
    pub fn new(repository_service: Arc<RepositoryService>, system_message_service: Arc<dyn SystemMessageService>) -> Self {
        Self {
            repository_service,
            system_message_service,
        }
    }
}

/// Payload for enrichment jobs — matches `EnrichEpubPayload` in bb-formats.
#[derive(serde::Serialize)]
struct EnrichPayload {
    book_id: u64,
}

impl crate::jobs::Enqueueable for EnrichPayload {
    const JOB_TYPE: &'static str = "enrich_epub";
}

/// Payload for KEPUB conversion jobs — matches `ConvertKepubPayload` in
/// bb-formats.
#[derive(serde::Serialize)]
struct KepubPayload {
    book_id: u64,
}

impl crate::jobs::Enqueueable for KepubPayload {
    const JOB_TYPE: &'static str = "convert_kepub";
}

impl JobHandler for EnsureEnrichmentsHandler {
    const JOB_TYPE: &'static str = "health.ensure_enrichments";
    type Payload = serde_json::Value;

    async fn handle(&self, _payload: serde_json::Value) -> Result<(), Error> {
        let book_repo = self.repository_service.book_repository().clone();

        // Find books missing enriched EPUB.
        let enrichment_ids = read_only_transaction(&**self.repository_service.repository(), |tx| {
            let book_repo = book_repo.clone();
            Box::pin(async move { book_repo.find_book_ids_needing_enrichment(tx).await })
        })
        .await?;

        // Find books missing KEPUB conversion.
        let book_repo = self.repository_service.book_repository().clone();
        let kepub_ids = read_only_transaction(&**self.repository_service.repository(), |tx| {
            let book_repo = book_repo.clone();
            Box::pin(async move { book_repo.find_book_ids_needing_kepub_conversion(tx).await })
        })
        .await?;

        // Find books where the enriched EPUB is older than the book's
        // updated_at — metadata changed since last enrichment.
        let book_repo = self.repository_service.book_repository().clone();
        let stale_ids = read_only_transaction(&**self.repository_service.repository(), |tx| {
            let book_repo = book_repo.clone();
            Box::pin(async move { book_repo.find_book_ids_with_stale_enrichment(tx).await })
        })
        .await?;

        // Merge stale IDs into the enrichment list (dedup since a book could
        // appear in both missing and stale sets).
        let mut all_enrich_ids = enrichment_ids;
        for id in stale_ids {
            if !all_enrich_ids.contains(&id) {
                all_enrich_ids.push(id);
            }
        }

        let enrich_count = all_enrich_ids.len();
        let kepub_count = kepub_ids.len();

        if enrich_count == 0 && kepub_count == 0 {
            tracing::info!("all books have up-to-date enrichments");
            return Ok(());
        }

        if !all_enrich_ids.is_empty() {
            let job_repo = self.repository_service.job_repository().clone();
            transaction(&**self.repository_service.repository(), |tx| {
                let job_repo = job_repo.clone();
                let all_enrich_ids = all_enrich_ids.clone();
                Box::pin(async move {
                    for book_id in all_enrich_ids {
                        job_repo.enqueue(tx, &EnrichPayload { book_id }).await?;
                    }
                    Ok(())
                })
            })
            .await?;

            tracing::info!(count = enrich_count, "enqueued enrichment jobs (missing + stale)");
        }

        if !kepub_ids.is_empty() {
            let job_repo = self.repository_service.job_repository().clone();
            transaction(&**self.repository_service.repository(), |tx| {
                let job_repo = job_repo.clone();
                let kepub_ids = kepub_ids.clone();
                Box::pin(async move {
                    for book_id in kepub_ids {
                        job_repo.enqueue(tx, &KepubPayload { book_id }).await?;
                    }
                    Ok(())
                })
            })
            .await?;

            tracing::info!(count = kepub_count, "enqueued missing KEPUB conversion jobs");
        }

        let mut parts = Vec::new();
        if enrich_count > 0 {
            parts.push(format!("{enrich_count} enrichment"));
        }
        if kepub_count > 0 {
            parts.push(format!("{kepub_count} KEPUB conversion"));
        }

        self.system_message_service
            .add_message(NewSystemMessage {
                source_task: Self::JOB_TYPE.to_string(),
                severity: MessageSeverity::Info,
                message: format!("Enqueued {} job(s) for missing/stale enrichments", parts.join(" and ")),
            })
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        book::repository::book::MockBookRepository, jobs::repository::MockJobRepository, message::repository::MockSystemMessageRepository,
        repository::testing::default_repository_service_builder,
    };

    fn fake_job() -> crate::jobs::Job {
        crate::jobs::Job {
            id: 1,
            job_type: String::new(),
            payload: serde_json::json!({}),
            status: crate::jobs::JobStatus::Pending,
            priority: 0,
            attempt: 0,
            max_attempts: 3,
            version: 0,
            scheduled_at: chrono::Utc::now(),
            started_at: None,
            completed_at: None,
            error_message: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn mock_add_message() -> MockSystemMessageRepository {
        let mut msg_repo = MockSystemMessageRepository::new();
        msg_repo.expect_add_message().returning(|_, msg| {
            let msg = crate::message::SystemMessage {
                id: 1,
                source_task: msg.source_task,
                severity: msg.severity,
                message: msg.message,
                created_at: chrono::Utc::now(),
            };
            Box::pin(std::future::ready(Ok(msg)))
        });
        msg_repo
    }

    #[tokio::test]
    async fn enqueues_missing_enrichments() {
        let mut book_repo = MockBookRepository::new();
        let mut job_repo = MockJobRepository::new();
        let msg_repo = mock_add_message();

        book_repo
            .expect_find_book_ids_needing_enrichment()
            .returning(|_| Box::pin(std::future::ready(Ok(vec![1]))));

        book_repo
            .expect_find_book_ids_needing_kepub_conversion()
            .returning(|_| Box::pin(std::future::ready(Ok(vec![]))));

        book_repo
            .expect_find_book_ids_with_stale_enrichment()
            .returning(|_| Box::pin(std::future::ready(Ok(vec![]))));

        job_repo
            .expect_enqueue_raw()
            .returning(|_, _, _, _| Box::pin(std::future::ready(Ok(fake_job()))));

        let repo_service = Arc::new(
            default_repository_service_builder()
                .book_repository(Arc::new(book_repo))
                .job_repository(Arc::new(job_repo))
                .system_message_repository(Arc::new(msg_repo))
                .build()
                .unwrap(),
        );

        let sms = crate::message::SystemMessageServiceImpl::new(repo_service.clone(), crate::test_support::nop_event_service());
        let handler = EnsureEnrichmentsHandler::new(repo_service, Arc::new(sms));
        handler.handle(serde_json::json!({})).await.unwrap();
    }

    #[tokio::test]
    async fn enqueues_stale_enrichments() {
        let mut book_repo = MockBookRepository::new();
        let mut job_repo = MockJobRepository::new();
        let msg_repo = mock_add_message();

        // No missing enrichments, but book 5 has a stale enriched EPUB.
        book_repo
            .expect_find_book_ids_needing_enrichment()
            .returning(|_| Box::pin(std::future::ready(Ok(vec![]))));

        book_repo
            .expect_find_book_ids_needing_kepub_conversion()
            .returning(|_| Box::pin(std::future::ready(Ok(vec![]))));

        book_repo
            .expect_find_book_ids_with_stale_enrichment()
            .returning(|_| Box::pin(std::future::ready(Ok(vec![5]))));

        job_repo
            .expect_enqueue_raw()
            .returning(|_, _, _, _| Box::pin(std::future::ready(Ok(fake_job()))));

        let repo_service = Arc::new(
            default_repository_service_builder()
                .book_repository(Arc::new(book_repo))
                .job_repository(Arc::new(job_repo))
                .system_message_repository(Arc::new(msg_repo))
                .build()
                .unwrap(),
        );

        let sms = crate::message::SystemMessageServiceImpl::new(repo_service.clone(), crate::test_support::nop_event_service());
        let handler = EnsureEnrichmentsHandler::new(repo_service, Arc::new(sms));
        handler.handle(serde_json::json!({})).await.unwrap();
    }

    #[tokio::test]
    async fn deduplicates_missing_and_stale() {
        let mut book_repo = MockBookRepository::new();
        let mut job_repo = MockJobRepository::new();
        let msg_repo = mock_add_message();

        // Book 1 appears in both missing and stale — should only be enqueued once.
        book_repo
            .expect_find_book_ids_needing_enrichment()
            .returning(|_| Box::pin(std::future::ready(Ok(vec![1]))));

        book_repo
            .expect_find_book_ids_needing_kepub_conversion()
            .returning(|_| Box::pin(std::future::ready(Ok(vec![]))));

        book_repo
            .expect_find_book_ids_with_stale_enrichment()
            .returning(|_| Box::pin(std::future::ready(Ok(vec![1, 2]))));

        let enqueue_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let enqueue_count_clone = enqueue_count.clone();
        job_repo.expect_enqueue_raw().returning(move |_, _, _, _| {
            enqueue_count_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Box::pin(std::future::ready(Ok(fake_job())))
        });

        let repo_service = Arc::new(
            default_repository_service_builder()
                .book_repository(Arc::new(book_repo))
                .job_repository(Arc::new(job_repo))
                .system_message_repository(Arc::new(msg_repo))
                .build()
                .unwrap(),
        );

        let sms = crate::message::SystemMessageServiceImpl::new(repo_service.clone(), crate::test_support::nop_event_service());
        let handler = EnsureEnrichmentsHandler::new(repo_service, Arc::new(sms));
        handler.handle(serde_json::json!({})).await.unwrap();

        // Book 1 (missing) + book 2 (stale only) = 2 enqueue calls, not 3.
        assert_eq!(enqueue_count.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn noop_when_all_enriched() {
        let mut book_repo = MockBookRepository::new();

        book_repo
            .expect_find_book_ids_needing_enrichment()
            .returning(|_| Box::pin(std::future::ready(Ok(vec![]))));

        book_repo
            .expect_find_book_ids_needing_kepub_conversion()
            .returning(|_| Box::pin(std::future::ready(Ok(vec![]))));

        book_repo
            .expect_find_book_ids_with_stale_enrichment()
            .returning(|_| Box::pin(std::future::ready(Ok(vec![]))));

        let repo_service = Arc::new(default_repository_service_builder().book_repository(Arc::new(book_repo)).build().unwrap());

        let sms = crate::message::SystemMessageServiceImpl::new(repo_service.clone(), crate::test_support::nop_event_service());
        let handler = EnsureEnrichmentsHandler::new(repo_service, Arc::new(sms));
        handler.handle(serde_json::json!({})).await.unwrap();
    }
}
