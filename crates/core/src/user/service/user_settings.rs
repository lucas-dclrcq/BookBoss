use std::sync::Arc;

use crate::{
    Error,
    repository::RepositoryService,
    user::{NewUserSetting, UserId, UserSetting},
    with_read_only_transaction, with_transaction,
};

#[async_trait::async_trait]
pub trait UserSettingService: Send + Sync {
    async fn get(&self, user_id: UserId, key: &str) -> Result<Option<UserSetting>, Error>;
    async fn set(&self, user_id: UserId, key: &str, value: &str) -> Result<UserSetting, Error>;
    async fn delete(&self, user_id: UserId, key: &str) -> Result<(), Error>;
    async fn list_by_user(&self, user_id: UserId) -> Result<Vec<UserSetting>, Error>;
}

pub(crate) struct UserSettingServiceImpl {
    repository_service: Arc<RepositoryService>,
}

impl UserSettingServiceImpl {
    pub(crate) fn new(repository_service: Arc<RepositoryService>) -> Self {
        Self { repository_service }
    }
}

#[async_trait::async_trait]
impl UserSettingService for UserSettingServiceImpl {
    #[tracing::instrument(level = "trace", skip(self))]
    async fn get(&self, user_id: UserId, key: &str) -> Result<Option<UserSetting>, Error> {
        let key = key.to_owned();
        with_read_only_transaction!(self, user_setting_repository, |tx| user_setting_repository.get(tx, user_id, &key).await)
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn set(&self, user_id: UserId, key: &str, value: &str) -> Result<UserSetting, Error> {
        let setting = NewUserSetting {
            user_id,
            key: key.to_owned(),
            value: value.to_owned(),
        };
        with_transaction!(self, user_setting_repository, |tx| user_setting_repository.set(tx, setting).await)
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn delete(&self, user_id: UserId, key: &str) -> Result<(), Error> {
        let key = key.to_owned();
        with_transaction!(self, user_setting_repository, |tx| user_setting_repository.delete(tx, user_id, &key).await)
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn list_by_user(&self, user_id: UserId) -> Result<Vec<UserSetting>, Error> {
        with_read_only_transaction!(self, user_setting_repository, |tx| user_setting_repository.list_by_user(tx, user_id).await)
    }
}

#[cfg(test)]
mod tests {
    use std::{any::Any, sync::Arc};

    use super::{UserSettingService, UserSettingServiceImpl};
    use crate::{
        Error, RepositoryError,
        auth::repository::MockSessionRepository,
        book::repository::{
            author::MockAuthorRepository, book::MockBookRepository, genre::MockGenreRepository, publisher::MockPublisherRepository,
            series::MockSeriesRepository, tag::MockTagRepository,
        },
        device::repository::device::MockDeviceRepository,
        import::repository::import_job::MockImportJobRepository,
        jobs::repository::MockJobRepository,
        reading::repository::user_book_metadata::MockUserBookMetadataRepository,
        repository::{MockRepository, RepositoryServiceBuilder, Transaction},
        shelf::repository::shelf::MockShelfRepository,
        user::{
            UserId, UserSetting,
            repository::{user::MockUserRepository, user_settings::MockUserSettingRepository},
        },
    };

    // ─── Mock Transaction ────────────────────────────────────────────────────

    struct MockTransaction;

    #[async_trait::async_trait]
    impl Transaction for MockTransaction {
        fn as_any(&self) -> &dyn Any {
            self
        }

        async fn commit(self: Box<Self>) -> Result<(), Error> {
            Ok(())
        }

        async fn rollback(self: Box<Self>) -> Result<(), Error> {
            Ok(())
        }
    }

    // ─── Helper ──────────────────────────────────────────────────────────────

    fn make_mock_repo() -> MockRepository {
        let mut r = MockRepository::new();
        r.expect_begin()
            .returning(|| Box::pin(async { Ok(Box::new(MockTransaction) as Box<dyn Transaction>) }));
        r.expect_begin_read_only()
            .returning(|| Box::pin(async { Ok(Box::new(MockTransaction) as Box<dyn Transaction>) }));
        r
    }

    fn fake_setting(user_id: UserId, key: &str, value: &str) -> UserSetting {
        UserSetting {
            user_id,
            key: key.to_owned(),
            value: value.to_owned(),
        }
    }

    fn create_service(setting_repo: MockUserSettingRepository) -> UserSettingServiceImpl {
        let repository_service = Arc::new(
            RepositoryServiceBuilder::default()
                .repository(Arc::new(make_mock_repo()))
                .session_repository(Arc::new(MockSessionRepository::new()))
                .user_repository(Arc::new(MockUserRepository::new()))
                .user_setting_repository(Arc::new(setting_repo))
                .author_repository(Arc::new(MockAuthorRepository::new()))
                .series_repository(Arc::new(MockSeriesRepository::new()))
                .publisher_repository(Arc::new(MockPublisherRepository::new()))
                .genre_repository(Arc::new(MockGenreRepository::new()))
                .tag_repository(Arc::new(MockTagRepository::new()))
                .book_repository(Arc::new(MockBookRepository::new()))
                .import_job_repository(Arc::new(MockImportJobRepository::new()))
                .job_repository(Arc::new(MockJobRepository::new()))
                .shelf_repository(Arc::new(MockShelfRepository::new()))
                .user_book_metadata_repository(Arc::new(MockUserBookMetadataRepository::new()))
                .device_repository(Arc::new(MockDeviceRepository::new()))
                .build()
                .expect("all fields provided"),
        );
        UserSettingServiceImpl::new(repository_service)
    }

    // ─── get ─────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_get_returns_none_when_not_found() {
        let mut setting_repo = MockUserSettingRepository::new();
        setting_repo.expect_get().returning(|_, _, _| Box::pin(async { Ok(None) }));
        let svc = create_service(setting_repo);

        let result = svc.get(1, "some-key").await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_get_returns_setting_when_found() {
        let expected = fake_setting(1, "theme", "dark");
        let mut setting_repo = MockUserSettingRepository::new();
        setting_repo.expect_get().returning(move |_, _, _| {
            let expected = expected.clone();
            Box::pin(async move { Ok(Some(expected)) })
        });
        let svc = create_service(setting_repo);

        let result = svc.get(1, "theme").await;

        assert!(result.is_ok());
        let setting = result.unwrap().unwrap();
        assert_eq!(setting.user_id, 1);
        assert_eq!(setting.key, "theme");
        assert_eq!(setting.value, "dark");
    }

    #[tokio::test]
    async fn test_get_propagates_error() {
        let mut setting_repo = MockUserSettingRepository::new();
        setting_repo
            .expect_get()
            .returning(|_, _, _| Box::pin(async { Err(Error::RepositoryError(RepositoryError::Database("db error".into()))) }));
        let svc = create_service(setting_repo);

        let result = svc.get(1, "theme").await;

        assert!(matches!(result, Err(Error::RepositoryError(RepositoryError::Database(_)))));
    }

    // ─── set ─────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_set_returns_setting_on_success() {
        let expected = fake_setting(1, "theme", "dark");
        let mut setting_repo = MockUserSettingRepository::new();
        setting_repo.expect_set().returning(move |_, _| {
            let expected = expected.clone();
            Box::pin(async move { Ok(expected) })
        });
        let svc = create_service(setting_repo);

        let result = svc.set(1, "theme", "dark").await;

        assert!(result.is_ok());
        let setting = result.unwrap();
        assert_eq!(setting.user_id, 1);
        assert_eq!(setting.key, "theme");
        assert_eq!(setting.value, "dark");
    }

    #[tokio::test]
    async fn test_set_propagates_error() {
        let mut setting_repo = MockUserSettingRepository::new();
        setting_repo
            .expect_set()
            .returning(|_, _| Box::pin(async { Err(Error::RepositoryError(RepositoryError::Database("db error".into()))) }));
        let svc = create_service(setting_repo);

        let result = svc.set(1, "theme", "dark").await;

        assert!(matches!(result, Err(Error::RepositoryError(RepositoryError::Database(_)))));
    }

    // ─── delete ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_delete_returns_ok_on_success() {
        let mut setting_repo = MockUserSettingRepository::new();
        setting_repo.expect_delete().returning(|_, _, _| Box::pin(async { Ok(()) }));
        let svc = create_service(setting_repo);

        let result = svc.delete(1, "theme").await;

        result.unwrap();
    }

    #[tokio::test]
    async fn test_delete_propagates_error() {
        let mut setting_repo = MockUserSettingRepository::new();
        setting_repo
            .expect_delete()
            .returning(|_, _, _| Box::pin(async { Err(Error::RepositoryError(RepositoryError::Database("db error".into()))) }));
        let svc = create_service(setting_repo);

        let result = svc.delete(1, "theme").await;

        assert!(matches!(result, Err(Error::RepositoryError(RepositoryError::Database(_)))));
    }

    // ─── list_by_user ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_list_by_user_returns_empty() {
        let mut setting_repo = MockUserSettingRepository::new();
        setting_repo.expect_list_by_user().returning(|_, _| Box::pin(async { Ok(vec![]) }));
        let svc = create_service(setting_repo);

        let result = svc.list_by_user(1).await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_list_by_user_returns_multiple() {
        let settings = vec![fake_setting(1, "theme", "dark"), fake_setting(1, "lang", "en")];
        let mut setting_repo = MockUserSettingRepository::new();
        setting_repo.expect_list_by_user().returning(move |_, _| {
            let settings = settings.clone();
            Box::pin(async move { Ok(settings) })
        });
        let svc = create_service(setting_repo);

        let result = svc.list_by_user(1).await;

        assert!(result.is_ok());
        let list = result.unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].key, "theme");
        assert_eq!(list[1].key, "lang");
    }
}
