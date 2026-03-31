use bb_core::{
    Error,
    message::{MessageSeverity, NewSystemMessage, SystemMessage, SystemMessageId, SystemMessageRepository},
    repository::Transaction,
};
use chrono::{DateTime, Utc};
use sea_orm::{ActiveModelBehavior, ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::{
    entities::{prelude, system_messages},
    error::handle_dberr,
    transaction::TransactionImpl,
};

fn str_to_severity(s: &str) -> MessageSeverity {
    match s {
        "info" => MessageSeverity::Info,
        "warning" => MessageSeverity::Warning,
        "error" => MessageSeverity::Error,
        other => panic!("unknown message severity: {other}"),
    }
}

fn severity_to_str(s: MessageSeverity) -> &'static str {
    match s {
        MessageSeverity::Info => "info",
        MessageSeverity::Warning => "warning",
        MessageSeverity::Error => "error",
    }
}

impl From<system_messages::Model> for SystemMessage {
    fn from(m: system_messages::Model) -> Self {
        Self {
            id: m.id as u64,
            source_task: m.source_task,
            severity: str_to_severity(&m.severity),
            message: m.message,
            created_at: m.created_at.with_timezone(&Utc),
        }
    }
}

pub(crate) struct SystemMessageRepositoryAdapter;

impl SystemMessageRepositoryAdapter {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl SystemMessageRepository for SystemMessageRepositoryAdapter {
    async fn add_message(&self, tx: &dyn Transaction, msg: NewSystemMessage) -> Result<SystemMessage, Error> {
        let db_tx = TransactionImpl::get_db_transaction(tx)?;

        let model = system_messages::ActiveModel {
            source_task: Set(msg.source_task),
            severity: Set(severity_to_str(msg.severity).to_owned()),
            message: Set(msg.message),
            ..system_messages::ActiveModel::new()
        };

        let inserted = model.insert(db_tx).await.map_err(handle_dberr)?;
        Ok(inserted.into())
    }

