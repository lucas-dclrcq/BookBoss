use std::sync::Arc;

use chrono::{Duration, Utc};

use crate::{
    Error,
    jobs::JobHandler,
    message::{MessageSeverity, NewSystemMessage, SystemMessageService},
    repository::{RepositoryService, transaction},
};

pub struct CleanupOldImportJobsHandler {
    repository_service: Arc<RepositoryService>,
    system_message_service: Arc<dyn SystemMessageService>,
}

impl CleanupOldImportJobsHandler {
    #[must_use]
    pub fn new(repository_service: Arc<RepositoryService>, system_message_service: Arc<dyn SystemMessageService>) -> Self {
        Self {
            repository_service,
            system_message_service,
        }
    }
}

impl JobHandler for CleanupOldImportJobsHandler {
    const JOB_TYPE: &'static str = "health.cleanup_old_import_jobs";
    type Payload = serde_json::Value;

    async fn handle(&self, _payload: serde_json::Value) -> Result<(), Error> {
        let cutoff = Utc::now() - Duration::days(7);

        let import_job_repo = self.repository_service.import_job_repository().clone();
        let deleted = transaction(&**self.repository_service.repository(), |tx| {
            let import_job_repo = import_job_repo.clone();
            Box::pin(async move { import_job_repo.delete_old_terminal_jobs(tx, cutoff).await })
        })
        .await?;

        if deleted > 0 {
            tracing::info!(count = deleted, "deleted old terminal import jobs");

            self.system_message_service
                .add_message(NewSystemMessage {
                    source_task: Self::JOB_TYPE.to_string(),
                    severity: MessageSeverity::Info,
                    message: format!("Cleaned up {deleted} old import job(s)"),
                })
                .await?;
        } else {
            tracing::info!("no old import jobs to clean up");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{import::repository::import_job::MockImportJobRepository, repository::testing::default_repository_service_builder};

    #[tokio::test]
    async fn deletes_old_import_jobs_and_logs_message() {
        use crate::message::repository::MockSystemMessageRepository;

        let mut import_repo = MockImportJobRepository::new();
        let mut msg_repo = MockSystemMessageRepository::new();

        import_repo
            .expect_delete_old_terminal_jobs()
            .returning(|_, _| Box::pin(std::future::ready(Ok(3))));

        msg_repo.expect_add_message().returning(|_, msg| {
            assert!(msg.message.contains('3'));
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
                .import_job_repository(Arc::new(import_repo))
                .system_message_repository(Arc::new(msg_repo))
                .build()
                .unwrap(),
        );

        let sms = crate::message::SystemMessageServiceImpl::new(repo_service.clone(), crate::test_support::nop_event_service());
        let handler = CleanupOldImportJobsHandler::new(repo_service, Arc::new(sms));
        handler.handle(serde_json::json!({})).await.unwrap();
    }

    #[tokio::test]
    async fn noop_when_no_old_import_jobs() {
        let mut import_repo = MockImportJobRepository::new();

        import_repo
            .expect_delete_old_terminal_jobs()
            .returning(|_, _| Box::pin(std::future::ready(Ok(0))));

        let repo_service = Arc::new(
            default_repository_service_builder()
                .import_job_repository(Arc::new(import_repo))
                .build()
                .unwrap(),
        );

        let sms = crate::message::SystemMessageServiceImpl::new(repo_service.clone(), crate::test_support::nop_event_service());
        let handler = CleanupOldImportJobsHandler::new(repo_service, Arc::new(sms));
        handler.handle(serde_json::json!({})).await.unwrap();
    }
}
