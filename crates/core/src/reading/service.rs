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

    /// Applies a device-reported reading state update.
    ///
    /// The state machine transitions (`date_started`, `date_finished`,
    /// `times_read`) are applied for the given `new_status`, then the
    /// position, progress, and reading-time fields are set from the device
    /// report. The caller (Kobo handler) is responsible for applying
    /// status-driven overrides before calling this method (e.g. clearing
    /// the position token when `Finished` is reported).
    #[allow(clippy::too_many_arguments, reason = "Required to capture state")]
    async fn sync_device_state(
        &self,
        user_id: UserId,
        book_id: BookId,
        new_status: ReadStatus,
        progress_bps: Option<u16>,
        position_type: Option<String>,
        position_token: Option<String>,
        spent_reading_minutes: Option<i32>,
        remaining_time_minutes: Option<i32>,
        last_progress_at: Option<chrono::DateTime<Utc>>,
    ) -> Result<UserBookMetadata, Error>;
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
pub(crate) fn default_state(user_id: UserId, book_id: BookId) -> UserBookMetadata {
    UserBookMetadata {
        user_id,
        book_id,
        read_status: ReadStatus::Unread,
        progress_percentage: None,
        position_type: None,
        position_token: None,
        last_progress_at: None,
        spent_reading_minutes: None,
        remaining_time_minutes: None,
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
pub(crate) fn apply_transition(mut current: UserBookMetadata, target: ReadStatus) -> UserBookMetadata {
    let now = Utc::now();

    match &target {
        ReadStatus::Unread => {
            current.read_status = ReadStatus::Unread;
            current.progress_percentage = None;
            current.position_type = None;
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
            current.position_type = None;
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

    #[tracing::instrument(level = "trace", skip(self))]
    #[allow(clippy::too_many_arguments, reason = "Required to capture state")]
    async fn sync_device_state(
        &self,
        user_id: UserId,
        book_id: BookId,
        new_status: ReadStatus,
        progress_bps: Option<u16>,
        position_type: Option<String>,
        position_token: Option<String>,
        spent_reading_minutes: Option<i32>,
        remaining_time_minutes: Option<i32>,
        last_progress_at: Option<chrono::DateTime<Utc>>,
    ) -> Result<UserBookMetadata, Error> {
        with_transaction!(self, user_book_metadata_repository, |tx| {
            let current = user_book_metadata_repository
                .find_by_user_and_book(tx, user_id, book_id)
                .await?
                .unwrap_or_else(|| default_state(user_id, book_id));

            let mut state = apply_transition(current, new_status);
            state.progress_percentage = progress_bps;
            state.position_type = position_type;
            state.position_token = position_token;
            state.spent_reading_minutes = spent_reading_minutes;
            state.remaining_time_minutes = remaining_time_minutes;
            if let Some(at) = last_progress_at {
                state.last_progress_at = Some(at);
            }
            user_book_metadata_repository.upsert(tx, state).await
        })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::{any::Any, sync::Arc};

    use super::{ReadingService, ReadingServiceImpl};
    use crate::{
        Error, RepositoryError,
        auth::repository::MockSessionRepository,
        book::{
            BookId,
            repository::{
                author::MockAuthorRepository, book::MockBookRepository, genre::MockGenreRepository, publisher::MockPublisherRepository,
                series::MockSeriesRepository, tag::MockTagRepository,
            },
        },
        device::repository::device::MockDeviceRepository,
        import::repository::import_job::MockImportJobRepository,
        jobs::repository::MockJobRepository,
        library::MockLibraryRepository,
        reading::{ReadStatus, UserBookMetadata, repository::user_book_metadata::MockUserBookMetadataRepository},
        repository::{MockRepository, RepositoryServiceBuilder, Transaction},
        shelf::repository::shelf::MockShelfRepository,
        user::{
            UserId,
            repository::{user::MockUserRepository, user_settings::MockUserSettingRepository},
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

    // ─── Helper ───────────────────────────────────────────────────────────────

    fn make_mock_repo() -> MockRepository {
        let mut r = MockRepository::new();
        r.expect_begin()
            .returning(|| Box::pin(async { Ok(Box::new(MockTransaction) as Box<dyn Transaction>) }));
        r.expect_begin_read_only()
            .returning(|| Box::pin(async { Ok(Box::new(MockTransaction) as Box<dyn Transaction>) }));
        r
    }

    fn create_service(mock: MockUserBookMetadataRepository) -> ReadingServiceImpl {
        let repository_service = Arc::new(
            RepositoryServiceBuilder::default()
                .repository(Arc::new(make_mock_repo()))
                .session_repository(Arc::new(MockSessionRepository::new()))
                .user_repository(Arc::new(MockUserRepository::new()))
                .user_setting_repository(Arc::new(MockUserSettingRepository::new()))
                .author_repository(Arc::new(MockAuthorRepository::new()))
                .series_repository(Arc::new(MockSeriesRepository::new()))
                .publisher_repository(Arc::new(MockPublisherRepository::new()))
                .genre_repository(Arc::new(MockGenreRepository::new()))
                .tag_repository(Arc::new(MockTagRepository::new()))
                .book_repository(Arc::new(MockBookRepository::new()))
                .import_job_repository(Arc::new(MockImportJobRepository::new()))
                .job_repository(Arc::new(MockJobRepository::new()))
                .library_repository(Arc::new(MockLibraryRepository::new()))
                .shelf_repository(Arc::new(MockShelfRepository::new()))
                .user_book_metadata_repository(Arc::new(mock))
                .device_repository(Arc::new(MockDeviceRepository::new()))
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
            position_type: None,
            position_token: None,
            last_progress_at: None,
            spent_reading_minutes: None,
            remaining_time_minutes: None,
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
        let mut mock = MockUserBookMetadataRepository::new();
        mock.expect_find_by_user_and_book().returning(|_, _, _| Box::pin(async { Ok(None) }));
        let svc = create_service(mock);
        let result = svc.get_reading_state(1, 1).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_get_reading_state_returns_state_when_found() {
        let existing = state(1, 1, ReadStatus::Reading);
        let mut mock = MockUserBookMetadataRepository::new();
        mock.expect_find_by_user_and_book().returning(move |_, _, _| {
            let existing = existing.clone();
            Box::pin(async move { Ok(Some(existing)) })
        });
        let svc = create_service(mock);
        let result = svc.get_reading_state(1, 1).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().read_status, ReadStatus::Reading);
    }

    // ─── set_status ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_set_status_unread_to_reading_sets_date_started() {
        let mut mock = MockUserBookMetadataRepository::new();
        mock.expect_find_by_user_and_book().returning(|_, _, _| Box::pin(async { Ok(None) }));
        mock.expect_upsert().returning(|_, s| Box::pin(async { Ok(s) }));
        let svc = create_service(mock);
        let result = svc.set_status(1, 1, ReadStatus::Reading).await.unwrap();
        assert_eq!(result.read_status, ReadStatus::Reading);
        assert!(result.date_started.is_some());
    }

    #[tokio::test]
    async fn test_set_status_reading_to_read_increments_times_read_and_sets_date_finished() {
        let existing = state(1, 1, ReadStatus::Reading);
        let mut mock = MockUserBookMetadataRepository::new();
        mock.expect_find_by_user_and_book().returning(move |_, _, _| {
            let existing = existing.clone();
            Box::pin(async move { Ok(Some(existing)) })
        });
        mock.expect_upsert().returning(|_, s| Box::pin(async { Ok(s) }));
        let svc = create_service(mock);
        let result = svc.set_status(1, 1, ReadStatus::Read).await.unwrap();
        assert_eq!(result.read_status, ReadStatus::Read);
        assert_eq!(result.times_read, 1);
        assert!(result.date_finished.is_some());
    }

    #[tokio::test]
    async fn test_set_status_read_to_read_does_not_double_increment() {
        let mut existing = state(1, 1, ReadStatus::Read);
        existing.times_read = 1;
        let mut mock = MockUserBookMetadataRepository::new();
        mock.expect_find_by_user_and_book().returning(move |_, _, _| {
            let existing = existing.clone();
            Box::pin(async move { Ok(Some(existing)) })
        });
        mock.expect_upsert().returning(|_, s| Box::pin(async { Ok(s) }));
        let svc = create_service(mock);
        let result = svc.set_status(1, 1, ReadStatus::Read).await.unwrap();
        // Not coming from Reading/Rereading/Paused, so times_read should not increment
        assert_eq!(result.times_read, 1);
    }

    #[tokio::test]
    async fn test_set_status_reading_to_abandoned_sets_date_finished() {
        let existing = state(1, 1, ReadStatus::Reading);
        let mut mock = MockUserBookMetadataRepository::new();
        mock.expect_find_by_user_and_book().returning(move |_, _, _| {
            let existing = existing.clone();
            Box::pin(async move { Ok(Some(existing)) })
        });
        mock.expect_upsert().returning(|_, s| Box::pin(async { Ok(s) }));
        let svc = create_service(mock);
        let result = svc.set_status(1, 1, ReadStatus::Abandoned).await.unwrap();
        assert_eq!(result.read_status, ReadStatus::Abandoned);
        assert!(result.date_finished.is_some());
    }

    #[tokio::test]
    async fn test_set_status_rereading_resets_progress_and_sets_date_started() {
        let mut existing = state(1, 1, ReadStatus::Read);
        existing.times_read = 1;
        existing.progress_percentage = Some(10000);
        let mut mock = MockUserBookMetadataRepository::new();
        mock.expect_find_by_user_and_book().returning(move |_, _, _| {
            let existing = existing.clone();
            Box::pin(async move { Ok(Some(existing)) })
        });
        mock.expect_upsert().returning(|_, s| Box::pin(async { Ok(s) }));
        let svc = create_service(mock);
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
        let mut mock = MockUserBookMetadataRepository::new();
        mock.expect_find_by_user_and_book().returning(move |_, _, _| {
            let existing = existing.clone();
            Box::pin(async move { Ok(Some(existing)) })
        });
        mock.expect_upsert().returning(|_, s| Box::pin(async { Ok(s) }));
        let svc = create_service(mock);
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
        let mut mock = MockUserBookMetadataRepository::new();
        mock.expect_find_by_user_and_book().returning(|_, _, _| Box::pin(async { Ok(None) }));
        mock.expect_upsert().returning(|_, s| Box::pin(async { Ok(s) }));
        let svc = create_service(mock);
        let result = svc.update_progress(1, 1, 1000, None, None).await.unwrap();
        assert_eq!(result.read_status, ReadStatus::Reading);
        assert_eq!(result.progress_percentage, Some(1000));
        assert!(result.date_started.is_some());
        assert!(result.last_progress_at.is_some());
    }

    #[tokio::test]
    async fn test_update_progress_on_reading_stays_reading_below_threshold() {
        let existing = state(1, 1, ReadStatus::Reading);
        let mut mock = MockUserBookMetadataRepository::new();
        mock.expect_find_by_user_and_book().returning(move |_, _, _| {
            let existing = existing.clone();
            Box::pin(async move { Ok(Some(existing)) })
        });
        mock.expect_upsert().returning(|_, s| Box::pin(async { Ok(s) }));
        let svc = create_service(mock);
        let result = svc.update_progress(1, 1, 5000, None, Some(9500)).await.unwrap();
        assert_eq!(result.read_status, ReadStatus::Reading);
        assert_eq!(result.progress_percentage, Some(5000));
    }

    #[tokio::test]
    async fn test_update_progress_meets_threshold_transitions_to_read() {
        let existing = state(1, 1, ReadStatus::Reading);
        let mut mock = MockUserBookMetadataRepository::new();
        mock.expect_find_by_user_and_book().returning(move |_, _, _| {
            let existing = existing.clone();
            Box::pin(async move { Ok(Some(existing)) })
        });
        mock.expect_upsert().returning(|_, s| Box::pin(async { Ok(s) }));
        let svc = create_service(mock);
        let result = svc.update_progress(1, 1, 9500, None, Some(9500)).await.unwrap();
        assert_eq!(result.read_status, ReadStatus::Read);
        assert_eq!(result.times_read, 1);
    }

    // ─── set_rating ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_set_rating_valid() {
        let mut mock = MockUserBookMetadataRepository::new();
        mock.expect_find_by_user_and_book().returning(|_, _, _| Box::pin(async { Ok(None) }));
        mock.expect_upsert().returning(|_, s| Box::pin(async { Ok(s) }));
        let svc = create_service(mock);
        let result = svc.set_rating(1, 1, 4).await.unwrap();
        assert_eq!(result.personal_rating, Some(4));
    }

    #[tokio::test]
    async fn test_set_rating_zero_returns_validation_error() {
        let svc = create_service(MockUserBookMetadataRepository::new());
        assert!(matches!(svc.set_rating(1, 1, 0).await, Err(Error::Validation(_))));
    }

    #[tokio::test]
    async fn test_set_rating_six_returns_validation_error() {
        let svc = create_service(MockUserBookMetadataRepository::new());
        assert!(matches!(svc.set_rating(1, 1, 6).await, Err(Error::Validation(_))));
    }

    // ─── set_notes ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_set_notes_stores_notes() {
        let mut mock = MockUserBookMetadataRepository::new();
        mock.expect_find_by_user_and_book().returning(|_, _, _| Box::pin(async { Ok(None) }));
        mock.expect_upsert().returning(|_, s| Box::pin(async { Ok(s) }));
        let svc = create_service(mock);
        let result = svc.set_notes(1, 1, "A great book".to_owned()).await.unwrap();
        assert_eq!(result.notes.as_deref(), Some("A great book"));
    }

    // ─── list_for_user ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_list_for_user_propagates_error() {
        let mut mock = MockUserBookMetadataRepository::new();
        mock.expect_list_for_user()
            .returning(|_, _, _, _, _| Box::pin(async { Err(Error::RepositoryError(RepositoryError::Database("db error".into()))) }));
        let svc = create_service(mock);
        assert!(matches!(svc.list_for_user(1, None).await, Err(Error::RepositoryError(_))));
    }
}
