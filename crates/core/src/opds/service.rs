use std::sync::Arc;

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use rand::RngExt;

use crate::{Error, repository::RepositoryService, types::Capability, user::User, with_read_only_transaction, with_transaction};

const OPDS_PASSWORD_KEY: &str = "opds_password_hash";
const OPDS_PASSWORD_LENGTH: usize = 12;
const OPDS_PASSWORD_CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789";

#[async_trait::async_trait]
pub trait OpdsService: Send + Sync {
    /// Returns the plaintext OPDS password for a user, generating one if none
    /// exists. Returns `Some(plaintext)` when a new password was created, or
    /// `None` when a password already existed (the plaintext cannot be
    /// recovered from the stored hash).
    async fn get_or_create_password(&self, user: &User) -> Result<Option<String>, Error>;

    /// Generates a new OPDS password, replacing any existing one.
    /// Returns the new plaintext password.
    async fn regenerate_password(&self, user: &User) -> Result<String, Error>;

    /// Verifies a plaintext password against the stored OPDS password hash for
    /// a user.
    async fn verify_password(&self, user: &User, password: &str) -> Result<bool, Error>;

    /// Returns true if the user has a stored OPDS password.
    async fn has_password(&self, user: &User) -> Result<bool, Error>;
}

pub(crate) struct OpdsServiceImpl {
    repository_service: Arc<RepositoryService>,
}

impl OpdsServiceImpl {
    pub(crate) fn new(repository_service: Arc<RepositoryService>) -> Self {
        Self { repository_service }
    }
}

fn generate_opds_password() -> String {
    let mut rng = rand::rng();
    (0..OPDS_PASSWORD_LENGTH)
        .map(|_| {
            let idx = rng.random_range(0..OPDS_PASSWORD_CHARSET.len());
            OPDS_PASSWORD_CHARSET[idx] as char
        })
        .collect()
}

fn hash_opds_password(password: &str) -> Result<String, Error> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| Error::CryptoError(e.to_string()))?;
    Ok(hash.to_string())
}

fn verify_opds_password(password: &str, hash: &str) -> bool {
    let Ok(parsed_hash) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default().verify_password(password.as_bytes(), &parsed_hash).is_ok()
}

#[async_trait::async_trait]
impl OpdsService for OpdsServiceImpl {
    async fn get_or_create_password(&self, user: &User) -> Result<Option<String>, Error> {
        if !user.has_capability(Capability::OpdsAccess) {
            return Err(Error::Validation("User does not have OPDS access".to_string()));
        }

        let user_id = user.id;
        let existing = with_read_only_transaction!(self, user_setting_repository, |tx| user_setting_repository
            .get(tx, user_id, OPDS_PASSWORD_KEY)
            .await)?;

        if existing.is_some() {
            return Ok(None);
        }

        let plaintext = generate_opds_password();
        let hash = hash_opds_password(&plaintext)?;

        let setting = crate::user::NewUserSetting {
            user_id,
            key: OPDS_PASSWORD_KEY.to_owned(),
            value: hash,
        };
        with_transaction!(self, user_setting_repository, |tx| user_setting_repository.set(tx, setting).await)?;

        Ok(Some(plaintext))
    }

    async fn regenerate_password(&self, user: &User) -> Result<String, Error> {
        if !user.has_capability(Capability::OpdsAccess) {
            return Err(Error::Validation("User does not have OPDS access".to_string()));
        }

        let plaintext = generate_opds_password();
        let hash = hash_opds_password(&plaintext)?;

        let setting = crate::user::NewUserSetting {
            user_id: user.id,
            key: OPDS_PASSWORD_KEY.to_owned(),
            value: hash,
        };
        with_transaction!(self, user_setting_repository, |tx| user_setting_repository.set(tx, setting).await)?;

        Ok(plaintext)
    }

    async fn verify_password(&self, user: &User, password: &str) -> Result<bool, Error> {
        if !user.has_capability(Capability::OpdsAccess) {
            return Ok(false);
        }

        let user_id = user.id;
        let setting = with_read_only_transaction!(self, user_setting_repository, |tx| user_setting_repository
            .get(tx, user_id, OPDS_PASSWORD_KEY)
            .await)?;

        match setting {
            Some(s) => Ok(verify_opds_password(password, &s.value)),
            None => Ok(false),
        }
    }

    async fn has_password(&self, user: &User) -> Result<bool, Error> {
        let user_id = user.id;
        let setting = with_read_only_transaction!(self, user_setting_repository, |tx| user_setting_repository
            .get(tx, user_id, OPDS_PASSWORD_KEY)
            .await)?;
        Ok(setting.is_some())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        any::Any,
        collections::HashSet,
        sync::Mutex,
    };

