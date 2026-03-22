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
