use chrono::{DateTime, Utc};

use crate::{
    Error,
    message::{NewSystemMessage, SystemMessage, SystemMessageId},
    repository::Transaction,
};

#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait SystemMessageRepository: Send + Sync {
    async fn add_message(&self, tx: &dyn Transaction, msg: NewSystemMessage) -> Result<SystemMessage, Error>;
    async fn list_messages(&self, tx: &dyn Transaction) -> Result<Vec<SystemMessage>, Error>;
    async fn delete_message(&self, tx: &dyn Transaction, id: SystemMessageId) -> Result<(), Error>;
    async fn delete_all_messages(&self, tx: &dyn Transaction) -> Result<(), Error>;
    async fn delete_older_than(&self, tx: &dyn Transaction, cutoff: DateTime<Utc>) -> Result<u64, Error>;
}
