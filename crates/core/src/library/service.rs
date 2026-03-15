use std::sync::Arc;

use crate::{
    Error, RepositoryError,
    book::{BookToken, FileRole},
    repository::{RepositoryService, read_only_transaction, transaction},
    storage::LibraryStore,
};

pub struct LibraryStats {
    pub books: u64,
    pub authors: u64,
}

#[async_trait::async_trait]
pub trait LibraryService: Send + Sync {
    /// Returns aggregate counts for the library.
    async fn library_stats(&self) -> Result<LibraryStats, Error>;

    /// Permanently deletes a book and its files from the library.
    ///
    /// Removes all DB records (book, authors/identifiers join rows, and orphan
    /// authors with no remaining books) then deletes the book directory from
    /// the library store.
    async fn delete_book(&self, book_token: &BookToken) -> Result<(), Error>;
}

pub struct LibraryServiceImpl {
    repository_service: Arc<RepositoryService>,
    library_store: Arc<dyn LibraryStore>,
}

impl LibraryServiceImpl {
    pub(crate) fn new(repository_service: Arc<RepositoryService>, library_store: Arc<dyn LibraryStore>) -> Self {
        Self {
            repository_service,
            library_store,
        }
    }
}

#[async_trait::async_trait]
impl LibraryService for LibraryServiceImpl {
    async fn library_stats(&self) -> Result<LibraryStats, Error> {
        let book_repo = self.repository_service.book_repository().clone();
        let author_repo = self.repository_service.author_repository().clone();

        read_only_transaction(&**self.repository_service.repository(), |tx| {
            Box::pin(async move {
                let books = book_repo.count_available_books(tx).await?;
                let authors = author_repo.count_authors(tx).await?;
                Ok(LibraryStats { books, authors })
            })
        })
        .await
    }