    use chrono::Utc;

    use super::*;
    use crate::{
        auth::{NewSession, Session, repository::SessionRepository},
        book::{
            Author, AuthorId, AuthorRepository, AuthorRole, AuthorToken, Book, BookAuthor, BookFile, BookId, BookIdentifier, BookQuery, BookRepository,
            BookToken, FileFormat, FileRole, Genre, GenreId, GenreRepository, GenreToken, IdentifierType, NewAuthor, NewBook, NewGenre, NewPublisher,
            NewSeries, NewTag, Publisher, PublisherId, PublisherRepository, PublisherToken, Series, SeriesId, SeriesRepository, SeriesToken, Tag, TagId,
            TagRepository, TagToken,
        },
        device::{Device, DeviceBook, DeviceId, DeviceRepository, DeviceSyncLog, DeviceToken, NewDevice, NewDeviceSyncLog},
        filter::BookFilter,
        import::{ImportJob, ImportJobId, ImportJobRepository, ImportJobToken, ImportStatus, NewImportJob},
        jobs::{Job, JobRepository},
        reading::{ReadStatus, UserBookMetadata, UserBookMetadataRepository},
        repository::{Repository, RepositoryServiceBuilder, Transaction},
        shelf::{BookShelf, NewShelf, Shelf, ShelfId, ShelfRepository, ShelfToken},
        types::EmailAddress,
        user::{
            NewUser, NewUserSetting, UserId, UserSetting, UserToken,
            repository::{UserRepository, UserSettingRepository},
        },
    };

    // ── Unit tests ──────────────────────────────────────────────────────────

    #[test]
    fn test_generate_opds_password_length() {
        let password = generate_opds_password();
        assert_eq!(password.len(), OPDS_PASSWORD_LENGTH);
    }

