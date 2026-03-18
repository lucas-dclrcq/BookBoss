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

                // Collect original file paths before deleting records.
                let original_filenames: Vec<String> = br
                    .files_for_book(tx, book.id)
                    .await?
                    .into_iter()
                    .filter(|f| f.file_role == FileRole::Original)
                    .map(|f| f.path)
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
    use std::{any::Any, sync::Arc};

    use super::{LibraryService, LibraryServiceImpl};
    use crate::{
        Error, RepositoryError,
        auth::repository::MockSessionRepository,
        book::{
            AuthorId, BookAuthor, BookFile, BookId, BookStatus, BookToken, FileRole,
            repository::{
                author::MockAuthorRepository, book::MockBookRepository, genre::MockGenreRepository, publisher::MockPublisherRepository,
                series::MockSeriesRepository, tag::MockTagRepository,
            },
        },
        device::repository::device::MockDeviceRepository,
        import::{ImportJob, ImportJobId, ImportJobToken, ImportStatus, repository::import_job::MockImportJobRepository},
        jobs::repository::MockJobRepository,
        reading::repository::user_book_metadata::MockUserBookMetadataRepository,
        repository::{MockRepository, RepositoryServiceBuilder, Transaction},
        shelf::repository::shelf::MockShelfRepository,
        storage::store::MockLibraryStore,
        user::repository::{user::MockUserRepository, user_settings::MockUserSettingRepository},
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

    // ─── Helper: build a MockRepository ───────────────────────────────────────

    fn make_mock_repo() -> MockRepository {
        let mut repo = MockRepository::new();
        repo.expect_begin()
            .returning(|| Box::pin(async { Ok(Box::new(MockTransaction) as Box<dyn Transaction>) }));
        repo.expect_begin_read_only()
            .returning(|| Box::pin(async { Ok(Box::new(MockTransaction) as Box<dyn Transaction>) }));
        repo
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
                .repository(Arc::new(make_mock_repo()))
                .session_repository(Arc::new(MockSessionRepository::new()))
                .user_repository(Arc::new(MockUserRepository::new()))
                .user_setting_repository(Arc::new(MockUserSettingRepository::new()))
                .author_repository(Arc::new(author_repo))
                .series_repository(Arc::new(MockSeriesRepository::new()))
                .publisher_repository(Arc::new(MockPublisherRepository::new()))
                .genre_repository(Arc::new(MockGenreRepository::new()))
                .tag_repository(Arc::new(MockTagRepository::new()))
                .book_repository(Arc::new(book_repo))
                .import_job_repository(Arc::new(job_repo))
                .job_repository(Arc::new(MockJobRepository::new()))
                .shelf_repository(Arc::new(MockShelfRepository::new()))
                .user_book_metadata_repository(Arc::new(MockUserBookMetadataRepository::new()))
                .device_repository(Arc::new(MockDeviceRepository::new()))
                .build()
                .expect("all fields provided"),
        );
        LibraryServiceImpl::new(repository_service, Arc::new(library_store))
    }

    fn fake_book_with_id(id: BookId) -> crate::book::Book {
        crate::book::Book::fake(id, "Test Book", BookStatus::Available)
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
            file_format: crate::book::FileFormat::Epub,
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
        let mut book_repo = MockBookRepository::new();
        book_repo.expect_count_available_books().returning(|_| Box::pin(async { Ok(5) }));

        let mut author_repo = MockAuthorRepository::new();
        author_repo.expect_count_authors().returning(|_| Box::pin(async { Ok(3) }));

        let svc = create_service(book_repo, author_repo, MockImportJobRepository::new(), MockLibraryStore::new());

        let stats = svc.library_stats().await.unwrap();

        assert_eq!(stats.books, 5);
        assert_eq!(stats.authors, 3);
    }

    #[tokio::test]
    async fn library_stats_returns_zeroes_when_empty() {
        let mut book_repo = MockBookRepository::new();
        book_repo.expect_count_available_books().returning(|_| Box::pin(async { Ok(0) }));

        let mut author_repo = MockAuthorRepository::new();
        author_repo.expect_count_authors().returning(|_| Box::pin(async { Ok(0) }));

        let svc = create_service(book_repo, author_repo, MockImportJobRepository::new(), MockLibraryStore::new());

        let stats = svc.library_stats().await.unwrap();

        assert_eq!(stats.books, 0);
        assert_eq!(stats.authors, 0);
    }

    // ─── delete_book ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn delete_book_returns_not_found_when_book_missing() {
        let mut book_repo = MockBookRepository::new();
        book_repo.expect_find_by_token().returning(|_, _| Box::pin(async { Ok(None) }));

        let svc = create_service(book_repo, MockAuthorRepository::new(), MockImportJobRepository::new(), MockLibraryStore::new());
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
        let book = fake_book_with_id(book_id);

        let mut book_repo = MockBookRepository::new();
        book_repo.expect_find_by_token().returning(move |_, _| {
            let b = book.clone();
            Box::pin(async move { Ok(Some(b)) })
        });
        book_repo.expect_authors_for_book().returning(|_, _| Box::pin(async { Ok(vec![]) }));
        book_repo.expect_files_for_book().returning(|_, _| Box::pin(async { Ok(vec![]) }));
        book_repo.expect_delete_book_authors().returning(|_, _| Box::pin(async { Ok(()) }));
        book_repo.expect_delete_book_identifiers().returning(|_, _| Box::pin(async { Ok(()) }));
        book_repo.expect_delete_book().returning(|_, _| Box::pin(async { Ok(()) }));

        let mut job_repo = MockImportJobRepository::new();
        job_repo.expect_find_by_candidate_book_id().returning(|_, _| Box::pin(async { Ok(None) }));

        let mut store = MockLibraryStore::new();
        store.expect_delete_book().returning(|_| Box::pin(async { Ok(()) }));

        let svc = create_service(book_repo, MockAuthorRepository::new(), job_repo, store);
        let token = BookToken::new(book_id);

        svc.delete_book(&token).await.unwrap();
    }

    #[tokio::test]
    async fn delete_book_removes_orphan_author() {
        let book_id: BookId = 1;
        let author_id: AuthorId = 42;
        let book = fake_book_with_id(book_id);
        let link = fake_book_author_link(book_id, author_id);

        let mut book_repo = MockBookRepository::new();
        book_repo.expect_find_by_token().returning(move |_, _| {
            let b = book.clone();
            Box::pin(async move { Ok(Some(b)) })
        });
        book_repo.expect_authors_for_book().returning(move |_, _| {
            let l = link.clone();
            Box::pin(async move { Ok(vec![l]) })
        });
        book_repo.expect_files_for_book().returning(|_, _| Box::pin(async { Ok(vec![]) }));
        book_repo.expect_delete_book_authors().returning(|_, _| Box::pin(async { Ok(()) }));
        book_repo.expect_delete_book_identifiers().returning(|_, _| Box::pin(async { Ok(()) }));
        book_repo.expect_delete_book().returning(|_, _| Box::pin(async { Ok(()) }));
        book_repo.expect_count_books_for_author().returning(|_, _| Box::pin(async { Ok(0) })); // no remaining books → orphan

        let mut author_repo = MockAuthorRepository::new();
        author_repo
            .expect_delete_author()
            .withf(move |_, id| *id == author_id)
            .times(1)
            .returning(|_, _| Box::pin(async { Ok(()) }));

        let mut job_repo = MockImportJobRepository::new();
        job_repo.expect_find_by_candidate_book_id().returning(|_, _| Box::pin(async { Ok(None) }));

        let mut store = MockLibraryStore::new();
        store.expect_delete_book().returning(|_| Box::pin(async { Ok(()) }));

        let svc = create_service(book_repo, author_repo, job_repo, store);
        let token = BookToken::new(book_id);

        svc.delete_book(&token).await.unwrap();
        // `.times(1)` on `delete_author` verifies the orphan was deleted when
        // the mock is dropped
    }

    #[tokio::test]
    async fn delete_book_preserves_author_with_other_books() {
        let book_id: BookId = 1;
        let author_id: AuthorId = 42;
        let book = fake_book_with_id(book_id);
        let link = fake_book_author_link(book_id, author_id);

        let mut book_repo = MockBookRepository::new();
        book_repo.expect_find_by_token().returning(move |_, _| {
            let b = book.clone();
            Box::pin(async move { Ok(Some(b)) })
        });
        book_repo.expect_authors_for_book().returning(move |_, _| {
            let l = link.clone();
            Box::pin(async move { Ok(vec![l]) })
        });
        book_repo.expect_files_for_book().returning(|_, _| Box::pin(async { Ok(vec![]) }));
        book_repo.expect_delete_book_authors().returning(|_, _| Box::pin(async { Ok(()) }));
        book_repo.expect_delete_book_identifiers().returning(|_, _| Box::pin(async { Ok(()) }));
        book_repo.expect_delete_book().returning(|_, _| Box::pin(async { Ok(()) }));
        book_repo.expect_count_books_for_author().returning(|_, _| Box::pin(async { Ok(1) })); // still has 1 other book → not an orphan

        // No expectation set on delete_author — mockall will panic if it is called
        let author_repo = MockAuthorRepository::new();

        let mut job_repo = MockImportJobRepository::new();
        job_repo.expect_find_by_candidate_book_id().returning(|_, _| Box::pin(async { Ok(None) }));

        let mut store = MockLibraryStore::new();
        store.expect_delete_book().returning(|_| Box::pin(async { Ok(()) }));

        let svc = create_service(book_repo, author_repo, job_repo, store);
        let token = BookToken::new(book_id);

        svc.delete_book(&token).await.unwrap();
    }

    #[tokio::test]
    async fn delete_book_removes_linked_import_job() {
        let book_id: BookId = 1;
        let job_id: ImportJobId = 99;
        let book = fake_book_with_id(book_id);
        let job = fake_import_job(job_id, book_id);

        let mut book_repo = MockBookRepository::new();
        book_repo.expect_find_by_token().returning(move |_, _| {
            let b = book.clone();
            Box::pin(async move { Ok(Some(b)) })
        });
        book_repo.expect_authors_for_book().returning(|_, _| Box::pin(async { Ok(vec![]) }));
        book_repo.expect_files_for_book().returning(|_, _| Box::pin(async { Ok(vec![]) }));
        book_repo.expect_delete_book_authors().returning(|_, _| Box::pin(async { Ok(()) }));
        book_repo.expect_delete_book_identifiers().returning(|_, _| Box::pin(async { Ok(()) }));
        book_repo.expect_delete_book().returning(|_, _| Box::pin(async { Ok(()) }));

        let mut job_repo = MockImportJobRepository::new();
        job_repo.expect_find_by_candidate_book_id().returning(move |_, _| {
            let j = job.clone();
            Box::pin(async move { Ok(Some(j)) })
        });
        job_repo
            .expect_delete_job()
            .withf(move |_, id| *id == job_id)
            .times(1)
            .returning(|_, _| Box::pin(async { Ok(()) }));

        let mut store = MockLibraryStore::new();
        store.expect_delete_book().returning(|_| Box::pin(async { Ok(()) }));

        let svc = create_service(book_repo, MockAuthorRepository::new(), job_repo, store);
        let token = BookToken::new(book_id);

        svc.delete_book(&token).await.unwrap();
        // `.times(1)` on `delete_job` verifies the linked job was deleted when
        // the mock is dropped
    }

    #[tokio::test]
    async fn delete_book_deletes_original_files_from_store() {
        let book_id: BookId = 1;
        let book = fake_book_with_id(book_id);
        let mut file = BookFile::fake(book_id, "epub");
        file.path = "Originals/test.epub".to_owned();

        let mut book_repo = MockBookRepository::new();
        book_repo.expect_find_by_token().returning(move |_, _| {
            let b = book.clone();
            Box::pin(async move { Ok(Some(b)) })
        });
        book_repo.expect_authors_for_book().returning(|_, _| Box::pin(async { Ok(vec![]) }));
        book_repo.expect_files_for_book().returning(move |_, _| {
            let f = file.clone();
            Box::pin(async move { Ok(vec![f]) })
        });
        book_repo.expect_delete_book_authors().returning(|_, _| Box::pin(async { Ok(()) }));
        book_repo.expect_delete_book_identifiers().returning(|_, _| Box::pin(async { Ok(()) }));
        book_repo.expect_delete_book().returning(|_, _| Box::pin(async { Ok(()) }));

        let mut job_repo = MockImportJobRepository::new();
        job_repo.expect_find_by_candidate_book_id().returning(|_, _| Box::pin(async { Ok(None) }));

        let mut store = MockLibraryStore::new();
        store.expect_delete_book().returning(|_| Box::pin(async { Ok(()) }));
        store
            .expect_delete_original_file()
            .withf(|path| path == "Originals/test.epub")
            .times(1)
            .returning(|_| Box::pin(async { Ok(()) }));

        let svc = create_service(book_repo, MockAuthorRepository::new(), job_repo, store);
        let token = BookToken::new(book_id);

        svc.delete_book(&token).await.unwrap();
        // `.times(1)` on `delete_original_file` verifies the file was removed
        // from the store
    }
}