    async fn delete_book(&self, book_token: &BookToken) -> Result<(), Error> {
        let token = *book_token;
        let book_repo = self.repository_service.book_repository().clone();
        let author_repo = self.repository_service.author_repository().clone();
        let job_repo = self.repository_service.import_job_repository().clone();

        let original_filenames = transaction(&**self.repository_service.repository(), |tx| {
            let br = book_repo.clone();
            let ar = author_repo.clone();
            let jr = job_repo.clone();
            Box::pin(async move {
                let book = br.find_by_token(tx, &token).await?.ok_or(Error::RepositoryError(RepositoryError::NotFound))?;

                let author_links = br.authors_for_book(tx, book.id).await?;
                let author_ids: Vec<u64> = author_links.iter().map(|a| a.author_id).collect();

                // Collect original filenames before deleting records.
                let original_filenames: Vec<String> = br
                    .files_for_book(tx, book.id)
                    .await?
                    .into_iter()
                    .filter(|f| f.file_role == FileRole::Original)
                    .filter_map(|f| f.original_filename)
                    .collect();

                // Delete the originating import job so the file can be re-imported.
                if let Some(job) = jr.find_by_candidate_book_id(tx, book.id).await? {
                    jr.delete_job(tx, job.id).await?;
                }

                br.delete_book_authors(tx, book.id).await?;
                br.delete_book_identifiers(tx, book.id).await?;
                br.delete_book(tx, book.id).await?;

                for author_id in author_ids {
                    if br.count_books_for_author(tx, author_id).await? == 0 {
                        ar.delete_author(tx, author_id).await?;
                    }
                }

                Ok(original_filenames)
            })
        })
        .await?;

        self.library_store.delete_book(&token).await?;
        for filename in original_filenames {
            self.library_store.delete_original_file(&filename).await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        any::Any,
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    use super::{LibraryService, LibraryServiceImpl};
    use crate::{
        Error, RepositoryError,
        auth::{NewSession, Session, repository::SessionRepository},
        book::{
            Author, AuthorId, AuthorRepository, AuthorRole, AuthorToken, Book, BookAuthor, BookFile, BookId, BookIdentifier, BookQuery, BookRepository,
            BookStatus, BookToken, FileFormat, FileRole, Genre, GenreId, GenreRepository, GenreToken, IdentifierType, NewAuthor, NewBook, NewGenre,
            NewPublisher, NewSeries, NewTag, Publisher, PublisherId, PublisherRepository, PublisherToken, Series, SeriesId, SeriesRepository, SeriesToken, Tag,
            TagId, TagRepository, TagToken,
        },
        device::{Device, DeviceBook, DeviceId, DeviceRepository, DeviceSyncLog, DeviceToken, NewDevice, NewDeviceSyncLog},
        import::{ImportJob, ImportJobId, ImportJobRepository, ImportJobToken, ImportStatus, NewImportJob},
        jobs::{Job, JobRepository},
        reading::{ReadStatus, UserBookMetadata, UserBookMetadataRepository},
        repository::{Repository, RepositoryServiceBuilder, Transaction},
        shelf::{BookShelf, NewShelf, Shelf, ShelfId, ShelfRepository, ShelfToken},
        storage::{BookSidecar, LibraryStore},
        user::{
            NewUser, NewUserSetting, User, UserId, UserSetting,
            repository::{UserRepository, UserSettingRepository},
        },
    };

    // ─── Mock Transaction / Repository ───────────────────────────────────────

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

    // ─── Unimplemented stubs (repos not exercised by LibraryService) ─────────

    struct StubSessionRepository;
    #[async_trait::async_trait]
    impl SessionRepository for StubSessionRepository {
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

    struct StubUserRepository;
    #[async_trait::async_trait]
    impl UserRepository for StubUserRepository {
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

    struct StubUserSettingRepository;
    #[async_trait::async_trait]
    impl UserSettingRepository for StubUserSettingRepository {
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

    struct StubSeriesRepository;
    #[async_trait::async_trait]
    impl SeriesRepository for StubSeriesRepository {
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

    struct StubGenreRepository;
    #[async_trait::async_trait]
    impl GenreRepository for StubGenreRepository {
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

    struct StubPublisherRepository;
    #[async_trait::async_trait]
    impl PublisherRepository for StubPublisherRepository {
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

    struct StubTagRepository;
    #[async_trait::async_trait]
    impl TagRepository for StubTagRepository {
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

    struct StubJobRepository;
    #[async_trait::async_trait]
    impl JobRepository for StubJobRepository {
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

    struct StubShelfRepository;
    #[async_trait::async_trait]
    impl ShelfRepository for StubShelfRepository {
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
        async fn books_for_filter(
            &self,
            _: &dyn Transaction,
            _: &crate::filter::BookFilter,
            _: UserId,
            _: Option<BookId>,
            _: Option<u64>,
        ) -> Result<Vec<Book>, Error> {
            unimplemented!()
        }
        async fn count_for_filter(&self, _: &dyn Transaction, _: &crate::filter::BookFilter, _: UserId) -> Result<u64, Error> {
            unimplemented!()
        }
        async fn find_by_device_id(&self, _: &dyn Transaction, _: DeviceId) -> Result<Option<Shelf>, Error> {
            unimplemented!()
        }
    }

    struct StubUserBookMetadataRepository;
    #[async_trait::async_trait]
    impl UserBookMetadataRepository for StubUserBookMetadataRepository {
        async fn upsert(&self, _: &dyn Transaction, _: UserBookMetadata) -> Result<UserBookMetadata, Error> {
            unimplemented!()
        }
        async fn find_by_user_and_book(&self, _: &dyn Transaction, _: UserId, _: BookId) -> Result<Option<UserBookMetadata>, Error> {
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
    }

    struct StubDeviceRepository;
    #[async_trait::async_trait]
    impl DeviceRepository for StubDeviceRepository {
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

    // ─── Configurable MockBookRepository ─────────────────────────────────────

    #[derive(Default)]
    struct MockBookRepository {
        find_by_token_result: Option<Result<Option<Book>, Error>>,
        authors_for_book_result: Option<Result<Vec<BookAuthor>, Error>>,
        files_for_book_result: Option<Result<Vec<BookFile>, Error>>,
        count_available_books_result: Option<Result<u64, Error>>,
        count_books_for_author_result: Option<Result<u64, Error>>,
    }

    impl MockBookRepository {
        fn with_find_by_token(mut self, r: Result<Option<Book>, Error>) -> Self {
            self.find_by_token_result = Some(r);
            self
        }
        fn with_authors_for_book(mut self, r: Result<Vec<BookAuthor>, Error>) -> Self {
            self.authors_for_book_result = Some(r);
            self
        }
        fn with_files_for_book(mut self, r: Result<Vec<BookFile>, Error>) -> Self {
            self.files_for_book_result = Some(r);
            self
        }
        fn with_count_available_books(mut self, r: Result<u64, Error>) -> Self {
            self.count_available_books_result = Some(r);
            self
        }
        fn with_count_books_for_author(mut self, r: Result<u64, Error>) -> Self {
            self.count_books_for_author_result = Some(r);
            self
        }
    }

    #[async_trait::async_trait]
    impl BookRepository for MockBookRepository {
        async fn find_by_token(&self, _: &dyn Transaction, _: &BookToken) -> Result<Option<Book>, Error> {
            self.find_by_token_result
                .clone()
                .unwrap_or_else(|| Err(Error::MockNotConfigured("find_by_token")))
        }
        async fn authors_for_book(&self, _: &dyn Transaction, _: BookId) -> Result<Vec<BookAuthor>, Error> {
            self.authors_for_book_result
                .clone()
                .unwrap_or_else(|| Err(Error::MockNotConfigured("authors_for_book")))
        }
        async fn files_for_book(&self, _: &dyn Transaction, _: BookId) -> Result<Vec<BookFile>, Error> {
            self.files_for_book_result
                .clone()
                .unwrap_or_else(|| Err(Error::MockNotConfigured("files_for_book")))
        }
        async fn count_available_books(&self, _: &dyn Transaction) -> Result<u64, Error> {
            self.count_available_books_result
                .clone()
                .unwrap_or_else(|| Err(Error::MockNotConfigured("count_available_books")))
        }
        async fn count_books_for_author(&self, _: &dyn Transaction, _: AuthorId) -> Result<u64, Error> {
            self.count_books_for_author_result
                .clone()
                .unwrap_or_else(|| Err(Error::MockNotConfigured("count_books_for_author")))
        }
        // Mutating ops used during delete — just succeed.
        async fn delete_book(&self, _: &dyn Transaction, _: BookId) -> Result<(), Error> {
            Ok(())
        }
        async fn delete_book_authors(&self, _: &dyn Transaction, _: BookId) -> Result<(), Error> {
            Ok(())
        }
        async fn delete_book_identifiers(&self, _: &dyn Transaction, _: BookId) -> Result<(), Error> {
            Ok(())
        }
        // Unimplemented — not exercised by LibraryService.
        async fn add_book(&self, _: &dyn Transaction, _: NewBook) -> Result<Book, Error> {
            unimplemented!()
        }
        async fn update_book(&self, _: &dyn Transaction, _: Book) -> Result<Book, Error> {
            unimplemented!()
        }
        async fn find_by_id(&self, _: &dyn Transaction, _: BookId) -> Result<Option<Book>, Error> {
            unimplemented!()
        }
        async fn list_books(&self, _: &dyn Transaction, _: &BookQuery, _: Option<BookId>, _: Option<u64>) -> Result<Vec<Book>, Error> {
            unimplemented!()
        }
        async fn identifiers_for_book(&self, _: &dyn Transaction, _: BookId) -> Result<Vec<BookIdentifier>, Error> {
            unimplemented!()
        }
        async fn find_file_by_hash(&self, _: &dyn Transaction, _: &str) -> Result<Option<BookFile>, Error> {
            unimplemented!()
        }
        async fn add_book_file(
            &self,
            _: &dyn Transaction,
            _: BookId,
            _: FileFormat,
            _: FileRole,
            _: Option<String>,
            _: i64,
            _: String,
        ) -> Result<BookFile, Error> {
            unimplemented!()
        }
        async fn add_book_author(&self, _: &dyn Transaction, _: BookId, _: AuthorId, _: AuthorRole, _: i32) -> Result<(), Error> {
            unimplemented!()
        }
        async fn add_book_identifier(&self, _: &dyn Transaction, _: BookId, _: IdentifierType, _: String) -> Result<(), Error> {
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
    }

    // ─── Configurable MockAuthorRepository ───────────────────────────────────

    struct MockAuthorRepository {
        count_authors_result: Option<Result<u64, Error>>,
        delete_author_calls: Arc<Mutex<Vec<AuthorId>>>,
    }

    impl MockAuthorRepository {
        fn new(count_authors: Option<Result<u64, Error>>) -> Self {
            Self {
                count_authors_result: count_authors,
                delete_author_calls: Arc::new(Mutex::new(vec![])),
            }
        }
        fn tracking_deletes(count_authors: Result<u64, Error>) -> (Self, Arc<Mutex<Vec<AuthorId>>>) {
            let calls = Arc::new(Mutex::new(vec![]));
            let repo = Self {
                count_authors_result: Some(count_authors),
                delete_author_calls: Arc::clone(&calls),
            };
            (repo, calls)
        }
    }

    #[async_trait::async_trait]
    impl AuthorRepository for MockAuthorRepository {
        async fn count_authors(&self, _: &dyn Transaction) -> Result<u64, Error> {
            self.count_authors_result
                .clone()
                .unwrap_or_else(|| Err(Error::MockNotConfigured("count_authors")))
        }
        async fn delete_author(&self, _: &dyn Transaction, id: AuthorId) -> Result<(), Error> {
            self.delete_author_calls.lock().unwrap().push(id);
            Ok(())
        }
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
    }

    // ─── Configurable MockImportJobRepository ─────────────────────────────────

    struct MockImportJobRepository {
        find_by_candidate_book_id_result: Option<Result<Option<ImportJob>, Error>>,
        delete_job_calls: Arc<Mutex<Vec<ImportJobId>>>,
    }

    impl MockImportJobRepository {
        fn returning_no_job() -> Self {
            Self {
                find_by_candidate_book_id_result: Some(Ok(None)),
                delete_job_calls: Arc::new(Mutex::new(vec![])),
            }
        }
        fn returning_job(job: ImportJob) -> (Self, Arc<Mutex<Vec<ImportJobId>>>) {
            let calls = Arc::new(Mutex::new(vec![]));
            let repo = Self {
                find_by_candidate_book_id_result: Some(Ok(Some(job))),
                delete_job_calls: Arc::clone(&calls),
            };
            (repo, calls)
        }
    }

    #[async_trait::async_trait]
    impl ImportJobRepository for MockImportJobRepository {
        async fn find_by_candidate_book_id(&self, _: &dyn Transaction, _: BookId) -> Result<Option<ImportJob>, Error> {
            self.find_by_candidate_book_id_result
                .clone()
                .unwrap_or_else(|| Err(Error::MockNotConfigured("find_by_candidate_book_id")))
        }
        async fn delete_job(&self, _: &dyn Transaction, id: ImportJobId) -> Result<(), Error> {
            self.delete_job_calls.lock().unwrap().push(id);
            Ok(())
        }
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
        async fn approve_job(&self, _: &dyn Transaction, _: ImportJobId) -> Result<(), Error> {
            unimplemented!()
        }
    }

    // ─── Configurable MockLibraryStore ────────────────────────────────────────

    #[derive(Default)]
    struct MockLibraryStore {
        deleted_original_files: Arc<Mutex<Vec<String>>>,
    }

    impl MockLibraryStore {
        fn tracking() -> (Self, Arc<Mutex<Vec<String>>>) {
            let calls = Arc::new(Mutex::new(vec![]));
            let store = Self {
                deleted_original_files: Arc::clone(&calls),
            };
            (store, calls)
        }
    }

    #[async_trait::async_trait]
    impl LibraryStore for MockLibraryStore {
        fn original_file_path(&self, _: &str) -> PathBuf {
            unimplemented!()
        }
        fn book_file_path(&self, _: &BookToken, _: &str, _: FileFormat) -> PathBuf {
            unimplemented!()
        }
        fn cover_path(&self, _: &BookToken, _: &str) -> PathBuf {
            unimplemented!()
        }
        fn metadata_path(&self, _: &BookToken) -> PathBuf {
            unimplemented!()
        }
        async fn store_original_file(&self, _: &str, _: &str, _: &std::path::Path) -> Result<String, Error> {
            unimplemented!()
        }
        async fn store_book_file(&self, _: &BookToken, _: &str, _: FileFormat, _: &std::path::Path) -> Result<(), Error> {
            unimplemented!()
        }
        async fn store_cover(&self, _: &BookToken, _: &str, _: &[u8]) -> Result<(), Error> {
            unimplemented!()
        }
        async fn store_metadata(&self, _: &BookToken, _: &BookSidecar) -> Result<(), Error> {
            unimplemented!()
        }
        async fn rename_book_files(&self, _: &BookToken, _: &str, _: &str) -> Result<(), Error> {
            unimplemented!()
        }
        async fn delete_book(&self, _: &BookToken) -> Result<(), Error> {
            Ok(())
        }
        async fn delete_original_file(&self, filename: &str) -> Result<(), Error> {
            self.deleted_original_files.lock().unwrap().push(filename.to_owned());
            Ok(())
        }
    }

    // ─── Service builder ──────────────────────────────────────────────────────

    fn create_service(
        book_repo: MockBookRepository,
        author_repo: MockAuthorRepository,
        job_repo: MockImportJobRepository,
        library_store: MockLibraryStore,
    ) -> LibraryServiceImpl {
        let repository_service = Arc::new(
            RepositoryServiceBuilder::default()
                .repository(Arc::new(MockRepository) as Arc<dyn Repository>)
                .session_repository(Arc::new(StubSessionRepository) as Arc<dyn SessionRepository>)
                .user_repository(Arc::new(StubUserRepository) as Arc<dyn UserRepository>)
                .user_setting_repository(Arc::new(StubUserSettingRepository) as Arc<dyn UserSettingRepository>)
                .author_repository(Arc::new(author_repo) as Arc<dyn AuthorRepository>)
                .series_repository(Arc::new(StubSeriesRepository) as Arc<dyn SeriesRepository>)
                .publisher_repository(Arc::new(StubPublisherRepository) as Arc<dyn PublisherRepository>)
                .genre_repository(Arc::new(StubGenreRepository) as Arc<dyn GenreRepository>)
                .tag_repository(Arc::new(StubTagRepository) as Arc<dyn TagRepository>)
                .book_repository(Arc::new(book_repo) as Arc<dyn BookRepository>)
                .import_job_repository(Arc::new(job_repo) as Arc<dyn ImportJobRepository>)
                .job_repository(Arc::new(StubJobRepository) as Arc<dyn JobRepository>)
                .shelf_repository(Arc::new(StubShelfRepository) as Arc<dyn ShelfRepository>)
                .user_book_metadata_repository(Arc::new(StubUserBookMetadataRepository) as Arc<dyn UserBookMetadataRepository>)
                .device_repository(Arc::new(StubDeviceRepository) as Arc<dyn DeviceRepository>)
                .build()
                .expect("all fields provided"),
        );
        LibraryServiceImpl::new(repository_service, Arc::new(library_store))
    }

    fn fake_book_with_id(id: BookId) -> Book {
        Book::fake(id, "Test Book", BookStatus::Available)
    }

    fn fake_book_author_link(book_id: BookId, author_id: AuthorId) -> BookAuthor {
        BookAuthor::fake(book_id, author_id, "Author", 0)
    }

    fn fake_import_job(id: ImportJobId, book_id: BookId) -> ImportJob {
        ImportJob {
            id,
            version: 1,
            token: ImportJobToken::new(id),
            file_path: "/watch/test.epub".to_owned(),
            file_hash: "abc123".to_owned(),
            file_format: FileFormat::Epub,
            detected_at: chrono::Utc::now(),
            status: ImportStatus::Approved,
            candidate_book_id: Some(book_id),
            metadata_source: None,
            error_message: None,
            reviewed_by: None,
            reviewed_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    // ─── library_stats ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn library_stats_returns_counts() {
        let svc = create_service(
            MockBookRepository::default().with_count_available_books(Ok(5)),
            MockAuthorRepository::new(Some(Ok(3))),
            MockImportJobRepository::returning_no_job(),
            MockLibraryStore::default(),
        );

        let stats = svc.library_stats().await.unwrap();

        assert_eq!(stats.books, 5);
        assert_eq!(stats.authors, 3);
    }

    #[tokio::test]
    async fn library_stats_returns_zeroes_when_empty() {
        let svc = create_service(
            MockBookRepository::default().with_count_available_books(Ok(0)),
            MockAuthorRepository::new(Some(Ok(0))),
            MockImportJobRepository::returning_no_job(),
            MockLibraryStore::default(),
        );

        let stats = svc.library_stats().await.unwrap();

        assert_eq!(stats.books, 0);
        assert_eq!(stats.authors, 0);
    }

    // ─── delete_book ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn delete_book_returns_not_found_when_book_missing() {
        let svc = create_service(
            MockBookRepository::default().with_find_by_token(Ok(None)),
            MockAuthorRepository::new(None),
            MockImportJobRepository::returning_no_job(),
            MockLibraryStore::default(),
        );
        let token = BookToken::new(99);

        let result = svc.delete_book(&token).await;

        assert!(
            matches!(result, Err(Error::RepositoryError(RepositoryError::NotFound))),
            "expected NotFound, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn delete_book_succeeds_with_no_authors_no_import_job() {
        let book_id: BookId = 1;
        let svc = create_service(
            MockBookRepository::default()
                .with_find_by_token(Ok(Some(fake_book_with_id(book_id))))
                .with_authors_for_book(Ok(vec![]))
                .with_files_for_book(Ok(vec![])),
            MockAuthorRepository::new(None),
            MockImportJobRepository::returning_no_job(),
            MockLibraryStore::default(),
        );
        let token = BookToken::new(book_id);

        svc.delete_book(&token).await.unwrap();
    }

    #[tokio::test]
    async fn delete_book_removes_orphan_author() {
        let book_id: BookId = 1;
        let author_id: AuthorId = 42;
        let (author_repo, delete_calls) = MockAuthorRepository::tracking_deletes(Ok(0));
        let svc = create_service(
            MockBookRepository::default()
                .with_find_by_token(Ok(Some(fake_book_with_id(book_id))))
                .with_authors_for_book(Ok(vec![fake_book_author_link(book_id, author_id)]))
                .with_files_for_book(Ok(vec![]))
                .with_count_books_for_author(Ok(0)), // no remaining books → orphan
            author_repo,
            MockImportJobRepository::returning_no_job(),
            MockLibraryStore::default(),
        );
        let token = BookToken::new(book_id);

        svc.delete_book(&token).await.unwrap();

        assert_eq!(*delete_calls.lock().unwrap(), vec![author_id], "orphan author must be deleted");
    }

    #[tokio::test]
    async fn delete_book_preserves_author_with_other_books() {
        let book_id: BookId = 1;
        let author_id: AuthorId = 42;
        let (author_repo, delete_calls) = MockAuthorRepository::tracking_deletes(Ok(0));
        let svc = create_service(
            MockBookRepository::default()
                .with_find_by_token(Ok(Some(fake_book_with_id(book_id))))
                .with_authors_for_book(Ok(vec![fake_book_author_link(book_id, author_id)]))
                .with_files_for_book(Ok(vec![]))
                .with_count_books_for_author(Ok(1)), // still has 1 other book → not an orphan
            author_repo,
            MockImportJobRepository::returning_no_job(),
            MockLibraryStore::default(),
        );
        let token = BookToken::new(book_id);

        svc.delete_book(&token).await.unwrap();

        assert!(delete_calls.lock().unwrap().is_empty(), "non-orphan author must not be deleted");
    }

    #[tokio::test]
    async fn delete_book_removes_linked_import_job() {
        let book_id: BookId = 1;
        let job_id: ImportJobId = 99;
        let job = fake_import_job(job_id, book_id);
        let (job_repo, delete_calls) = MockImportJobRepository::returning_job(job);
        let svc = create_service(
            MockBookRepository::default()
                .with_find_by_token(Ok(Some(fake_book_with_id(book_id))))
                .with_authors_for_book(Ok(vec![]))
                .with_files_for_book(Ok(vec![])),
            MockAuthorRepository::new(None),
            job_repo,
            MockLibraryStore::default(),
        );
        let token = BookToken::new(book_id);

        svc.delete_book(&token).await.unwrap();

        assert_eq!(*delete_calls.lock().unwrap(), vec![job_id], "linked import job must be deleted");
    }

    #[tokio::test]
    async fn delete_book_deletes_original_files_from_store() {
        let book_id: BookId = 1;
        let (store, deleted_files) = MockLibraryStore::tracking();
        let mut file = BookFile::fake(book_id, "epub");
        file.original_filename = Some("test.epub".to_owned());
        let svc = create_service(
            MockBookRepository::default()
                .with_find_by_token(Ok(Some(fake_book_with_id(book_id))))
                .with_authors_for_book(Ok(vec![]))
                .with_files_for_book(Ok(vec![file])),
            MockAuthorRepository::new(None),
            MockImportJobRepository::returning_no_job(),
            store,
        );
        let token = BookToken::new(book_id);

        svc.delete_book(&token).await.unwrap();

        assert_eq!(*deleted_files.lock().unwrap(), vec!["test.epub"], "original file must be removed from store");
    }
}