    #[test]
    fn test_generate_opds_password_is_alphanumeric() {
        let password = generate_opds_password();
        assert!(password.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn test_generate_opds_password_uniqueness() {
        let p1 = generate_opds_password();
        let p2 = generate_opds_password();
        assert_ne!(p1, p2);
    }

    #[test]
    fn test_hash_and_verify_round_trip() {
        let password = "testpassword";
        let hash = hash_opds_password(password).unwrap();
        assert!(verify_opds_password(password, &hash));
    }

    #[test]
    fn test_verify_wrong_password() {
        let hash = hash_opds_password("correct").unwrap();
        assert!(!verify_opds_password("wrong", &hash));
    }

    #[test]
    fn test_verify_invalid_hash() {
        assert!(!verify_opds_password("password", "not-a-valid-hash"));
    }

    // ── Mock infrastructure ─────────────────────────────────────────────────

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

    struct MockRepository;

    #[async_trait::async_trait]
    impl Repository for MockRepository {
        async fn begin(&self) -> Result<Box<dyn Transaction>, Error> {
            Ok(Box::new(MockTransaction))
        }
        async fn begin_read_only(&self) -> Result<Box<dyn Transaction>, Error> {
            Ok(Box::new(MockTransaction))
        }
        async fn close(&self) -> Result<(), Error> {
            Ok(())
        }
    }

    struct MockSessionRepository;
    #[async_trait::async_trait]
    impl SessionRepository for MockSessionRepository {
        async fn count(&self, _: &dyn Transaction) -> Result<i64, Error> {
            unimplemented!()
        }
        async fn store(&self, _: &dyn Transaction, _: NewSession) -> Result<Session, Error> {
            unimplemented!()
        }
        async fn load(&self, _: &dyn Transaction, _: &str) -> Result<Option<Session>, Error> {
            unimplemented!()
        }
        async fn delete_by_id(&self, _: &dyn Transaction, _: &str) -> Result<(), Error> {
            unimplemented!()
        }
        async fn exists(&self, _: &dyn Transaction, _: &str) -> Result<bool, Error> {
            unimplemented!()
        }
        async fn delete_by_expiry(&self, _: &dyn Transaction) -> Result<Vec<String>, Error> {
            unimplemented!()
        }
        async fn delete_all(&self, _: &dyn Transaction) -> Result<(), Error> {
            unimplemented!()
        }
        async fn get_ids(&self, _: &dyn Transaction) -> Result<Vec<String>, Error> {
            unimplemented!()
        }
    }

    struct MockUserRepository;
    #[async_trait::async_trait]
    impl UserRepository for MockUserRepository {
        async fn add_user(&self, _: &dyn Transaction, _: NewUser) -> Result<User, Error> {
            unimplemented!()
        }
        async fn update_user(&self, _: &dyn Transaction, _: User) -> Result<User, Error> {
            unimplemented!()
        }
        async fn delete_user(&self, _: &dyn Transaction, _: User) -> Result<User, Error> {
            unimplemented!()
        }
        async fn list_users(&self, _: &dyn Transaction, _: Option<UserId>, _: Option<u64>) -> Result<Vec<User>, Error> {
            unimplemented!()
        }
        async fn find_by_id(&self, _: &dyn Transaction, _: UserId) -> Result<Option<User>, Error> {
            unimplemented!()
        }
        async fn find_by_username(&self, _: &dyn Transaction, _: &str) -> Result<Option<User>, Error> {
            unimplemented!()
        }
    }

    #[derive(Default)]
    struct MockUserSettingRepository {
        get_result: Mutex<Option<Result<Option<UserSetting>, Error>>>,
        set_result: Mutex<Option<Result<UserSetting, Error>>>,
    }

    impl MockUserSettingRepository {
        fn with_get_result(self, result: Result<Option<UserSetting>, Error>) -> Self {
            *self.get_result.lock().unwrap() = Some(result);
            self
        }

        fn with_set_result(self, result: Result<UserSetting, Error>) -> Self {
            *self.set_result.lock().unwrap() = Some(result);
            self
        }
    }

    #[async_trait::async_trait]
    impl UserSettingRepository for MockUserSettingRepository {
        async fn get(&self, _: &dyn Transaction, _: UserId, _: &str) -> Result<Option<UserSetting>, Error> {
            self.get_result.lock().unwrap().clone().unwrap_or(Ok(None))
        }
        async fn set(&self, _: &dyn Transaction, _: NewUserSetting) -> Result<UserSetting, Error> {
            self.set_result.lock().unwrap().clone().unwrap_or_else(|| {
                Ok(UserSetting {
                    user_id: 1,
                    key: OPDS_PASSWORD_KEY.to_owned(),
                    value: String::new(),
                })
            })
        }
        async fn delete(&self, _: &dyn Transaction, _: UserId, _: &str) -> Result<(), Error> {
            unimplemented!()
        }
        async fn list_by_user(&self, _: &dyn Transaction, _: UserId) -> Result<Vec<UserSetting>, Error> {
            unimplemented!()
        }
    }

    struct MockAuthorRepository;
    #[async_trait::async_trait]
    impl AuthorRepository for MockAuthorRepository {
        async fn add_author(&self, _: &dyn Transaction, _: NewAuthor) -> Result<Author, Error> {
            unimplemented!()
        }
        async fn update_author(&self, _: &dyn Transaction, _: Author) -> Result<Author, Error> {
            unimplemented!()
        }
        async fn find_by_id(&self, _: &dyn Transaction, _: AuthorId) -> Result<Option<Author>, Error> {
            unimplemented!()
        }
        async fn find_by_token(&self, _: &dyn Transaction, _: &AuthorToken) -> Result<Option<Author>, Error> {
            unimplemented!()
        }
        async fn list_authors(&self, _: &dyn Transaction, _: Option<AuthorId>, _: Option<u64>) -> Result<Vec<Author>, Error> {
            unimplemented!()
        }
        async fn find_by_name(&self, _: &dyn Transaction, _: &str) -> Result<Option<Author>, Error> {
            unimplemented!()
        }
        async fn list_all_authors(&self, _: &dyn Transaction) -> Result<Vec<Author>, Error> {
            unimplemented!()
        }
        async fn count_authors(&self, _: &dyn Transaction) -> Result<u64, Error> {
            unimplemented!()
        }
        async fn delete_author(&self, _: &dyn Transaction, _: AuthorId) -> Result<(), Error> {
            unimplemented!()
        }
    }

    struct MockSeriesRepository;
    #[async_trait::async_trait]
    impl SeriesRepository for MockSeriesRepository {
        async fn add_series(&self, _: &dyn Transaction, _: NewSeries) -> Result<Series, Error> {
            unimplemented!()
        }
        async fn update_series(&self, _: &dyn Transaction, _: Series) -> Result<Series, Error> {
            unimplemented!()
        }
        async fn find_by_id(&self, _: &dyn Transaction, _: SeriesId) -> Result<Option<Series>, Error> {
            unimplemented!()
        }
        async fn find_by_token(&self, _: &dyn Transaction, _: &SeriesToken) -> Result<Option<Series>, Error> {
            unimplemented!()
        }
        async fn list_series(&self, _: &dyn Transaction, _: Option<SeriesId>, _: Option<u64>) -> Result<Vec<Series>, Error> {
            unimplemented!()
        }
        async fn find_by_name(&self, _: &dyn Transaction, _: &str) -> Result<Option<Series>, Error> {
            unimplemented!()
        }
        async fn list_all_series(&self, _: &dyn Transaction) -> Result<Vec<Series>, Error> {
            unimplemented!()
        }
        async fn max_series_number_for_series(&self, _: &dyn Transaction, _: SeriesId) -> Result<Option<rust_decimal::Decimal>, Error> {
            unimplemented!()
        }
    }

    struct MockPublisherRepository;
    #[async_trait::async_trait]
    impl PublisherRepository for MockPublisherRepository {
        async fn add_publisher(&self, _: &dyn Transaction, _: NewPublisher) -> Result<Publisher, Error> {
            unimplemented!()
        }
        async fn update_publisher(&self, _: &dyn Transaction, _: Publisher) -> Result<Publisher, Error> {
            unimplemented!()
        }
        async fn find_by_id(&self, _: &dyn Transaction, _: PublisherId) -> Result<Option<Publisher>, Error> {
            unimplemented!()
        }
        async fn find_by_token(&self, _: &dyn Transaction, _: &PublisherToken) -> Result<Option<Publisher>, Error> {
            unimplemented!()
        }
        async fn list_publishers(&self, _: &dyn Transaction, _: Option<PublisherId>, _: Option<u64>) -> Result<Vec<Publisher>, Error> {
            unimplemented!()
        }
        async fn list_all_publishers(&self, _: &dyn Transaction) -> Result<Vec<Publisher>, Error> {
            unimplemented!()
        }
        async fn find_by_name(&self, _: &dyn Transaction, _: &str) -> Result<Option<Publisher>, Error> {
            unimplemented!()
        }
    }

    struct MockGenreRepository;
    #[async_trait::async_trait]
    impl GenreRepository for MockGenreRepository {
        async fn add_genre(&self, _: &dyn Transaction, _: NewGenre) -> Result<Genre, Error> {
            unimplemented!()
        }
        async fn update_genre(&self, _: &dyn Transaction, _: Genre) -> Result<Genre, Error> {
            unimplemented!()
        }
        async fn find_by_id(&self, _: &dyn Transaction, _: GenreId) -> Result<Option<Genre>, Error> {
            unimplemented!()
        }
        async fn find_by_token(&self, _: &dyn Transaction, _: &GenreToken) -> Result<Option<Genre>, Error> {
            unimplemented!()
        }
        async fn find_by_name(&self, _: &dyn Transaction, _: &str) -> Result<Option<Genre>, Error> {
            unimplemented!()
        }
        async fn list_genres(&self, _: &dyn Transaction, _: Option<GenreId>, _: Option<u64>) -> Result<Vec<Genre>, Error> {
            unimplemented!()
        }
        async fn list_all_genres(&self, _: &dyn Transaction) -> Result<Vec<Genre>, Error> {
            unimplemented!()
        }
    }

    struct MockTagRepository;
    #[async_trait::async_trait]
    impl TagRepository for MockTagRepository {
        async fn add_tag(&self, _: &dyn Transaction, _: NewTag) -> Result<Tag, Error> {
            unimplemented!()
        }
        async fn update_tag(&self, _: &dyn Transaction, _: Tag) -> Result<Tag, Error> {
            unimplemented!()
        }
        async fn find_by_id(&self, _: &dyn Transaction, _: TagId) -> Result<Option<Tag>, Error> {
            unimplemented!()
        }
        async fn find_by_token(&self, _: &dyn Transaction, _: &TagToken) -> Result<Option<Tag>, Error> {
            unimplemented!()
        }
        async fn find_by_name(&self, _: &dyn Transaction, _: &str) -> Result<Option<Tag>, Error> {
            unimplemented!()
        }
        async fn list_tags(&self, _: &dyn Transaction, _: Option<TagId>, _: Option<u64>) -> Result<Vec<Tag>, Error> {
            unimplemented!()
        }
        async fn list_all_tags(&self, _: &dyn Transaction) -> Result<Vec<Tag>, Error> {
            unimplemented!()
        }
    }

    struct MockBookRepository;
    #[async_trait::async_trait]
    impl BookRepository for MockBookRepository {
        async fn add_book(&self, _: &dyn Transaction, _: NewBook) -> Result<Book, Error> {
            unimplemented!()
        }
        async fn update_book(&self, _: &dyn Transaction, _: Book) -> Result<Book, Error> {
            unimplemented!()
        }
        async fn find_by_id(&self, _: &dyn Transaction, _: BookId) -> Result<Option<Book>, Error> {
            unimplemented!()
        }
        async fn find_by_token(&self, _: &dyn Transaction, _: &BookToken) -> Result<Option<Book>, Error> {
            unimplemented!()
        }
        async fn list_books(&self, _: &dyn Transaction, _: &BookQuery, _: Option<BookId>, _: Option<u64>) -> Result<Vec<Book>, Error> {
            unimplemented!()
        }
        async fn authors_for_book(&self, _: &dyn Transaction, _: BookId) -> Result<Vec<BookAuthor>, Error> {
            unimplemented!()
        }
        async fn files_for_book(&self, _: &dyn Transaction, _: BookId) -> Result<Vec<BookFile>, Error> {
            unimplemented!()
        }
        async fn identifiers_for_book(&self, _: &dyn Transaction, _: BookId) -> Result<Vec<BookIdentifier>, Error> {
            unimplemented!()
        }
        async fn find_file_by_hash(&self, _: &dyn Transaction, _: &str) -> Result<Option<BookFile>, Error> {
            unimplemented!()
        }
        async fn add_book_file(&self, _: &dyn Transaction, _: BookId, _: FileFormat, _: FileRole, _: String, _: i64, _: String) -> Result<BookFile, Error> {
            unimplemented!()
        }
        async fn add_book_author(&self, _: &dyn Transaction, _: BookId, _: AuthorId, _: AuthorRole, _: i32) -> Result<(), Error> {
            unimplemented!()
        }
        async fn add_book_identifier(&self, _: &dyn Transaction, _: BookId, _: IdentifierType, _: String) -> Result<(), Error> {
            unimplemented!()
        }
        async fn delete_book(&self, _: &dyn Transaction, _: BookId) -> Result<(), Error> {
            unimplemented!()
        }
        async fn delete_book_authors(&self, _: &dyn Transaction, _: BookId) -> Result<(), Error> {
            unimplemented!()
        }
        async fn delete_book_identifiers(&self, _: &dyn Transaction, _: BookId) -> Result<(), Error> {
            unimplemented!()
        }
        async fn count_available_books(&self, _: &dyn Transaction) -> Result<u64, Error> {
            unimplemented!()
        }
        async fn count_books_for_author(&self, _: &dyn Transaction, _: AuthorId) -> Result<u64, Error> {
            unimplemented!()
        }
        async fn genres_for_book(&self, _: &dyn Transaction, _: BookId) -> Result<Vec<Genre>, Error> {
            unimplemented!()
        }
        async fn tags_for_book(&self, _: &dyn Transaction, _: BookId) -> Result<Vec<Tag>, Error> {
            unimplemented!()
        }
        async fn add_book_genre(&self, _: &dyn Transaction, _: BookId, _: GenreId) -> Result<(), Error> {
            unimplemented!()
        }
        async fn add_book_tag(&self, _: &dyn Transaction, _: BookId, _: TagId) -> Result<(), Error> {
            unimplemented!()
        }
        async fn delete_book_genres(&self, _: &dyn Transaction, _: BookId) -> Result<(), Error> {
            unimplemented!()
        }
        async fn delete_book_tags(&self, _: &dyn Transaction, _: BookId) -> Result<(), Error> {
            unimplemented!()
        }
        async fn delete_book_file_by_role(&self, _: &dyn Transaction, _: BookId, _: FileFormat, _: FileRole) -> Result<(), Error> {
            unimplemented!()
        }
        async fn find_book_ids_needing_enrichment(&self, _: &dyn Transaction) -> Result<Vec<BookId>, Error> {
            unimplemented!()
        }
        async fn find_book_ids_needing_kepub_conversion(&self, _: &dyn Transaction) -> Result<Vec<BookId>, Error> {
            unimplemented!()
        }
        async fn update_enriched_paths(&self, _: &dyn Transaction, _: BookId, _: &str, _: &str) -> Result<(), Error> {
            unimplemented!()
        }
    }

    struct MockImportJobRepository;
    #[async_trait::async_trait]
    impl ImportJobRepository for MockImportJobRepository {
        async fn add_job(&self, _: &dyn Transaction, _: NewImportJob) -> Result<ImportJob, Error> {
            unimplemented!()
        }
        async fn update_job(&self, _: &dyn Transaction, _: ImportJob) -> Result<ImportJob, Error> {
            unimplemented!()
        }
        async fn find_by_id(&self, _: &dyn Transaction, _: ImportJobId) -> Result<Option<ImportJob>, Error> {
            unimplemented!()
        }
        async fn find_by_token(&self, _: &dyn Transaction, _: &ImportJobToken) -> Result<Option<ImportJob>, Error> {
            unimplemented!()
        }
        async fn find_by_hash(&self, _: &dyn Transaction, _: &str) -> Result<Option<ImportJob>, Error> {
            unimplemented!()
        }
        async fn list_by_status(&self, _: &dyn Transaction, _: ImportStatus, _: Option<ImportJobId>, _: Option<u64>) -> Result<Vec<ImportJob>, Error> {
            unimplemented!()
        }
        async fn reset_in_progress_to_pending(&self, _: &dyn Transaction) -> Result<u64, Error> {
            unimplemented!()
        }
        async fn find_by_candidate_book_id(&self, _: &dyn Transaction, _: BookId) -> Result<Option<ImportJob>, Error> {
            unimplemented!()
        }
        async fn delete_job(&self, _: &dyn Transaction, _: ImportJobId) -> Result<(), Error> {
            unimplemented!()
        }
        async fn approve_job(&self, _: &dyn Transaction, _: ImportJobId) -> Result<(), Error> {
            unimplemented!()
        }
    }

    struct MockJobRepository;
    #[async_trait::async_trait]
    impl JobRepository for MockJobRepository {
        async fn enqueue_raw(&self, _: &dyn Transaction, _: &str, _: serde_json::Value, _: i16) -> Result<Job, Error> {
            unimplemented!()
        }
        async fn claim_next(&self, _: &dyn Transaction) -> Result<Option<Job>, Error> {
            unimplemented!()
        }
        async fn complete(&self, _: &dyn Transaction, _: Job) -> Result<Job, Error> {
            unimplemented!()
        }
        async fn fail(&self, _: &dyn Transaction, _: Job, _: String) -> Result<Job, Error> {
            unimplemented!()
        }
        async fn reset_running_to_pending(&self, _: &dyn Transaction) -> Result<u64, Error> {
            unimplemented!()
        }
        async fn count_pending_by_type(&self, _: &dyn Transaction, _: &str) -> Result<u64, Error> {
            unimplemented!()
        }
    }

    struct MockShelfRepository;
    #[async_trait::async_trait]
    impl ShelfRepository for MockShelfRepository {
        async fn add_shelf(&self, _: &dyn Transaction, _: NewShelf) -> Result<Shelf, Error> {
            unimplemented!()
        }
        async fn update_shelf(&self, _: &dyn Transaction, _: Shelf) -> Result<Shelf, Error> {
            unimplemented!()
        }
        async fn delete_shelf(&self, _: &dyn Transaction, _: Shelf) -> Result<(), Error> {
            unimplemented!()
        }
        async fn find_by_id(&self, _: &dyn Transaction, _: ShelfId) -> Result<Option<Shelf>, Error> {
            unimplemented!()
        }
        async fn find_by_token(&self, _: &dyn Transaction, _: &ShelfToken) -> Result<Option<Shelf>, Error> {
            unimplemented!()
        }
        async fn list_for_user(&self, _: &dyn Transaction, _: UserId) -> Result<Vec<Shelf>, Error> {
            unimplemented!()
        }
        async fn list_public_shelves(&self, _: &dyn Transaction, _: UserId) -> Result<Vec<Shelf>, Error> {
            unimplemented!()
        }
        async fn add_book_to_shelf(&self, _: &dyn Transaction, _: BookShelf) -> Result<BookShelf, Error> {
            unimplemented!()
        }
        async fn remove_book_from_shelf(&self, _: &dyn Transaction, _: ShelfId, _: BookId) -> Result<(), Error> {
            unimplemented!()
        }
        async fn books_for_shelf(&self, _: &dyn Transaction, _: ShelfId, _: Option<BookId>, _: Option<u64>) -> Result<Vec<BookShelf>, Error> {
            unimplemented!()
        }
        async fn books_for_filter(&self, _: &dyn Transaction, _: &BookFilter, _: UserId, _: Option<BookId>, _: Option<u64>) -> Result<Vec<Book>, Error> {
            unimplemented!()
        }
        async fn count_for_filter(&self, _: &dyn Transaction, _: &BookFilter, _: UserId) -> Result<u64, Error> {
            unimplemented!()
        }
        async fn find_by_device_id(&self, _: &dyn Transaction, _: DeviceId) -> Result<Option<Shelf>, Error> {
            unimplemented!()
        }
    }

    struct MockUserBookMetadataRepository;
    #[async_trait::async_trait]
    impl UserBookMetadataRepository for MockUserBookMetadataRepository {
        async fn find_by_user_and_book(&self, _: &dyn Transaction, _: UserId, _: BookId) -> Result<Option<UserBookMetadata>, Error> {
            unimplemented!()
        }
        async fn upsert(&self, _: &dyn Transaction, _: UserBookMetadata) -> Result<UserBookMetadata, Error> {
            unimplemented!()
        }
        async fn list_for_user(
            &self,
            _: &dyn Transaction,
            _: UserId,
            _: Option<ReadStatus>,
            _: Option<BookId>,
            _: Option<u64>,
        ) -> Result<Vec<UserBookMetadata>, Error> {
            unimplemented!()
        }
        async fn list_for_user_and_books(&self, _: &dyn Transaction, _: UserId, _: &[BookId]) -> Result<Vec<UserBookMetadata>, Error> {
            unimplemented!()
        }
    }

    struct MockDeviceRepository;
    #[async_trait::async_trait]
    impl DeviceRepository for MockDeviceRepository {
        async fn add_device(&self, _: &dyn Transaction, _: NewDevice) -> Result<Device, Error> {
            unimplemented!()
        }
        async fn update_device(&self, _: &dyn Transaction, _: Device) -> Result<Device, Error> {
            unimplemented!()
        }
        async fn delete_device(&self, _: &dyn Transaction, _: Device) -> Result<(), Error> {
            unimplemented!()
        }
        async fn find_by_id(&self, _: &dyn Transaction, _: DeviceId) -> Result<Option<Device>, Error> {
            unimplemented!()
        }
        async fn find_by_token(&self, _: &dyn Transaction, _: &DeviceToken) -> Result<Option<Device>, Error> {
            unimplemented!()
        }
        async fn list_for_user(&self, _: &dyn Transaction, _: UserId) -> Result<Vec<Device>, Error> {
            unimplemented!()
        }
        async fn count_with_name_prefix(&self, _: &dyn Transaction, _: UserId, _: &str) -> Result<u64, Error> {
            unimplemented!()
        }
        async fn add_device_book(&self, _: &dyn Transaction, _: DeviceBook) -> Result<DeviceBook, Error> {
            unimplemented!()
        }
        async fn remove_device_book(&self, _: &dyn Transaction, _: DeviceId, _: BookId) -> Result<(), Error> {
            unimplemented!()
        }
        async fn clear_device_books(&self, _: &dyn Transaction, _: DeviceId) -> Result<(), Error> {
            unimplemented!()
        }
        async fn update_device_book(&self, _: &dyn Transaction, _: DeviceBook) -> Result<DeviceBook, Error> {
            unimplemented!()
        }
        async fn books_for_device(&self, _: &dyn Transaction, _: DeviceId) -> Result<Vec<DeviceBook>, Error> {
            unimplemented!()
        }
        async fn add_sync_log(&self, _: &dyn Transaction, _: NewDeviceSyncLog) -> Result<DeviceSyncLog, Error> {
            unimplemented!()
        }
        async fn list_sync_logs_for_device(&self, _: &dyn Transaction, _: DeviceId, _: Option<u64>) -> Result<Vec<DeviceSyncLog>, Error> {
            unimplemented!()
        }
    }

    // ── Helpers ─────────────────────────────────────────────────────────────

    fn user_with_opds_access() -> User {
        User {
            id: 1,
            version: 1,
            token: UserToken::new(1),
            username: "alice".to_string(),
            full_name: "Alice".to_string(),
            password_hash: String::new(),
            email_address: EmailAddress::new("alice@example.com").unwrap(),
            capabilities: HashSet::from([Capability::OpdsAccess]),
            change_password_on_login: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn user_without_opds_access() -> User {
        User {
            capabilities: HashSet::new(),
            ..user_with_opds_access()
        }
    }

    fn create_service(mock_settings: MockUserSettingRepository) -> OpdsServiceImpl {
        let repository_service = Arc::new(
            RepositoryServiceBuilder::default()
                .repository(Arc::new(MockRepository) as Arc<dyn Repository>)
                .session_repository(Arc::new(MockSessionRepository) as Arc<dyn SessionRepository>)
                .user_repository(Arc::new(MockUserRepository) as Arc<dyn UserRepository>)
                .user_setting_repository(Arc::new(mock_settings) as Arc<dyn UserSettingRepository>)
                .author_repository(Arc::new(MockAuthorRepository) as Arc<dyn AuthorRepository>)
                .series_repository(Arc::new(MockSeriesRepository) as Arc<dyn SeriesRepository>)
                .publisher_repository(Arc::new(MockPublisherRepository) as Arc<dyn PublisherRepository>)
                .genre_repository(Arc::new(MockGenreRepository) as Arc<dyn GenreRepository>)
                .tag_repository(Arc::new(MockTagRepository) as Arc<dyn TagRepository>)
                .book_repository(Arc::new(MockBookRepository) as Arc<dyn BookRepository>)
                .import_job_repository(Arc::new(MockImportJobRepository) as Arc<dyn ImportJobRepository>)
                .job_repository(Arc::new(MockJobRepository) as Arc<dyn JobRepository>)
                .shelf_repository(Arc::new(MockShelfRepository) as Arc<dyn ShelfRepository>)
                .user_book_metadata_repository(Arc::new(MockUserBookMetadataRepository) as Arc<dyn UserBookMetadataRepository>)
                .device_repository(Arc::new(MockDeviceRepository) as Arc<dyn DeviceRepository>)
                .build()
                .expect("all fields provided"),
        );
        OpdsServiceImpl::new(repository_service)
    }

    // ── get_or_create_password ──────────────────────────────────────────────

    #[tokio::test]
    async fn get_or_create_password_creates_when_none_exists() {
        let svc = create_service(MockUserSettingRepository::default().with_get_result(Ok(None)).with_set_result(Ok(UserSetting {
            user_id: 1,
            key: OPDS_PASSWORD_KEY.to_owned(),
            value: "hashed".to_owned(),
        })));

        let result = svc.get_or_create_password(&user_with_opds_access()).await.unwrap();
        assert!(result.is_some());
        let pw = result.unwrap();
        assert_eq!(pw.len(), OPDS_PASSWORD_LENGTH);
        assert!(pw.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[tokio::test]
    async fn get_or_create_password_returns_none_when_password_exists() {
        let svc = create_service(MockUserSettingRepository::default().with_get_result(Ok(Some(UserSetting {
            user_id: 1,
            key: OPDS_PASSWORD_KEY.to_owned(),
            value: "existing-hash".to_owned(),
        }))));

        let result = svc.get_or_create_password(&user_with_opds_access()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn get_or_create_password_rejects_user_without_capability() {
        let svc = create_service(MockUserSettingRepository::default());

        let result = svc.get_or_create_password(&user_without_opds_access()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("OPDS access"));
    }

    // ── regenerate_password ─────────────────────────────────────────────────

    #[tokio::test]
    async fn regenerate_password_returns_new_password() {
        let svc = create_service(MockUserSettingRepository::default().with_set_result(Ok(UserSetting {
            user_id: 1,
            key: OPDS_PASSWORD_KEY.to_owned(),
            value: "hashed".to_owned(),
        })));

        let pw = svc.regenerate_password(&user_with_opds_access()).await.unwrap();
        assert_eq!(pw.len(), OPDS_PASSWORD_LENGTH);
        assert!(pw.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[tokio::test]
    async fn regenerate_password_rejects_user_without_capability() {
        let svc = create_service(MockUserSettingRepository::default());

        let result = svc.regenerate_password(&user_without_opds_access()).await;
        assert!(result.is_err());
    }

    // ── verify_password ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn verify_password_returns_true_for_correct_password() {
        let plaintext = "testpassword";
        let hash = hash_opds_password(plaintext).unwrap();

        let svc = create_service(MockUserSettingRepository::default().with_get_result(Ok(Some(UserSetting {
            user_id: 1,
            key: OPDS_PASSWORD_KEY.to_owned(),
            value: hash,
        }))));

        let valid = svc.verify_password(&user_with_opds_access(), plaintext).await.unwrap();
        assert!(valid);
    }

    #[tokio::test]
    async fn verify_password_returns_false_for_wrong_password() {
        let hash = hash_opds_password("correct").unwrap();

        let svc = create_service(MockUserSettingRepository::default().with_get_result(Ok(Some(UserSetting {
            user_id: 1,
            key: OPDS_PASSWORD_KEY.to_owned(),
            value: hash,
        }))));

        let valid = svc.verify_password(&user_with_opds_access(), "wrong").await.unwrap();
        assert!(!valid);
    }

    #[tokio::test]
    async fn verify_password_returns_false_when_no_password_stored() {
        let svc = create_service(MockUserSettingRepository::default().with_get_result(Ok(None)));

        let valid = svc.verify_password(&user_with_opds_access(), "anything").await.unwrap();
        assert!(!valid);
    }

    #[tokio::test]
    async fn verify_password_returns_false_without_capability() {
        let svc = create_service(MockUserSettingRepository::default());

        let valid = svc.verify_password(&user_without_opds_access(), "anything").await.unwrap();
        assert!(!valid);
    }

    // ── has_password ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn has_password_returns_true_when_exists() {
        let svc = create_service(MockUserSettingRepository::default().with_get_result(Ok(Some(UserSetting {
            user_id: 1,
            key: OPDS_PASSWORD_KEY.to_owned(),
            value: "hash".to_owned(),
        }))));

        assert!(svc.has_password(&user_with_opds_access()).await.unwrap());
    }

    #[tokio::test]
    async fn has_password_returns_false_when_absent() {
        let svc = create_service(MockUserSettingRepository::default().with_get_result(Ok(None)));

        assert!(!svc.has_password(&user_with_opds_access()).await.unwrap());
    }
}
