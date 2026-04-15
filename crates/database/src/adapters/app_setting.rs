use bb_core::{
    Error, RepositoryError,
    app_setting::{AppSetting, AppSettingRepository, NewAppSetting},
    repository::Transaction,
};
use chrono::Utc;
use sea_orm::{ActiveValue::Set, EntityTrait, sea_query::OnConflict};

use crate::{
    entities::{app_settings, prelude},
    error::handle_dberr,
    transaction::TransactionImpl,
};

impl From<app_settings::Model> for AppSetting {
    fn from(model: app_settings::Model) -> Self {
        Self {
            key: model.key,
            value: model.value,
        }
    }
}

pub(crate) struct AppSettingRepositoryAdapter;

impl AppSettingRepositoryAdapter {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl AppSettingRepository for AppSettingRepositoryAdapter {
    async fn get(&self, tx: &dyn Transaction, key: &str) -> Result<Option<AppSetting>, Error> {
        let transaction = TransactionImpl::get_db_transaction(tx)?;

        Ok(prelude::AppSettings::find_by_id(key.to_owned())
            .one(transaction)
            .await
            .map_err(handle_dberr)?
            .map(Into::into))
    }

    async fn set(&self, tx: &dyn Transaction, setting: NewAppSetting) -> Result<AppSetting, Error> {
        let transaction = TransactionImpl::get_db_transaction(tx)?;
        let now = Utc::now();

        let active_model = app_settings::ActiveModel {
            key: Set(setting.key.clone()),
            value: Set(setting.value.clone()),
            updated_at: Set(now.into()),
        };

        prelude::AppSettings::insert(active_model)
            .on_conflict(
                OnConflict::column(app_settings::Column::Key)
                    .update_columns([app_settings::Column::Value, app_settings::Column::UpdatedAt])
                    .to_owned(),
            )
            .exec(transaction)
            .await
            .map_err(handle_dberr)?;

        prelude::AppSettings::find_by_id(setting.key)
            .one(transaction)
            .await
            .map_err(handle_dberr)?
            .map(Into::into)
            .ok_or(Error::RepositoryError(RepositoryError::NotFound))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bb_core::{app_setting::NewAppSetting, repository::RepositoryService};
    use sea_orm::Database;

    use crate::create_repository_service;

    async fn setup() -> Arc<RepositoryService> {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        create_repository_service(db).await.unwrap()
    }

    #[tokio::test]
    async fn set_creates_new_setting() {
        let svc = setup().await;
        let tx = svc.repository().begin().await.unwrap();

        let setting = svc
            .app_setting_repository()
            .set(
                &*tx,
                NewAppSetting {
                    key: "enrichment.mobi_enabled".into(),
                    value: "true".into(),
                },
            )
            .await
            .unwrap();

        assert_eq!(setting.key, "enrichment.mobi_enabled");
        assert_eq!(setting.value, "true");
    }

    #[tokio::test]
    async fn set_updates_existing() {
        let svc = setup().await;
        let tx = svc.repository().begin().await.unwrap();

        svc.app_setting_repository()
            .set(
                &*tx,
                NewAppSetting {
                    key: "enrichment.mobi_enabled".into(),
                    value: "false".into(),
                },
            )
            .await
            .unwrap();

        let updated = svc
            .app_setting_repository()
            .set(
                &*tx,
                NewAppSetting {
                    key: "enrichment.mobi_enabled".into(),
                    value: "true".into(),
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.value, "true");
    }

    #[tokio::test]
    async fn get_returns_none_for_missing() {
        let svc = setup().await;
        let tx = svc.repository().begin().await.unwrap();

        let result = svc.app_setting_repository().get(&*tx, "nonexistent").await.unwrap();

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn get_returns_value_after_set() {
        let svc = setup().await;
        let tx = svc.repository().begin().await.unwrap();

        svc.app_setting_repository()
            .set(
                &*tx,
                NewAppSetting {
                    key: "foo".into(),
                    value: "bar".into(),
                },
            )
            .await
            .unwrap();

        let result = svc.app_setting_repository().get(&*tx, "foo").await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().value, "bar");
    }
}
