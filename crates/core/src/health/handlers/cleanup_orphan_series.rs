use std::sync::Arc;

use crate::{
    Error,
    jobs::JobHandler,
    message::{MessageSeverity, NewSystemMessage, SystemMessageService},
    repository::{RepositoryService, read_only_transaction, transaction},
};

pub struct CleanupOrphanSeriesHandler {
    repository_service: Arc<RepositoryService>,
    system_message_service: Arc<dyn SystemMessageService>,
}

impl CleanupOrphanSeriesHandler {
    #[must_use]
    pub fn new(repository_service: Arc<RepositoryService>, system_message_service: Arc<dyn SystemMessageService>) -> Self {
        Self {
            repository_service,
            system_message_service,
        }
    }
}

impl JobHandler for CleanupOrphanSeriesHandler {
    const JOB_TYPE: &'static str = "health.cleanup_orphan_series";
    type Payload = serde_json::Value;

    async fn handle(&self, _payload: serde_json::Value) -> Result<(), Error> {
        let series_repo = self.repository_service.series_repository().clone();

        let all_series = read_only_transaction(&**self.repository_service.repository(), |tx| {
            let series_repo = series_repo.clone();
            Box::pin(async move { series_repo.list_all_series(tx).await })
        })
        .await?;

        let mut orphan_ids = Vec::new();
        for s in &all_series {
            let series_id = s.id;
            let series_repo = self.repository_service.series_repository().clone();
            let count = read_only_transaction(&**self.repository_service.repository(), |tx| {
                let series_repo = series_repo.clone();
                Box::pin(async move { series_repo.count_books_for_series(tx, series_id).await })
            })
            .await?;

            if count == 0 {
                orphan_ids.push(series_id);
            }
        }

        if orphan_ids.is_empty() {
            tracing::info!("no orphan series found");
            return Ok(());
        }

        let delete_count = orphan_ids.len();
        let series_repo = self.repository_service.series_repository().clone();
        transaction(&**self.repository_service.repository(), |tx| {
            let series_repo = series_repo.clone();
            let orphan_ids = orphan_ids.clone();
            Box::pin(async move {
                for id in orphan_ids {
                    series_repo.delete_series(tx, id).await?;
                }
                Ok(())
            })
        })
        .await?;

        tracing::info!(count = delete_count, "deleted orphan series");

        self.system_message_service
            .add_message(NewSystemMessage {
                source_task: Self::JOB_TYPE.to_string(),
                severity: MessageSeverity::Info,
                message: format!("Cleaned up {delete_count} orphan series"),
            })
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        book::{Series, SeriesId, SeriesToken},
        repository::testing::default_repository_service_builder,
    };

    fn make_series(id: SeriesId, name: &str) -> Series {
        Series {
            id,
            version: 0,
            token: SeriesToken::new(id),
            name: name.to_string(),
            description: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn deletes_series_with_no_books() {
        use crate::{book::repository::series::MockSeriesRepository, message::repository::MockSystemMessageRepository};

        let mut series_repo = MockSeriesRepository::new();
        let mut msg_repo = MockSystemMessageRepository::new();

        let orphan = make_series(1, "Orphan Series");

        series_repo
            .expect_list_all_series()
            .returning(move |_| Box::pin(std::future::ready(Ok(vec![orphan.clone()]))));

        series_repo
            .expect_count_books_for_series()
            .returning(|_, _| Box::pin(std::future::ready(Ok(0))));

        series_repo.expect_delete_series().returning(|_, _| Box::pin(std::future::ready(Ok(()))));

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
                .series_repository(Arc::new(series_repo))
                .system_message_repository(Arc::new(msg_repo))
                .build()
                .unwrap(),
        );

        let sms = crate::message::SystemMessageServiceImpl::new(repo_service.clone());
        let handler = CleanupOrphanSeriesHandler::new(repo_service, Arc::new(sms));
        handler.handle(serde_json::json!({})).await.unwrap();
    }

    #[tokio::test]
    async fn skips_series_with_books() {
        use crate::book::repository::series::MockSeriesRepository;

        let mut series_repo = MockSeriesRepository::new();

        let active = make_series(1, "Active Series");

        series_repo
            .expect_list_all_series()
            .returning(move |_| Box::pin(std::future::ready(Ok(vec![active.clone()]))));

        series_repo
            .expect_count_books_for_series()
            .returning(|_, _| Box::pin(std::future::ready(Ok(5))));

        series_repo.expect_delete_series().never();

        let repo_service = Arc::new(default_repository_service_builder().series_repository(Arc::new(series_repo)).build().unwrap());

        let sms = crate::message::SystemMessageServiceImpl::new(repo_service.clone());
        let handler = CleanupOrphanSeriesHandler::new(repo_service, Arc::new(sms));
        handler.handle(serde_json::json!({})).await.unwrap();
    }
}