    async fn list_messages(&self, tx: &dyn Transaction) -> Result<Vec<SystemMessage>, Error> {
        let db_tx = TransactionImpl::get_db_transaction(tx)?;

        let rows = prelude::SystemMessages::find()
            .order_by_desc(system_messages::Column::CreatedAt)
            .all(db_tx)
            .await
            .map_err(handle_dberr)?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn delete_message(&self, tx: &dyn Transaction, id: SystemMessageId) -> Result<(), Error> {
        let db_tx = TransactionImpl::get_db_transaction(tx)?;

        prelude::SystemMessages::delete_by_id(id as i64).exec(db_tx).await.map_err(handle_dberr)?;

        Ok(())
    }

    async fn delete_all_messages(&self, tx: &dyn Transaction) -> Result<(), Error> {
        let db_tx = TransactionImpl::get_db_transaction(tx)?;

        prelude::SystemMessages::delete_many().exec(db_tx).await.map_err(handle_dberr)?;

        Ok(())
    }

    async fn delete_older_than(&self, tx: &dyn Transaction, cutoff: DateTime<Utc>) -> Result<u64, Error> {
        let db_tx = TransactionImpl::get_db_transaction(tx)?;

        let result = prelude::SystemMessages::delete_many()
            .filter(system_messages::Column::CreatedAt.lt(cutoff.fixed_offset()))
            .exec(db_tx)
            .await
            .map_err(handle_dberr)?;

        Ok(result.rows_affected)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bb_core::{
        message::{MessageSeverity, NewSystemMessage},
        repository::RepositoryService,
    };
    use sea_orm::Database;

    use crate::create_repository_service;

    async fn setup() -> Arc<RepositoryService> {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        create_repository_service(db).await.unwrap()
    }

    fn new_msg(source: &str, severity: MessageSeverity, text: &str) -> NewSystemMessage {
        NewSystemMessage {
            source_task: source.to_owned(),
            severity,
            message: text.to_owned(),
        }
    }

    // ─── add_message ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_add_message_all_severities() {
        let svc = setup().await;
        let tx = svc.repository().begin().await.unwrap();

        for severity in [MessageSeverity::Info, MessageSeverity::Warning, MessageSeverity::Error] {
            let result = svc.system_message_repository().add_message(&*tx, new_msg("task", severity, "msg")).await;
            assert!(result.is_ok());
            let msg = result.unwrap();
            assert_ne!(msg.id, 0);
            assert_eq!(msg.severity, severity);
        }
    }

    // ─── list_messages ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_list_messages_empty() {
        let svc = setup().await;
        let tx = svc.repository().begin().await.unwrap();
        assert!(svc.system_message_repository().list_messages(&*tx).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_list_messages_ordered_desc() {
        let svc = setup().await;
        let tx = svc.repository().begin().await.unwrap();

        let first = svc
            .system_message_repository()
            .add_message(&*tx, new_msg("task", MessageSeverity::Info, "first"))
            .await
            .unwrap();
        let second = svc
            .system_message_repository()
            .add_message(&*tx, new_msg("task", MessageSeverity::Info, "second"))
            .await
            .unwrap();

        let list = svc.system_message_repository().list_messages(&*tx).await.unwrap();
        assert_eq!(list.len(), 2);
        // Most-recent first
        assert!(list[0].created_at >= list[1].created_at);
        // IDs: second was inserted after first
        assert_eq!(list[0].id, second.id);
        assert_eq!(list[1].id, first.id);
    }

    // ─── delete_message ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_delete_message() {
        let svc = setup().await;
        let tx = svc.repository().begin().await.unwrap();

        let msg = svc
            .system_message_repository()
            .add_message(&*tx, new_msg("task", MessageSeverity::Info, "to delete"))
            .await
            .unwrap();
        svc.system_message_repository().delete_message(&*tx, msg.id).await.unwrap();

        assert!(svc.system_message_repository().list_messages(&*tx).await.unwrap().is_empty());
    }

    // ─── delete_all_messages ─────────────────────────────────────────────────

    #[tokio::test]
    async fn test_delete_all_messages() {
        let svc = setup().await;
        let tx = svc.repository().begin().await.unwrap();

        svc.system_message_repository()
            .add_message(&*tx, new_msg("a", MessageSeverity::Info, "one"))
            .await
            .unwrap();
        svc.system_message_repository()
            .add_message(&*tx, new_msg("b", MessageSeverity::Warning, "two"))
            .await
            .unwrap();

        svc.system_message_repository().delete_all_messages(&*tx).await.unwrap();

        assert!(svc.system_message_repository().list_messages(&*tx).await.unwrap().is_empty());
    }

    // ─── delete_older_than ───────────────────────────────────────────────────

    #[tokio::test]
    async fn test_delete_older_than() {
        let svc = setup().await;
        let tx = svc.repository().begin().await.unwrap();

        let msg = svc
            .system_message_repository()
            .add_message(&*tx, new_msg("task", MessageSeverity::Info, "old"))
            .await
            .unwrap();

        // Cutoff after the message was created — should delete it
        let cutoff = msg.created_at + chrono::Duration::seconds(1);
        let deleted = svc.system_message_repository().delete_older_than(&*tx, cutoff).await.unwrap();

        assert_eq!(deleted, 1);
        assert!(svc.system_message_repository().list_messages(&*tx).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_delete_older_than_keeps_newer() {
        let svc = setup().await;
        let tx = svc.repository().begin().await.unwrap();

        let msg = svc
            .system_message_repository()
            .add_message(&*tx, new_msg("task", MessageSeverity::Info, "recent"))
            .await
            .unwrap();

        // Cutoff before the message — nothing deleted
        let cutoff = msg.created_at - chrono::Duration::seconds(1);
        let deleted = svc.system_message_repository().delete_older_than(&*tx, cutoff).await.unwrap();

        assert_eq!(deleted, 0);
        assert_eq!(svc.system_message_repository().list_messages(&*tx).await.unwrap().len(), 1);
    }
}
