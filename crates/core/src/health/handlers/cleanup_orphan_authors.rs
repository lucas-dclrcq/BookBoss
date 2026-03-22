use std::sync::Arc;

use crate::{
    Error,
    jobs::JobHandler,
    message::{MessageSeverity, NewSystemMessage, SystemMessageService},
    repository::{RepositoryService, read_only_transaction, transaction},
};

pub struct CleanupOrphanAuthorsHandler {
    repository_service: Arc<RepositoryService>,
    system_message_service: Arc<dyn SystemMessageService>,
}

impl CleanupOrphanAuthorsHandler {
    #[must_use]
    pub fn new(repository_service: Arc<RepositoryService>, system_message_service: Arc<dyn SystemMessageService>) -> Self {
        Self {
            repository_service,
            system_message_service,
        }
    }
}

impl JobHandler for CleanupOrphanAuthorsHandler {
    const JOB_TYPE: &'static str = "health.cleanup_orphan_authors";
    type Payload = serde_json::Value;

    async fn handle(&self, _payload: serde_json::Value) -> Result<(), Error> {
        let author_repo = self.repository_service.author_repository().clone();
        let book_repo = self.repository_service.book_repository().clone();

        // Find all authors.
        let authors = read_only_transaction(&**self.repository_service.repository(), |tx| {
            let author_repo = author_repo.clone();
            Box::pin(async move { author_repo.list_all_authors(tx).await })
        })
        .await?;

        // Check each author for books and collect orphans.
        let mut orphan_ids = Vec::new();
        for author in &authors {
            let author_id = author.id;
            let count = read_only_transaction(&**self.repository_service.repository(), |tx| {
                let book_repo = book_repo.clone();
                Box::pin(async move { book_repo.count_books_for_author(tx, author_id).await })
            })
            .await?;

            if count == 0 {
                orphan_ids.push(author_id);
            }
        }

        if orphan_ids.is_empty() {
            tracing::info!("no orphan authors found");
            return Ok(());
        }

        // Delete orphan authors.
        let delete_count = orphan_ids.len();
        let author_repo = self.repository_service.author_repository().clone();
        transaction(&**self.repository_service.repository(), |tx| {
            let author_repo = author_repo.clone();
            let orphan_ids = orphan_ids.clone();
            Box::pin(async move {
                for id in orphan_ids {
                    author_repo.delete_author(tx, id).await?;
                }
                Ok(())
            })
        })
        .await?;

        tracing::info!(count = delete_count, "deleted orphan authors");

        self.system_message_service
            .add_message(NewSystemMessage {
                source_task: Self::JOB_TYPE.to_string(),
                severity: MessageSeverity::Info,
                message: format!("Cleaned up {delete_count} orphan author(s)"),
            })
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        book::{Author, AuthorId},
        repository::testing::default_repository_service_builder,
    };

    fn make_author(id: AuthorId, name: &str) -> Author {
        Author {
            id,
            version: 0,
            token: crate::book::AuthorToken::new(id),
            name: name.to_string(),
            bio: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn deletes_authors_with_no_books() {
        use crate::{
            book::repository::{author::MockAuthorRepository, book::MockBookRepository},
            message::repository::MockSystemMessageRepository,
        };

        let mut author_repo = MockAuthorRepository::new();
        let mut book_repo = MockBookRepository::new();
        let mut msg_repo = MockSystemMessageRepository::new();

        let orphan = make_author(1, "Orphan Author");

        author_repo
            .expect_list_all_authors()
            .returning(move |_| Box::pin(std::future::ready(Ok(vec![orphan.clone()]))));

        book_repo.expect_count_books_for_author().returning(|_, _| Box::pin(std::future::ready(Ok(0))));

        author_repo.expect_delete_author().returning(|_, _| Box::pin(std::future::ready(Ok(()))));

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
                .author_repository(Arc::new(author_repo))
                .book_repository(Arc::new(book_repo))
                .system_message_repository(Arc::new(msg_repo))
                .build()
                .unwrap(),
        );

        let sms = crate::message::SystemMessageServiceImpl::new(repo_service.clone());

        let handler = CleanupOrphanAuthorsHandler::new(repo_service, Arc::new(sms));
        handler.handle(serde_json::json!({})).await.unwrap();
    }

    #[tokio::test]
    async fn skips_authors_with_books() {
        use crate::book::repository::{author::MockAuthorRepository, book::MockBookRepository};

        let mut author_repo = MockAuthorRepository::new();
        let mut book_repo = MockBookRepository::new();

        let author = make_author(1, "Active Author");

        author_repo
            .expect_list_all_authors()
            .returning(move |_| Box::pin(std::future::ready(Ok(vec![author.clone()]))));

        book_repo.expect_count_books_for_author().returning(|_, _| Box::pin(std::future::ready(Ok(3))));

        // delete_author should NOT be called
        author_repo.expect_delete_author().never();

        let repo_service = Arc::new(
            default_repository_service_builder()
                .author_repository(Arc::new(author_repo))
                .book_repository(Arc::new(book_repo))
                .build()
                .unwrap(),
        );

        let sms = crate::message::SystemMessageServiceImpl::new(repo_service.clone());

        let handler = CleanupOrphanAuthorsHandler::new(repo_service, Arc::new(sms));
        handler.handle(serde_json::json!({})).await.unwrap();
    }
}
