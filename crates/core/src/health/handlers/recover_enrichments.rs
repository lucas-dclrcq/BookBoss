use std::sync::Arc;

use crate::{
    CoreServices, Error,
    jobs::{JobHandler, JobRepositoryExt},
    message::{MessageSeverity, NewSystemMessage},
    repository::{read_only_transaction, transaction},
};

pub struct RecoverEnrichmentsHandler {
    core: Arc<CoreServices>,
}

impl RecoverEnrichmentsHandler {
    #[must_use]
    pub fn new(core: Arc<CoreServices>) -> Self {
        Self { core }
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

impl JobHandler for RecoverEnrichmentsHandler {
    const JOB_TYPE: &'static str = "health.recover_enrichments";
    type Payload = serde_json::Value;

    async fn handle(&self, _payload: serde_json::Value) -> Result<(), Error> {
        let book_repo = self.core.repository_service.book_repository().clone();

        // Find books needing EPUB enrichment.
        let enrichment_ids = read_only_transaction(&**self.core.repository_service.repository(), |tx| {
            let book_repo = book_repo.clone();
            Box::pin(async move { book_repo.find_book_ids_needing_enrichment(tx).await })
        })
        .await?;

        // Find books needing KEPUB conversion.
        let book_repo = self.core.repository_service.book_repository().clone();
        let kepub_ids = read_only_transaction(&**self.core.repository_service.repository(), |tx| {
            let book_repo = book_repo.clone();
            Box::pin(async move { book_repo.find_book_ids_needing_kepub_conversion(tx).await })
        })
        .await?;

        let enrich_count = enrichment_ids.len();
        let kepub_count = kepub_ids.len();

        if enrich_count == 0 && kepub_count == 0 {
            tracing::info!("no enrichment or KEPUB recovery needed");
            return Ok(());
        }

        // Enqueue enrichment jobs.
        if !enrichment_ids.is_empty() {
            let job_repo = self.core.repository_service.job_repository().clone();
            transaction(&**self.core.repository_service.repository(), |tx| {
                let job_repo = job_repo.clone();
                let enrichment_ids = enrichment_ids.clone();
                Box::pin(async move {
                    for book_id in enrichment_ids {
                        job_repo.enqueue(tx, &EnrichPayload { book_id }).await?;
                    }
                    Ok(())
                })
            })
            .await?;

            tracing::info!(count = enrich_count, "enqueued enrichment recovery jobs");
        }

        // Enqueue KEPUB conversion jobs.
        if !kepub_ids.is_empty() {
            let job_repo = self.core.repository_service.job_repository().clone();
            transaction(&**self.core.repository_service.repository(), |tx| {
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

            tracing::info!(count = kepub_count, "enqueued KEPUB recovery jobs");
        }

        let mut parts = Vec::new();
        if enrich_count > 0 {
            parts.push(format!("{enrich_count} enrichment"));
        }
        if kepub_count > 0 {
            parts.push(format!("{kepub_count} KEPUB conversion"));
        }

        self.core
            .system_message_service
            .add_message(NewSystemMessage {
                source_task: Self::JOB_TYPE.to_string(),
                severity: MessageSeverity::Info,
                message: format!("Recovered {} job(s)", parts.join(" and ")),
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
        repository::testing::default_repository_service_builder, test_support::*,
    };

    #[tokio::test]
    async fn enqueues_enrichment_and_kepub_jobs() {
        let mut book_repo = MockBookRepository::new();
        let mut job_repo = MockJobRepository::new();
        let mut msg_repo = MockSystemMessageRepository::new();

        book_repo
            .expect_find_book_ids_needing_enrichment()
            .returning(|_| Box::pin(std::future::ready(Ok(vec![1, 2]))));

        book_repo
            .expect_find_book_ids_needing_kepub_conversion()
            .returning(|_| Box::pin(std::future::ready(Ok(vec![3]))));

        job_repo.expect_enqueue_raw().returning(|_, _, _, _| {
            Box::pin(std::future::ready(Ok(crate::jobs::Job {
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
            })))
        });

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

        let repo_service = Arc::new(
            default_repository_service_builder()
                .book_repository(Arc::new(book_repo))
                .job_repository(Arc::new(job_repo))
                .system_message_repository(Arc::new(msg_repo))
                .build()
                .unwrap(),
        );

        let core = crate::create_services(
            default_external_services_builder().repository_service(repo_service).build().unwrap(),
            "test-secret",
        )
        .unwrap();

        let handler = RecoverEnrichmentsHandler::new(core);
        handler.handle(serde_json::json!({})).await.unwrap();
    }

    #[tokio::test]
    async fn noop_when_nothing_to_recover() {
        let mut book_repo = MockBookRepository::new();

        book_repo
            .expect_find_book_ids_needing_enrichment()
            .returning(|_| Box::pin(std::future::ready(Ok(vec![]))));

        book_repo
            .expect_find_book_ids_needing_kepub_conversion()
            .returning(|_| Box::pin(std::future::ready(Ok(vec![]))));

        let repo_service = Arc::new(default_repository_service_builder().book_repository(Arc::new(book_repo)).build().unwrap());

        let core = crate::create_services(
            default_external_services_builder().repository_service(repo_service).build().unwrap(),
            "test-secret",
        )
        .unwrap();

        let handler = RecoverEnrichmentsHandler::new(core);
        handler.handle(serde_json::json!({})).await.unwrap();
    }
}
