use std::sync::Arc;

use crate::{Error, auth::AuthService, jobs::JobHandler};

pub struct CleanupExpiredSessionsHandler {
    auth_service: Arc<dyn AuthService>,
}

impl CleanupExpiredSessionsHandler {
    #[must_use]
    pub fn new(auth_service: Arc<dyn AuthService>) -> Self {
        Self { auth_service }
    }
}

impl JobHandler for CleanupExpiredSessionsHandler {
    const JOB_TYPE: &'static str = "health.cleanup_expired_sessions";
    type Payload = serde_json::Value;

    async fn handle(&self, _payload: serde_json::Value) -> Result<(), Error> {
        let deleted = self.auth_service.delete_by_expiry().await?;

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
    use crate::auth::service::MockAuthService;

    #[tokio::test]
    async fn deletes_expired_sessions() {
        let mut auth = MockAuthService::new();

        auth.expect_delete_by_expiry()
            .returning(|| Box::pin(std::future::ready(Ok(vec!["s1".into(), "s2".into(), "s3".into()]))));

        let handler = CleanupExpiredSessionsHandler::new(Arc::new(auth));
        handler.handle(serde_json::json!({})).await.unwrap();
    }

    #[tokio::test]
    async fn noop_when_no_expired_sessions() {
        let mut auth = MockAuthService::new();

        auth.expect_delete_by_expiry().returning(|| Box::pin(std::future::ready(Ok(vec![]))));

        let handler = CleanupExpiredSessionsHandler::new(Arc::new(auth));
        handler.handle(serde_json::json!({})).await.unwrap();
    }
}
