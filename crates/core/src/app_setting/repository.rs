use super::model::{AppSetting, NewAppSetting};
use crate::{Error, repository::Transaction};

#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait AppSettingRepository: Send + Sync {
    async fn get(&self, tx: &dyn Transaction, key: &str) -> Result<Option<AppSetting>, Error>;
    /// Upsert: inserts or replaces the setting for the given key.
    async fn set(&self, tx: &dyn Transaction, setting: NewAppSetting) -> Result<AppSetting, Error>;
}
