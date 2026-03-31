use std::sync::Arc;

use crate::{CoreServices, Error, jobs::JobHandler};

pub struct CleanupExpiredSessionsHandler {
    core: Arc<CoreServices>,
}

impl CleanupExpiredSessionsHandler {
    #[must_use]
    pub fn new(core: Arc<CoreServices>) -> Self {
        Self { core }
    }
}

impl JobHandler for CleanupExpiredSessionsHandler {
    const JOB_TYPE: &'static str = "health.cleanup_expired_sessions";
    const DISPLAY_NAME: &'static str = "Cleanup Expired Sessions";
    type Payload = serde_json::Value;

    async fn handle(&self, _payload: serde_json::Value) -> Result<(), Error> {
        let deleted = self.core.auth_service.delete_by_expiry().await?;

        if deleted.is_empty() {
            tracing::info!("no stale sessions to clean up");
        } else {
            tracing::info!(count = deleted.len(), "deleted stale sessions");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{auth::repository::MockSessionRepository, repository::testing::default_repository_service_builder, test_support::*};

    #[tokio::test]
    async fn deletes_expired_sessions() {
        let mut session_repo = MockSessionRepository::new();

        session_repo
            .expect_delete_by_expiry()
            .returning(|_| Box::pin(std::future::ready(Ok(vec!["s1".into(), "s2".into(), "s3".into()]))));

        let repo_service = Arc::new(default_repository_service_builder().session_repository(Arc::new(session_repo)).build().unwrap());

        let core = crate::create_services(
            default_external_services_builder().repository_service(repo_service).build().unwrap(),
            "test-secret",
        )
        .unwrap();

        let handler = CleanupExpiredSessionsHandler::new(core);
        handler.handle(serde_json::json!({})).await.unwrap();
    }

    #[tokio::test]
    async fn noop_when_no_expired_sessions() {
        let mut session_repo = MockSessionRepository::new();

        session_repo.expect_delete_by_expiry().returning(|_| Box::pin(std::future::ready(Ok(vec![]))));

        let repo_service = Arc::new(default_repository_service_builder().session_repository(Arc::new(session_repo)).build().unwrap());

        let core = crate::create_services(
            default_external_services_builder().repository_service(repo_service).build().unwrap(),
            "test-secret",
        )
        .unwrap();

        let handler = CleanupExpiredSessionsHandler::new(core);
        handler.handle(serde_json::json!({})).await.unwrap();
    }
}
