use std::sync::Arc;

use crate::{
    Error,
    app_setting::{AppSetting, NewAppSetting, OidcProvisioningDefaults},
    repository::RepositoryService,
    with_read_only_transaction, with_transaction,
};

const MOBI_ENABLED_KEY: &str = "enrichment.mobi_enabled";
const OIDC_PROVISIONING_CAPABILITIES_KEY: &str = "oidc.provisioning.capabilities";
const OIDC_PROVISIONING_LIBRARY_TOKENS_KEY: &str = "oidc.provisioning.library_tokens";
const OIDC_PROVISIONING_DEFAULT_LIBRARY_KEY: &str = "oidc.provisioning.default_library";

#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
#[async_trait::async_trait]
pub trait AppSettingService: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<AppSetting>, Error>;
    async fn set(&self, key: &str, value: &str) -> Result<AppSetting, Error>;
    async fn mobi_enabled(&self) -> Result<bool, Error>;
    async fn oidc_provisioning_defaults(&self) -> Result<OidcProvisioningDefaults, Error>;
    async fn set_oidc_provisioning_defaults(&self, defaults: &OidcProvisioningDefaults) -> Result<(), Error>;
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

    async fn oidc_provisioning_defaults(&self) -> Result<OidcProvisioningDefaults, Error> {
        let capabilities = match self.get(OIDC_PROVISIONING_CAPABILITIES_KEY).await? {
            Some(s) => serde_json::from_str(&s.value).map_err(|e| Error::Infrastructure(e.to_string()))?,
            None => crate::types::Capabilities::default(),
        };
        let library_tokens: Vec<String> = match self.get(OIDC_PROVISIONING_LIBRARY_TOKENS_KEY).await? {
            Some(s) => serde_json::from_str(&s.value).map_err(|e| Error::Infrastructure(e.to_string()))?,
            None => Vec::new(),
        };
        let default_library = self
            .get(OIDC_PROVISIONING_DEFAULT_LIBRARY_KEY)
            .await?
            .map(|s| s.value)
            .filter(|v| !v.is_empty());

        Ok(OidcProvisioningDefaults {
            capabilities,
            library_tokens,
            default_library,
        })
    }

    async fn set_oidc_provisioning_defaults(&self, defaults: &OidcProvisioningDefaults) -> Result<(), Error> {
        let capabilities = serde_json::to_string(&defaults.capabilities).map_err(|e| Error::Infrastructure(e.to_string()))?;
        let library_tokens = serde_json::to_string(&defaults.library_tokens).map_err(|e| Error::Infrastructure(e.to_string()))?;

        self.set(OIDC_PROVISIONING_CAPABILITIES_KEY, &capabilities).await?;
        self.set(OIDC_PROVISIONING_LIBRARY_TOKENS_KEY, &library_tokens).await?;
        self.set(OIDC_PROVISIONING_DEFAULT_LIBRARY_KEY, defaults.default_library.as_deref().unwrap_or(""))
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{AppSettingService, AppSettingServiceImpl};
    use crate::{
        Error, RepositoryError,
        app_setting::{AppSetting, OidcProvisioningDefaults, repository::MockAppSettingRepository},
        types::Capability,
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

    // ─── oidc_provisioning_defaults ──────────────────────────────────────────

    #[tokio::test]
    async fn oidc_provisioning_defaults_empty_when_unset() {
        let mut repo = MockAppSettingRepository::new();
        repo.expect_get().returning(|_, _| Box::pin(async { Ok(None) }));
        let svc = create_service(repo);

        let result = svc.oidc_provisioning_defaults().await.unwrap();

        assert!(result.capabilities.is_empty());
        assert!(result.library_tokens.is_empty());
        assert!(result.default_library.is_none());
    }

    #[tokio::test]
    async fn oidc_provisioning_defaults_parses_stored_values() {
        let mut repo = MockAppSettingRepository::new();
        repo.expect_get().returning(|_, key| {
            let key = key.to_owned();
            Box::pin(async move {
                let value = match key.as_str() {
                    "oidc.provisioning.capabilities" => Some(r#"["EditBook","Admin"]"#),
                    "oidc.provisioning.library_tokens" => Some(r#"["LB_AAAAAAAAAAAA1","LB_BBBBBBBBBBBB2"]"#),
                    "oidc.provisioning.default_library" => Some("LB_AAAAAAAAAAAA1"),
                    _ => None,
                };
                Ok(value.map(|v| AppSetting { key, value: v.to_owned() }))
            })
        });
        let svc = create_service(repo);

        let result = svc.oidc_provisioning_defaults().await.unwrap();

        assert!(result.capabilities.contains(&Capability::EditBook));
        assert!(result.capabilities.contains(&Capability::Admin));
        assert_eq!(result.library_tokens, vec!["LB_AAAAAAAAAAAA1", "LB_BBBBBBBBBBBB2"]);
        assert_eq!(result.default_library.as_deref(), Some("LB_AAAAAAAAAAAA1"));
    }

    #[tokio::test]
    async fn oidc_provisioning_defaults_treats_empty_default_as_none() {
        let mut repo = MockAppSettingRepository::new();
        repo.expect_get().returning(|_, key| {
            let key = key.to_owned();
            Box::pin(async move {
                let value = if key == "oidc.provisioning.default_library" { Some("") } else { None };
                Ok(value.map(|v: &str| AppSetting { key, value: v.to_owned() }))
            })
        });
        let svc = create_service(repo);

        let result = svc.oidc_provisioning_defaults().await.unwrap();

        assert!(result.default_library.is_none());
    }

    #[tokio::test]
    async fn oidc_provisioning_defaults_propagates_error() {
        let mut repo = MockAppSettingRepository::new();
        repo.expect_get()
            .returning(|_, _| Box::pin(async { Err(Error::RepositoryError(RepositoryError::Database("db error".into()))) }));
        let svc = create_service(repo);

        let result = svc.oidc_provisioning_defaults().await;

        assert!(matches!(result, Err(Error::RepositoryError(RepositoryError::Database(_)))));
    }

    // ─── set_oidc_provisioning_defaults ──────────────────────────────────────

    #[tokio::test]
    async fn set_oidc_provisioning_defaults_writes_three_keys() {
        use std::collections::HashSet;

        let mut repo = MockAppSettingRepository::new();
        repo.expect_set().times(3).returning(|_, setting| {
            Box::pin(async move {
                Ok(AppSetting {
                    key: setting.key,
                    value: setting.value,
                })
            })
        });
        let svc = create_service(repo);

        let defaults = OidcProvisioningDefaults {
            capabilities: HashSet::from([Capability::EditBook]),
            library_tokens: vec!["LB_AAAAAAAAAAAA1".to_string()],
            default_library: Some("LB_AAAAAAAAAAAA1".to_string()),
        };

        let result = svc.set_oidc_provisioning_defaults(&defaults).await;

        assert!(result.is_ok());
    }
}
