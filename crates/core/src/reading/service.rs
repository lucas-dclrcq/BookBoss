use std::sync::Arc;

use chrono::Utc;

use crate::{
    Error,
    book::BookId,
    reading::{ReadStatus, UserBookMetadata},
    repository::RepositoryService,
    user::UserId,
    with_read_only_transaction, with_transaction,
};

/// User setting key for the auto-read threshold.
///
/// Stored as a basis-point string (e.g. `"9500"` = 95%). Use
/// `DEFAULT_AUTO_READ_THRESHOLD` when the key is absent.
pub const AUTO_READ_THRESHOLD_KEY: &str = "reading.auto_read_threshold";

/// Default auto-read threshold in basis points (9500 = 95%).
///
/// When progress reaches or exceeds this value the service automatically
/// transitions a book from `Reading` to `Read`. Callers may supply a
/// per-user override via `set_status` / `update_progress`; passing `None`
/// disables auto-advance entirely.
pub const DEFAULT_AUTO_READ_THRESHOLD: u16 = 9500;

// ── Trait ─────────────────────────────────────────────────────────────────────

#[async_trait::async_trait]
pub trait ReadingService: Send + Sync {
    /// Returns the current reading state for a user/book pair, or `None` if no
    /// row exists (semantically equivalent to `Unread`).
    async fn get_reading_state(&self, user_id: UserId, book_id: BookId) -> Result<Option<UserBookMetadata>, Error>;

    /// Directly sets the reading status and returns the updated record.
    /// Timestamps (`date_started`, `date_finished`) and `times_read` are
    /// updated as appropriate for the target status.
    async fn set_status(&self, user_id: UserId, book_id: BookId, new_status: ReadStatus) -> Result<UserBookMetadata, Error>;

    /// Records reading progress and automatically manages status transitions.
    ///
    /// - Unread → Reading on the first progress update.
    /// - Reading → Read when `progress_bps` meets or exceeds
    ///   `auto_read_threshold`.
    async fn update_progress(
        &self,
        user_id: UserId,
        book_id: BookId,
        progress_bps: u16,
        position_token: Option<String>,
        auto_read_threshold: Option<u16>,
    ) -> Result<UserBookMetadata, Error>;

    /// Sets a personal star rating (1–5). Returns a validation error for values
    /// outside that range.
    async fn set_rating(&self, user_id: UserId, book_id: BookId, rating: u8) -> Result<UserBookMetadata, Error>;

    /// Clears the personal star rating, setting it to `None`.
    async fn clear_rating(&self, user_id: UserId, book_id: BookId) -> Result<UserBookMetadata, Error>;

    /// Stores free-form reading notes for the user/book pair.
    async fn set_notes(&self, user_id: UserId, book_id: BookId, notes: String) -> Result<UserBookMetadata, Error>;

    /// Returns all reading state rows for a user, optionally filtered by
    /// status.
    async fn list_for_user(&self, user_id: UserId, status: Option<ReadStatus>) -> Result<Vec<UserBookMetadata>, Error>;
}

// ── Impl ──────────────────────────────────────────────────────────────────────

pub struct ReadingServiceImpl {
    repository_service: Arc<RepositoryService>,
}

impl ReadingServiceImpl {
    #[must_use]
    pub fn new(repository_service: Arc<RepositoryService>) -> Self {
        Self { repository_service }
    }
}

// ── State machine helpers
// ─────────────────────────────────────────────────────

/// Returns a blank `UserBookMetadata` row representing the default `Unread`
/// state.
fn default_state(user_id: UserId, book_id: BookId) -> UserBookMetadata {
    UserBookMetadata {
        user_id,
        book_id,
        read_status: ReadStatus::Unread,
        progress_percentage: None,
        position_token: None,
        last_progress_at: None,
        personal_rating: None,
        times_read: 0,
        date_started: None,
        date_finished: None,
        last_opened_at: None,
        notes: None,
    }
}

