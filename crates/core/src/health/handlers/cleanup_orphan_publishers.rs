use std::sync::Arc;

use crate::{
    Error,
    jobs::JobHandler,
    message::{MessageSeverity, NewSystemMessage, SystemMessageService},
    repository::{RepositoryService, read_only_transaction, transaction},
};

pub struct CleanupOrphanPublishersHandler {
    repository_service: Arc<RepositoryService>,
    system_message_service: Arc<dyn SystemMessageService>,
}

impl CleanupOrphanPublishersHandler {
    #[must_use]
    pub fn new(repository_service: Arc<RepositoryService>, system_message_service: Arc<dyn SystemMessageService>) -> Self {
        Self {
            repository_service,
            system_message_service,
        }
    }
}

impl JobHandler for CleanupOrphanPublishersHandler {
    const JOB_TYPE: &'static str = "health.cleanup_orphan_publishers";
    type Payload = serde_json::Value;

    async fn handle(&self, _payload: serde_json::Value) -> Result<(), Error> {
        let publisher_repo = self.repository_service.publisher_repository().clone();

        let all_publishers = read_only_transaction(&**self.repository_service.repository(), |tx| {
            let publisher_repo = publisher_repo.clone();
            Box::pin(async move { publisher_repo.list_all_publishers(tx).await })
        })
        .await?;

        let mut orphan_ids = Vec::new();
        for p in &all_publishers {
            let publisher_id = p.id;
            let publisher_repo = self.repository_service.publisher_repository().clone();
            let count = read_only_transaction(&**self.repository_service.repository(), |tx| {
                let publisher_repo = publisher_repo.clone();
                Box::pin(async move { publisher_repo.count_books_for_publisher(tx, publisher_id).await })
            })
            .await?;

            if count == 0 {
                orphan_ids.push(publisher_id);
            }
        }

        if orphan_ids.is_empty() {
            tracing::info!("no orphan publishers found");
            return Ok(());
        }

        let delete_count = orphan_ids.len();
        let publisher_repo = self.repository_service.publisher_repository().clone();
        transaction(&**self.repository_service.repository(), |tx| {
            let publisher_repo = publisher_repo.clone();
            let orphan_ids = orphan_ids.clone();
            Box::pin(async move {
                for id in orphan_ids {
                    publisher_repo.delete_publisher(tx, id).await?;
                }
                Ok(())
            })
        })
        .await?;

        tracing::info!(count = delete_count, "deleted orphan publishers");

        self.system_message_service
            .add_message(NewSystemMessage {
                source_task: Self::JOB_TYPE.to_string(),
                severity: MessageSeverity::Info,
                message: format!("Cleaned up {delete_count} orphan publisher(s)"),
            })
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        book::{Publisher, PublisherId, PublisherToken},
        repository::testing::default_repository_service_builder,
    };

    fn make_publisher(id: PublisherId, name: &str) -> Publisher {
        Publisher {
            id,
            version: 0,
            token: PublisherToken::new(id),
            name: name.to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn deletes_publishers_with_no_books() {
        use crate::{book::repository::publisher::MockPublisherRepository, message::repository::MockSystemMessageRepository};

        let mut pub_repo = MockPublisherRepository::new();
        let mut msg_repo = MockSystemMessageRepository::new();

        let orphan = make_publisher(1, "Orphan Publisher");

        pub_repo
            .expect_list_all_publishers()
            .returning(move |_| Box::pin(std::future::ready(Ok(vec![orphan.clone()]))));

        pub_repo
            .expect_count_books_for_publisher()
            .returning(|_, _| Box::pin(std::future::ready(Ok(0))));

        pub_repo.expect_delete_publisher().returning(|_, _| Box::pin(std::future::ready(Ok(()))));

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
                .publisher_repository(Arc::new(pub_repo))
                .system_message_repository(Arc::new(msg_repo))
                .build()
                .unwrap(),
        );

        let sms = crate::message::SystemMessageServiceImpl::new(repo_service.clone());
        let handler = CleanupOrphanPublishersHandler::new(repo_service, Arc::new(sms));
        handler.handle(serde_json::json!({})).await.unwrap();
    }

    #[tokio::test]
    async fn skips_publishers_with_books() {
        use crate::book::repository::publisher::MockPublisherRepository;

        let mut pub_repo = MockPublisherRepository::new();

        let active = make_publisher(1, "Active Publisher");

        pub_repo
            .expect_list_all_publishers()
            .returning(move |_| Box::pin(std::future::ready(Ok(vec![active.clone()]))));

        pub_repo
            .expect_count_books_for_publisher()
            .returning(|_, _| Box::pin(std::future::ready(Ok(3))));

        pub_repo.expect_delete_publisher().never();

        let repo_service = Arc::new(default_repository_service_builder().publisher_repository(Arc::new(pub_repo)).build().unwrap());

        let sms = crate::message::SystemMessageServiceImpl::new(repo_service.clone());
        let handler = CleanupOrphanPublishersHandler::new(repo_service, Arc::new(sms));
        handler.handle(serde_json::json!({})).await.unwrap();
    }
}
