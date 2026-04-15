use std::sync::Arc;

use crate::{
    Error,
    app_setting::{AppSetting, NewAppSetting},
    repository::RepositoryService,
    with_read_only_transaction, with_transaction,
};

const MOBI_ENABLED_KEY: &str = "enrichment.mobi_enabled";

#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
#[async_trait::async_trait]
pub trait AppSettingService: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<AppSetting>, Error>;
    async fn set(&self, key: &str, value: &str) -> Result<AppSetting, Error>;
    async fn mobi_enabled(&self) -> Result<bool, Error>;
}

pub(crate) struct AppSettingServiceImpl {
    repository_service: Arc<RepositoryService>,
}

impl AppSettingServiceImpl {
    pub(crate) fn new(repository_service: Arc<RepositoryService>) -> Self {
        Self { repository_service }
    }
}

#[async_trait::async_trait]
impl AppSettingService for AppSettingServiceImpl {
    async fn get(&self, key: &str) -> Result<Option<AppSetting>, Error> {
        let key = key.to_owned();
        with_read_only_transaction!(self, app_setting_repository, |tx| app_setting_repository.get(tx, &key).await)
    }

    async fn set(&self, key: &str, value: &str) -> Result<AppSetting, Error> {
        let setting = NewAppSetting {
            key: key.to_owned(),
            value: value.to_owned(),
        };
        with_transaction!(self, app_setting_repository, |tx| app_setting_repository.set(tx, setting).await)
    }

    async fn mobi_enabled(&self) -> Result<bool, Error> {
        let result = self.get(MOBI_ENABLED_KEY).await?;
        Ok(result.is_some_and(|s| s.value == "true"))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{AppSettingService, AppSettingServiceImpl};
    use crate::{
        Error, RepositoryError,
        app_setting::{AppSetting, repository::MockAppSettingRepository},
    };

    fn fake_setting(key: &str, value: &str) -> AppSetting {
        AppSetting {
            key: key.to_owned(),
            value: value.to_owned(),
        }
    }

    fn create_service(repo: MockAppSettingRepository) -> AppSettingServiceImpl {
        let repository_service = Arc::new(
            crate::repository::testing::default_repository_service_builder()
                .app_setting_repository(Arc::new(repo))
                .build()
                .expect("all fields provided"),
        );
        AppSettingServiceImpl::new(repository_service)
    }

    // ─── mobi_enabled ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn mobi_enabled_defaults_false_when_not_set() {
        let mut repo = MockAppSettingRepository::new();
        repo.expect_get().returning(|_, _| Box::pin(async { Ok(None) }));
        let svc = create_service(repo);

        let result = svc.mobi_enabled().await;

        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn mobi_enabled_returns_true_when_set() {
        let setting = fake_setting("enrichment.mobi_enabled", "true");
        let mut repo = MockAppSettingRepository::new();
        repo.expect_get().returning(move |_, _| {
            let setting = setting.clone();
            Box::pin(async move { Ok(Some(setting)) })
        });
        let svc = create_service(repo);

        let result = svc.mobi_enabled().await;

        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[tokio::test]
    async fn mobi_enabled_returns_false_for_unknown_value() {
        let setting = fake_setting("enrichment.mobi_enabled", "yes");
        let mut repo = MockAppSettingRepository::new();
        repo.expect_get().returning(move |_, _| {
            let setting = setting.clone();
            Box::pin(async move { Ok(Some(setting)) })
        });
        let svc = create_service(repo);

        let result = svc.mobi_enabled().await;

        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    // ─── get ─────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn get_returns_none_when_not_found() {
        let mut repo = MockAppSettingRepository::new();
        repo.expect_get().returning(|_, _| Box::pin(async { Ok(None) }));
        let svc = create_service(repo);

        let result = svc.get("some-key").await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn get_propagates_error() {
        let mut repo = MockAppSettingRepository::new();
        repo.expect_get()
            .returning(|_, _| Box::pin(async { Err(Error::RepositoryError(RepositoryError::Database("db error".into()))) }));
        let svc = create_service(repo);

        let result = svc.get("some-key").await;

        assert!(matches!(result, Err(Error::RepositoryError(RepositoryError::Database(_)))));
    }

    // ─── set ─────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn set_returns_setting_on_success() {
        let expected = fake_setting("some-key", "some-value");
        let mut repo = MockAppSettingRepository::new();
        repo.expect_set().returning(move |_, _| {
            let expected = expected.clone();
            Box::pin(async move { Ok(expected) })
        });
        let svc = create_service(repo);

        let result = svc.set("some-key", "some-value").await;

        assert!(result.is_ok());
        let setting = result.unwrap();
        assert_eq!(setting.key, "some-key");
        assert_eq!(setting.value, "some-value");
    }
}