/// Applies the requested status to `current`, updating timestamps and
/// `times_read` as appropriate.
fn apply_transition(mut current: UserBookMetadata, target: ReadStatus) -> UserBookMetadata {
    let now = Utc::now();

    match &target {
        ReadStatus::Unread => {
            current.read_status = ReadStatus::Unread;
            current.progress_percentage = None;
            current.position_token = None;
            current.date_started = None;
            current.date_finished = None;
            current.last_progress_at = None;
        }
        ReadStatus::Reading => {
            if current.date_started.is_none() {
                current.date_started = Some(now);
            }
            current.read_status = ReadStatus::Reading;
        }
        ReadStatus::Paused => {
            current.read_status = ReadStatus::Paused;
        }
        ReadStatus::Rereading => {
            current.date_started = Some(now);
            current.date_finished = None;
            current.progress_percentage = None;
            current.position_token = None;
            current.read_status = ReadStatus::Rereading;
        }
        ReadStatus::Read => {
            let was_in_progress = matches!(current.read_status, ReadStatus::Reading | ReadStatus::Rereading | ReadStatus::Paused);
            current.read_status = ReadStatus::Read;
            current.date_finished = Some(now);
            if was_in_progress {
                current.times_read += 1;
            }
        }
        ReadStatus::Abandoned => {
            current.read_status = ReadStatus::Abandoned;
            current.date_finished = Some(now);
        }
    }

    current
}

