use std::sync::Arc;

use crate::{
    CoreServices, Error,
    format::handler::EnrichBookFilesPayload,
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

        // Deduplicate: the unified enrichment job handles both epub and kepub.
        let mut all_ids = enrichment_ids.clone();
        for id in &kepub_ids {
            if !all_ids.contains(id) {
                all_ids.push(*id);
            }
        }

        if all_ids.is_empty() {
            tracing::info!("no enrichment recovery needed");
            return Ok(());
        }

        let count = all_ids.len();
        let job_repo = self.core.repository_service.job_repository().clone();
        transaction(&**self.core.repository_service.repository(), |tx| {
            let job_repo = job_repo.clone();
            let all_ids = all_ids.clone();
            Box::pin(async move {
                for book_id in all_ids {
                    job_repo.enqueue(tx, &EnrichBookFilesPayload { book_id }).await?;
                }
                Ok(())
            })
        })
        .await?;

        tracing::info!(count, "enqueued enrichment recovery jobs");

        self.core
            .system_message_service
            .add_message(NewSystemMessage {
                source_task: Self::JOB_TYPE.to_string(),
                severity: MessageSeverity::Info,
                message: format!("Recovered {count} enrichment job(s)"),
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
