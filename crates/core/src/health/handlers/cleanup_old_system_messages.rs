use std::sync::Arc;

use chrono::{Duration, Utc};

use crate::{Error, jobs::JobHandler, message::SystemMessageService};

pub struct CleanupOldSystemMessagesHandler {
    system_message_service: Arc<dyn SystemMessageService>,
}

impl CleanupOldSystemMessagesHandler {
    #[must_use]
    pub fn new(system_message_service: Arc<dyn SystemMessageService>) -> Self {
        Self { system_message_service }
    }
}

impl JobHandler for CleanupOldSystemMessagesHandler {
    const JOB_TYPE: &'static str = "health.cleanup_old_system_messages";
    type Payload = serde_json::Value;

    async fn handle(&self, _payload: serde_json::Value) -> Result<(), Error> {
        let cutoff = Utc::now() - Duration::days(30);

        let deleted = self.system_message_service.delete_older_than(cutoff).await?;

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
    use crate::message::service::MockSystemMessageService;

    #[tokio::test]
    async fn deletes_old_messages() {
        let mut sms = MockSystemMessageService::new();

        sms.expect_delete_older_than().returning(|_| Box::pin(std::future::ready(Ok(10))));

        let handler = CleanupOldSystemMessagesHandler::new(Arc::new(sms));
        handler.handle(serde_json::json!({})).await.unwrap();
    }

    #[tokio::test]
    async fn noop_when_no_old_messages() {
        let mut sms = MockSystemMessageService::new();

        sms.expect_delete_older_than().returning(|_| Box::pin(std::future::ready(Ok(0))));

        let handler = CleanupOldSystemMessagesHandler::new(Arc::new(sms));
        handler.handle(serde_json::json!({})).await.unwrap();
    }
}