#[async_trait::async_trait]
impl ReadingService for ReadingServiceImpl {
    #[tracing::instrument(level = "trace", skip(self))]
    async fn get_reading_state(&self, user_id: UserId, book_id: BookId) -> Result<Option<UserBookMetadata>, Error> {
        with_read_only_transaction!(self, user_book_metadata_repository, |tx| {
            user_book_metadata_repository.find_by_user_and_book(tx, user_id, book_id).await
        })
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn set_status(&self, user_id: UserId, book_id: BookId, new_status: ReadStatus) -> Result<UserBookMetadata, Error> {
        with_transaction!(self, user_book_metadata_repository, |tx| {
            let current = user_book_metadata_repository
                .find_by_user_and_book(tx, user_id, book_id)
                .await?
                .unwrap_or_else(|| default_state(user_id, book_id));

            let next = apply_transition(current, new_status);
            user_book_metadata_repository.upsert(tx, next).await
        })
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn update_progress(
        &self,
        user_id: UserId,
        book_id: BookId,
        progress_bps: u16,
        position_token: Option<String>,
        auto_read_threshold: Option<u16>,
    ) -> Result<UserBookMetadata, Error> {
        with_transaction!(self, user_book_metadata_repository, |tx| {
            let now = Utc::now();

            let mut current = user_book_metadata_repository
                .find_by_user_and_book(tx, user_id, book_id)
                .await?
                .unwrap_or_else(|| default_state(user_id, book_id));

            // Unread → Reading on first progress update.
            if current.read_status == ReadStatus::Unread {
                current = apply_transition(current, ReadStatus::Reading);
            }

            current.progress_percentage = Some(progress_bps);
            current.position_token = position_token;
            current.last_progress_at = Some(now);

            // Auto-advance to Read if threshold is met.
            if matches!(current.read_status, ReadStatus::Reading | ReadStatus::Rereading) {
                if let Some(threshold) = auto_read_threshold {
                    if progress_bps >= threshold {
                        current = apply_transition(current, ReadStatus::Read);
                    }
                }
            }

            user_book_metadata_repository.upsert(tx, current).await
        })
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn set_rating(&self, user_id: UserId, book_id: BookId, rating: u8) -> Result<UserBookMetadata, Error> {
        if !(1..=5).contains(&rating) {
            return Err(Error::Validation(format!("rating must be between 1 and 5, got {rating}")));
        }

        with_transaction!(self, user_book_metadata_repository, |tx| {
            let mut current = user_book_metadata_repository
                .find_by_user_and_book(tx, user_id, book_id)
                .await?
                .unwrap_or_else(|| default_state(user_id, book_id));

            current.personal_rating = Some(rating);
            user_book_metadata_repository.upsert(tx, current).await
        })
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn clear_rating(&self, user_id: UserId, book_id: BookId) -> Result<UserBookMetadata, Error> {
        with_transaction!(self, user_book_metadata_repository, |tx| {
            let mut current = user_book_metadata_repository
                .find_by_user_and_book(tx, user_id, book_id)
                .await?
                .unwrap_or_else(|| default_state(user_id, book_id));

            current.personal_rating = None;
            user_book_metadata_repository.upsert(tx, current).await
        })
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn set_notes(&self, user_id: UserId, book_id: BookId, notes: String) -> Result<UserBookMetadata, Error> {
        with_transaction!(self, user_book_metadata_repository, |tx| {
            let mut current = user_book_metadata_repository
                .find_by_user_and_book(tx, user_id, book_id)
                .await?
                .unwrap_or_else(|| default_state(user_id, book_id));

            current.notes = Some(notes);
            user_book_metadata_repository.upsert(tx, current).await
        })
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn list_for_user(&self, user_id: UserId, status: Option<ReadStatus>) -> Result<Vec<UserBookMetadata>, Error> {
        with_read_only_transaction!(self, user_book_metadata_repository, |tx| {
            user_book_metadata_repository.list_for_user(tx, user_id, status, None, None).await
        })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::{
        any::Any,
        sync::{Arc, Mutex},
    };

    use super::{ReadingService, ReadingServiceImpl};
    use crate::{
        Error, RepositoryError,
        auth::{NewSession, Session, repository::SessionRepository},
        book::{
            Author, AuthorId, AuthorRepository, AuthorRole, AuthorToken, Book, BookAuthor, BookFile, BookFilter, BookId, BookIdentifier, BookRepository,
            BookToken, FileFormat, Genre, GenreId, GenreRepository, GenreToken, IdentifierType, NewAuthor, NewBook, NewGenre, NewPublisher, NewSeries, NewTag,
            Publisher, PublisherId, PublisherRepository, PublisherToken, Series, SeriesId, SeriesRepository, SeriesToken, Tag, TagId, TagRepository, TagToken,
        },
        import::{ImportJob, ImportJobId, ImportJobRepository, ImportJobToken, ImportStatus, NewImportJob},
        jobs::{Job, JobRepository},
        reading::{ReadStatus, UserBookMetadata, UserBookMetadataRepository},
        repository::{Repository, RepositoryServiceBuilder, Transaction},
        shelf::{BookShelf, NewShelf, Shelf, ShelfFilter, ShelfId, ShelfRepository, ShelfToken},
        user::{
            NewUser, NewUserSetting, User, UserId, UserSetting,
            repository::{UserRepository, UserSettingRepository},
        },
    };

    // ─── Mock Transaction ─────────────────────────────────────────────────────

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

    // ─── Mock Repository ──────────────────────────────────────────────────────

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

    // ─── Stub impls for unused repositories ───────────────────────────────────

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

    struct MockUserSettingRepository;
    #[async_trait::async_trait]
    impl UserSettingRepository for MockUserSettingRepository {
        async fn get(&self, _: &dyn Transaction, _: UserId, _: &str) -> Result<Option<UserSetting>, Error> {
            unimplemented!()
        }
        async fn set(&self, _: &dyn Transaction, _: NewUserSetting) -> Result<UserSetting, Error> {
            unimplemented!()
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
        async fn count_authors(&self, _: &dyn Transaction) -> Result<u64, Error> {
            unimplemented!()
        }
        async fn delete_author(&self, _: &dyn Transaction, _: AuthorId) -> Result<(), Error> {
            unimplemented!()
        }
        async fn list_all_authors(&self, _: &dyn Transaction) -> Result<Vec<Author>, Error> {
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
        async fn list_books(&self, _: &dyn Transaction, _: &BookFilter, _: Option<BookId>, _: Option<u64>) -> Result<Vec<Book>, Error> {
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
        async fn add_book_file(&self, _: &dyn Transaction, _: BookId, _: FileFormat, _: i64, _: String) -> Result<BookFile, Error> {
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
        async fn books_for_filter(&self, _: &dyn Transaction, _: &ShelfFilter, _: UserId, _: Option<BookId>, _: Option<u64>) -> Result<Vec<Book>, Error> {
            unimplemented!()
        }
        async fn count_for_filter(&self, _: &dyn Transaction, _: &ShelfFilter, _: UserId) -> Result<u64, Error> {
            unimplemented!()
        }
    }

    // ─── Configurable mock for UserBookMetadataRepository ────────────────────

    #[derive(Default)]
    struct MockUserBookMetadataRepository {
        find_result: Mutex<Option<Result<Option<UserBookMetadata>, Error>>>,
        upsert_result: Mutex<Option<Result<UserBookMetadata, Error>>>,
        list_result: Mutex<Option<Result<Vec<UserBookMetadata>, Error>>>,
    }

    impl MockUserBookMetadataRepository {
        fn with_find(self, r: Result<Option<UserBookMetadata>, Error>) -> Self {
            *self.find_result.lock().unwrap() = Some(r);
            self
        }
        fn with_list(self, r: Result<Vec<UserBookMetadata>, Error>) -> Self {
            *self.list_result.lock().unwrap() = Some(r);
            self
        }
    }

    #[async_trait::async_trait]
    impl UserBookMetadataRepository for MockUserBookMetadataRepository {
        async fn upsert(&self, _: &dyn Transaction, metadata: UserBookMetadata) -> Result<UserBookMetadata, Error> {
            self.upsert_result.lock().unwrap().clone().unwrap_or(Ok(metadata))
        }
        async fn find_by_user_and_book(&self, _: &dyn Transaction, _: UserId, _: BookId) -> Result<Option<UserBookMetadata>, Error> {
            self.find_result.lock().unwrap().clone().unwrap_or(Ok(None))
        }
        async fn list_for_user(
            &self,
            _: &dyn Transaction,
            _: UserId,
            _: Option<ReadStatus>,
            _: Option<BookId>,
            _: Option<u64>,
        ) -> Result<Vec<UserBookMetadata>, Error> {
            self.list_result.lock().unwrap().clone().unwrap_or(Ok(vec![]))
        }
    }

    // ─── Helper ───────────────────────────────────────────────────────────────

    fn create_service(mock: MockUserBookMetadataRepository) -> ReadingServiceImpl {
        let repository_service = Arc::new(
            RepositoryServiceBuilder::default()
                .repository(Arc::new(MockRepository) as Arc<dyn Repository>)
                .session_repository(Arc::new(MockSessionRepository) as Arc<dyn SessionRepository>)
                .user_repository(Arc::new(MockUserRepository) as Arc<dyn UserRepository>)
                .user_setting_repository(Arc::new(MockUserSettingRepository) as Arc<dyn UserSettingRepository>)
                .author_repository(Arc::new(MockAuthorRepository) as Arc<dyn AuthorRepository>)
                .series_repository(Arc::new(MockSeriesRepository) as Arc<dyn SeriesRepository>)
                .publisher_repository(Arc::new(MockPublisherRepository) as Arc<dyn PublisherRepository>)
                .genre_repository(Arc::new(MockGenreRepository) as Arc<dyn GenreRepository>)
                .tag_repository(Arc::new(MockTagRepository) as Arc<dyn TagRepository>)
                .book_repository(Arc::new(MockBookRepository) as Arc<dyn BookRepository>)
                .import_job_repository(Arc::new(MockImportJobRepository) as Arc<dyn ImportJobRepository>)
                .job_repository(Arc::new(MockJobRepository) as Arc<dyn JobRepository>)
                .shelf_repository(Arc::new(MockShelfRepository) as Arc<dyn ShelfRepository>)
                .user_book_metadata_repository(Arc::new(mock) as Arc<dyn UserBookMetadataRepository>)
                .build()
                .expect("all fields provided"),
        );
        ReadingServiceImpl::new(repository_service)
    }

    fn state(user_id: UserId, book_id: BookId, status: ReadStatus) -> UserBookMetadata {
        UserBookMetadata {
            user_id,
            book_id,
            read_status: status,
            progress_percentage: None,
            position_token: None,
            last_progress_at: None,
            personal_rating: None,
            times_read: 0,
            date_started: None,
            date_finished: None,
            last_opened_at: None,
            notes: None,
        }
    }

    // ─── get_reading_state ────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_get_reading_state_returns_none_when_not_found() {
        let svc = create_service(MockUserBookMetadataRepository::default());
        let result = svc.get_reading_state(1, 1).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_get_reading_state_returns_state_when_found() {
        let existing = state(1, 1, ReadStatus::Reading);
        let svc = create_service(MockUserBookMetadataRepository::default().with_find(Ok(Some(existing))));
        let result = svc.get_reading_state(1, 1).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().read_status, ReadStatus::Reading);
    }

    // ─── set_status ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_set_status_unread_to_reading_sets_date_started() {
        let svc = create_service(MockUserBookMetadataRepository::default());
        let result = svc.set_status(1, 1, ReadStatus::Reading).await.unwrap();
        assert_eq!(result.read_status, ReadStatus::Reading);
        assert!(result.date_started.is_some());
    }

    #[tokio::test]
    async fn test_set_status_reading_to_read_increments_times_read_and_sets_date_finished() {
        let existing = state(1, 1, ReadStatus::Reading);
        let svc = create_service(MockUserBookMetadataRepository::default().with_find(Ok(Some(existing))));
        let result = svc.set_status(1, 1, ReadStatus::Read).await.unwrap();
        assert_eq!(result.read_status, ReadStatus::Read);
        assert_eq!(result.times_read, 1);
        assert!(result.date_finished.is_some());
    }

    #[tokio::test]
    async fn test_set_status_read_to_read_does_not_double_increment() {
        let mut existing = state(1, 1, ReadStatus::Read);
        existing.times_read = 1;
        let svc = create_service(MockUserBookMetadataRepository::default().with_find(Ok(Some(existing))));
        let result = svc.set_status(1, 1, ReadStatus::Read).await.unwrap();
        // Not coming from Reading/Rereading/Paused, so times_read should not increment
        assert_eq!(result.times_read, 1);
    }

    #[tokio::test]
    async fn test_set_status_reading_to_abandoned_sets_date_finished() {
        let existing = state(1, 1, ReadStatus::Reading);
        let svc = create_service(MockUserBookMetadataRepository::default().with_find(Ok(Some(existing))));
        let result = svc.set_status(1, 1, ReadStatus::Abandoned).await.unwrap();
        assert_eq!(result.read_status, ReadStatus::Abandoned);
        assert!(result.date_finished.is_some());
    }

    #[tokio::test]
    async fn test_set_status_rereading_resets_progress_and_sets_date_started() {
        let mut existing = state(1, 1, ReadStatus::Read);
        existing.times_read = 1;
        existing.progress_percentage = Some(10000);
        let svc = create_service(MockUserBookMetadataRepository::default().with_find(Ok(Some(existing))));
        let result = svc.set_status(1, 1, ReadStatus::Rereading).await.unwrap();
        assert_eq!(result.read_status, ReadStatus::Rereading);
        assert_eq!(result.times_read, 1); // unchanged until next Read transition
        assert!(result.date_started.is_some());
        assert!(result.date_finished.is_none());
        assert!(result.progress_percentage.is_none());
    }

    #[tokio::test]
    async fn test_set_status_any_to_unread_clears_dates_and_progress() {
        let mut existing = state(1, 1, ReadStatus::Reading);
        existing.progress_percentage = Some(5000);
        existing.date_started = Some(chrono::Utc::now());
        let svc = create_service(MockUserBookMetadataRepository::default().with_find(Ok(Some(existing))));
        let result = svc.set_status(1, 1, ReadStatus::Unread).await.unwrap();
        assert_eq!(result.read_status, ReadStatus::Unread);
        assert!(result.progress_percentage.is_none());
        assert!(result.date_started.is_none());
        assert!(result.date_finished.is_none());
        assert!(result.last_progress_at.is_none());
    }

    // ─── update_progress ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_update_progress_on_unread_auto_starts_reading() {
        let svc = create_service(MockUserBookMetadataRepository::default());
        let result = svc.update_progress(1, 1, 1000, None, None).await.unwrap();
        assert_eq!(result.read_status, ReadStatus::Reading);
        assert_eq!(result.progress_percentage, Some(1000));
        assert!(result.date_started.is_some());
        assert!(result.last_progress_at.is_some());
    }

    #[tokio::test]
    async fn test_update_progress_on_reading_stays_reading_below_threshold() {
        let existing = state(1, 1, ReadStatus::Reading);
        let svc = create_service(MockUserBookMetadataRepository::default().with_find(Ok(Some(existing))));
        let result = svc.update_progress(1, 1, 5000, None, Some(9500)).await.unwrap();
        assert_eq!(result.read_status, ReadStatus::Reading);
        assert_eq!(result.progress_percentage, Some(5000));
    }

    #[tokio::test]
    async fn test_update_progress_meets_threshold_transitions_to_read() {
        let existing = state(1, 1, ReadStatus::Reading);
        let svc = create_service(MockUserBookMetadataRepository::default().with_find(Ok(Some(existing))));
        let result = svc.update_progress(1, 1, 9500, None, Some(9500)).await.unwrap();
        assert_eq!(result.read_status, ReadStatus::Read);
        assert_eq!(result.times_read, 1);
    }

    // ─── set_rating ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_set_rating_valid() {
        let svc = create_service(MockUserBookMetadataRepository::default());
        let result = svc.set_rating(1, 1, 4).await.unwrap();
        assert_eq!(result.personal_rating, Some(4));
    }

    #[tokio::test]
    async fn test_set_rating_zero_returns_validation_error() {
        let svc = create_service(MockUserBookMetadataRepository::default());
        assert!(matches!(svc.set_rating(1, 1, 0).await, Err(Error::Validation(_))));
    }

    #[tokio::test]
    async fn test_set_rating_six_returns_validation_error() {
        let svc = create_service(MockUserBookMetadataRepository::default());
        assert!(matches!(svc.set_rating(1, 1, 6).await, Err(Error::Validation(_))));
    }

    // ─── set_notes ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_set_notes_stores_notes() {
        let svc = create_service(MockUserBookMetadataRepository::default());
        let result = svc.set_notes(1, 1, "A great book".to_owned()).await.unwrap();
        assert_eq!(result.notes.as_deref(), Some("A great book"));
    }

    // ─── list_for_user ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_list_for_user_propagates_error() {
        let svc =
            create_service(MockUserBookMetadataRepository::default().with_list(Err(Error::RepositoryError(RepositoryError::Database("db error".into())))));
        assert!(matches!(svc.list_for_user(1, None).await, Err(Error::RepositoryError(_))));
    }
}
