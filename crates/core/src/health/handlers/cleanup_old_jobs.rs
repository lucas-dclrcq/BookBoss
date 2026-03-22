use std::sync::Arc;

use chrono::{Duration, Utc};

use crate::{
    Error,
    jobs::JobHandler,
    message::{MessageSeverity, NewSystemMessage, SystemMessageService},
    repository::{RepositoryService, transaction},
};

pub struct CleanupOldJobsHandler {
    repository_service: Arc<RepositoryService>,
    system_message_service: Arc<dyn SystemMessageService>,
}

impl CleanupOldJobsHandler {
    #[must_use]
    pub fn new(repository_service: Arc<RepositoryService>, system_message_service: Arc<dyn SystemMessageService>) -> Self {
        Self {
            repository_service,
            system_message_service,
        }
    }
}

impl JobHandler for CleanupOldJobsHandler {
    const JOB_TYPE: &'static str = "health.cleanup_old_jobs";
    type Payload = serde_json::Value;

    async fn handle(&self, _payload: serde_json::Value) -> Result<(), Error> {
        let cutoff = Utc::now() - Duration::days(7);

        let job_repo = self.repository_service.job_repository().clone();
        let deleted = transaction(&**self.repository_service.repository(), |tx| {
            let job_repo = job_repo.clone();
            Box::pin(async move { job_repo.delete_old_jobs(tx, cutoff).await })
        })
        .await?;

        if deleted > 0 {
            tracing::info!(count = deleted, "deleted old completed/failed jobs");

            self.system_message_service
                .add_message(NewSystemMessage {
                    source_task: Self::JOB_TYPE.to_string(),
                    severity: MessageSeverity::Info,
                    message: format!("Cleaned up {deleted} old job(s)"),
                })
                .await?;
        } else {
            tracing::info!("no old jobs to clean up");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{jobs::repository::MockJobRepository, repository::testing::default_repository_service_builder};

    #[tokio::test]
    async fn deletes_old_jobs_and_logs_message() {
        use crate::message::repository::MockSystemMessageRepository;

        let mut job_repo = MockJobRepository::new();
        let mut msg_repo = MockSystemMessageRepository::new();

        job_repo.expect_delete_old_jobs().returning(|_, _| Box::pin(std::future::ready(Ok(5))));

        msg_repo.expect_add_message().returning(|_, msg| {
            assert!(msg.message.contains('5'));
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
                .job_repository(Arc::new(job_repo))
                .system_message_repository(Arc::new(msg_repo))
                .build()
                .unwrap(),
        );

        let sms = crate::message::SystemMessageServiceImpl::new(repo_service.clone(), crate::test_support::nop_event_service());
        let handler = CleanupOldJobsHandler::new(repo_service, Arc::new(sms));
        handler.handle(serde_json::json!({})).await.unwrap();
    }

    #[tokio::test]
    async fn noop_when_no_old_jobs() {
        let mut job_repo = MockJobRepository::new();

        job_repo.expect_delete_old_jobs().returning(|_, _| Box::pin(std::future::ready(Ok(0))));

        let repo_service = Arc::new(default_repository_service_builder().job_repository(Arc::new(job_repo)).build().unwrap());

        let sms = crate::message::SystemMessageServiceImpl::new(repo_service.clone(), crate::test_support::nop_event_service());
        let handler = CleanupOldJobsHandler::new(repo_service, Arc::new(sms));
        handler.handle(serde_json::json!({})).await.unwrap();
    }
}
