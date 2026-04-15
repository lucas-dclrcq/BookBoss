use bb_core::{
    Error,
    app_setting::{AppSetting, AppSettingRepository, NewAppSetting},
    repository::Transaction,
};

pub(crate) struct AppSettingRepositoryAdapter;

impl AppSettingRepositoryAdapter {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl AppSettingRepository for AppSettingRepositoryAdapter {
    async fn get(&self, _tx: &dyn Transaction, _key: &str) -> Result<Option<AppSetting>, Error> {
        Err(Error::Infrastructure("AppSettingRepositoryAdapter not yet implemented".into()))
    }

    async fn set(&self, _tx: &dyn Transaction, _setting: NewAppSetting) -> Result<AppSetting, Error> {
        Err(Error::Infrastructure("AppSettingRepositoryAdapter not yet implemented".into()))
    }
}
