use std::sync::Arc;

use crate::{
    Error,
    jobs::JobHandler,
    message::{MessageSeverity, NewSystemMessage, SystemMessageService},
    repository::{RepositoryService, read_only_transaction},
    storage::FileStoreService,
};

pub struct VerifyFileIntegrityHandler {
    repository_service: Arc<RepositoryService>,
    system_message_service: Arc<dyn SystemMessageService>,
    file_store: Arc<dyn FileStoreService>,
}

impl VerifyFileIntegrityHandler {
    #[must_use]
    pub fn new(
        repository_service: Arc<RepositoryService>,
        system_message_service: Arc<dyn SystemMessageService>,
        file_store: Arc<dyn FileStoreService>,
    ) -> Self {
        Self {
            repository_service,
            system_message_service,
            file_store,
        }
    }
}

impl JobHandler for VerifyFileIntegrityHandler {
    const JOB_TYPE: &'static str = "health.verify_file_integrity";
    type Payload = serde_json::Value;

    async fn handle(&self, _payload: serde_json::Value) -> Result<(), Error> {
        let book_repo = self.repository_service.book_repository().clone();

        let all_files = read_only_transaction(&**self.repository_service.repository(), |tx| {
            let book_repo = book_repo.clone();
            Box::pin(async move { book_repo.list_all_book_files(tx).await })
        })
        .await?;

        let mut missing = Vec::new();

        for file in &all_files {
            let abs_path = self.file_store.resolve(&file.path);
            if !abs_path.exists() {
                missing.push(format!("book_id={}, path={}", file.book_id, file.path));
            }
        }

        if missing.is_empty() {
            tracing::info!(total = all_files.len(), "all library files verified");
            self.system_message_service
                .add_message(NewSystemMessage {
                    source_task: Self::JOB_TYPE.to_string(),
                    severity: MessageSeverity::Info,
                    message: format!("All {} library file(s) verified on disk", all_files.len()),
                })
                .await?;
        } else {
            let count = missing.len();
            tracing::warn!(count, "missing library files detected");

            for entry in &missing {
                tracing::warn!(entry, "missing file");
            }

            self.system_message_service
                .add_message(NewSystemMessage {
                    source_task: Self::JOB_TYPE.to_string(),
                    severity: MessageSeverity::Warning,
                    message: format!("{count} library file(s) missing from disk"),
                })
                .await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::{
        book::{BookFile, FileFormat, FileRole, repository::book::MockBookRepository},
        message::repository::MockSystemMessageRepository,
        repository::testing::default_repository_service_builder,
        storage::MockFileStoreService,
    };

    #[tokio::test]
    async fn reports_info_when_all_files_exist() {
        let mut book_repo = MockBookRepository::new();
        let mut msg_repo = MockSystemMessageRepository::new();
        let mut store = MockFileStoreService::new();

        book_repo.expect_list_all_book_files().returning(|_| {
            Box::pin(std::future::ready(Ok(vec![BookFile {
                book_id: 1,
                format: FileFormat::Epub,
                file_role: FileRole::Original,
                path: "BK_abc/book.epub".to_string(),
                file_size: 1000,
                file_hash: "hash123".to_string(),
                created_at: chrono::Utc::now(),
            }])))
        });

        // Return a path that exists (use current executable as a stand-in).
        store.expect_resolve().returning(|_| {
            // Use a path that is guaranteed to exist.
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")
        });

        msg_repo.expect_add_message().returning(|_, msg| {
            assert_eq!(msg.severity, MessageSeverity::Info);
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
                .system_message_repository(Arc::new(msg_repo))
                .build()
                .unwrap(),
        );

        let sms = crate::message::SystemMessageServiceImpl::new(repo_service.clone(), crate::test_support::nop_event_service());
        let handler = VerifyFileIntegrityHandler::new(repo_service, Arc::new(sms), Arc::new(store));
        handler.handle(serde_json::json!({})).await.unwrap();
    }

    #[tokio::test]
    async fn reports_warning_when_files_missing() {
        let mut book_repo = MockBookRepository::new();
        let mut msg_repo = MockSystemMessageRepository::new();
        let mut store = MockFileStoreService::new();

        book_repo.expect_list_all_book_files().returning(|_| {
            Box::pin(std::future::ready(Ok(vec![BookFile {
                book_id: 1,
                format: FileFormat::Epub,
                file_role: FileRole::Original,
                path: "BK_abc/book.epub".to_string(),
                file_size: 1000,
                file_hash: "hash123".to_string(),
                created_at: chrono::Utc::now(),
            }])))
        });

        // Return a path that does NOT exist.
        store.expect_resolve().returning(|_| PathBuf::from("/nonexistent/path/to/file.epub"));

        msg_repo.expect_add_message().returning(|_, msg| {
            assert_eq!(msg.severity, MessageSeverity::Warning);
            assert!(msg.message.contains("missing"));
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
                .system_message_repository(Arc::new(msg_repo))
                .build()
                .unwrap(),
        );

        let sms = crate::message::SystemMessageServiceImpl::new(repo_service.clone(), crate::test_support::nop_event_service());
        let handler = VerifyFileIntegrityHandler::new(repo_service, Arc::new(sms), Arc::new(store));
        handler.handle(serde_json::json!({})).await.unwrap();
    }
}
