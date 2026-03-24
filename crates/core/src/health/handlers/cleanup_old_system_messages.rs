use std::sync::Arc;

use chrono::{Duration, Utc};

use crate::{CoreServices, Error, jobs::JobHandler};

pub struct CleanupOldSystemMessagesHandler {
    core: Arc<CoreServices>,
}

impl CleanupOldSystemMessagesHandler {
    #[must_use]
    pub fn new(core: Arc<CoreServices>) -> Self {
        Self { core }
    }
}

impl JobHandler for CleanupOldSystemMessagesHandler {
    const JOB_TYPE: &'static str = "health.cleanup_old_system_messages";
    type Payload = serde_json::Value;

    async fn handle(&self, _payload: serde_json::Value) -> Result<(), Error> {
        let cutoff = Utc::now() - Duration::days(30);

        let deleted = self.core.system_message_service.delete_older_than(cutoff).await?;

        if deleted > 0 {
            tracing::info!(count = deleted, "deleted old system messages");
        } else {
            tracing::info!("no old system messages to clean up");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{message::repository::MockSystemMessageRepository, repository::testing::default_repository_service_builder, test_support::*};

    #[tokio::test]
    async fn deletes_old_messages() {
        let mut msg_repo = MockSystemMessageRepository::new();

        msg_repo.expect_delete_older_than().returning(|_, _| Box::pin(std::future::ready(Ok(10))));

        let repo_service = Arc::new(
            default_repository_service_builder()
                .system_message_repository(Arc::new(msg_repo))
                .build()
                .unwrap(),
        );

        let core = crate::create_services(
            default_external_services_builder().repository_service(repo_service).build().unwrap(),
            "test-secret",
        )
        .unwrap();

        let handler = CleanupOldSystemMessagesHandler::new(core);
        handler.handle(serde_json::json!({})).await.unwrap();
    }

    #[tokio::test]
    async fn noop_when_no_old_messages() {
        let mut msg_repo = MockSystemMessageRepository::new();

        msg_repo.expect_delete_older_than().returning(|_, _| Box::pin(std::future::ready(Ok(0))));

        let repo_service = Arc::new(
            default_repository_service_builder()
                .system_message_repository(Arc::new(msg_repo))
                .build()
                .unwrap(),
        );

        let core = crate::create_services(
            default_external_services_builder().repository_service(repo_service).build().unwrap(),
            "test-secret",
        )
        .unwrap();

        let handler = CleanupOldSystemMessagesHandler::new(core);
        handler.handle(serde_json::json!({})).await.unwrap();
    }
}
