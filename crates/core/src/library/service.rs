use std::sync::Arc;

use tracing::warn;

use crate::{
    Error, RepositoryError,
    book::{Book, BookToken, FileRole},
    filter::BookFilter,
    repository::{RepositoryService, read_only_transaction, transaction},
    storage::FileStoreService,
};

pub struct LibraryStats {
    pub books: u64,
    pub authors: u64,
}

#[async_trait::async_trait]
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
#[allow(unused_lifetimes, reason = "async_trait + mockall expansion emits a spurious 'life0 parameter")]
pub trait LibraryService: Send + Sync {
    /// Returns aggregate counts for the library.
    async fn library_stats(&self) -> Result<LibraryStats, Error>;

    /// Searches the library catalog using a [`BookFilter`].
    ///
    /// Only supports library-level (non-user-scoped) filter rules. Returns
    /// `Err` if the filter contains user-scoped rules such as `ReadStatus` —
    /// use a user-aware search method for those.
    async fn search_books(&self, filter: &BookFilter, offset: Option<u64>, page_size: Option<u64>) -> Result<Vec<Book>, Error>;

    /// Permanently deletes a book and its files from the library.
    ///
    /// Removes all DB records (book, authors/identifiers join rows, and orphan
    /// authors with no remaining books) then deletes the book directory from
    /// the library store.
    async fn delete_book(&self, book_token: BookToken) -> Result<(), Error>;
}

pub struct LibraryServiceImpl {
    repository_service: Arc<RepositoryService>,
    file_store: Arc<dyn FileStoreService>,
}

impl LibraryServiceImpl {
    pub(crate) fn new(repository_service: Arc<RepositoryService>, file_store: Arc<dyn FileStoreService>) -> Self {
        Self {
            repository_service,
            file_store,
        }
    }
}

#[async_trait::async_trait]
impl LibraryService for LibraryServiceImpl {
    async fn library_stats(&self) -> Result<LibraryStats, Error> {
        let library_repo = self.repository_service.library_repository().clone();
        read_only_transaction(&**self.repository_service.repository(), |tx| {
            Box::pin(async move {
                let books = library_repo.count_available_books(tx).await?;
                let authors = library_repo.count_authors(tx).await?;
                Ok(LibraryStats { books, authors })
            })
        })
        .await
    }

    async fn search_books(&self, filter: &BookFilter, offset: Option<u64>, page_size: Option<u64>) -> Result<Vec<Book>, Error> {
        if filter.contains_user_scoped_rules() {
            return Err(Error::Validation(
                "library search does not support user-scoped filter rules such as ReadStatus".to_string(),
            ));
        }
        let filter = filter.clone();
        let library_repo = self.repository_service.library_repository().clone();
        read_only_transaction(&**self.repository_service.repository(), |tx| {
            Box::pin(async move { library_repo.books_for_filter(tx, &filter, 0, offset, page_size, None).await })
        })
        .await
    }

    async fn delete_book(&self, book_token: BookToken) -> Result<(), Error> {
        let book_repo = self.repository_service.book_repository().clone();
        let author_repo = self.repository_service.author_repository().clone();
        let job_repo = self.repository_service.import_job_repository().clone();

        let (original_filenames, enriched_filename) = transaction(&**self.repository_service.repository(), |tx| {
            let br = book_repo.clone();
            let ar = author_repo.clone();
            let jr = job_repo.clone();
            Box::pin(async move {
                let book = br
                    .find_by_token(tx, book_token)
                    .await?
                    .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;

                let author_links = br.authors_for_book(tx, book.id).await?;
                let author_ids: Vec<u64> = author_links.iter().map(|a| a.author_id).collect();

                // Collect file paths before deleting records.
                let files = br.files_for_book(tx, book.id).await?;
                let original_filenames: Vec<String> = files.iter().filter(|f| f.file_role == FileRole::Original).map(|f| f.path.clone()).collect();
                let enriched_filename: Option<String> = files
                    .iter()
                    .find(|f| f.file_role == FileRole::Enriched)
                    .and_then(|f| std::path::Path::new(&f.path).file_name().and_then(|n| n.to_str()).map(String::from));

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

                Ok((original_filenames, enriched_filename))
            })
        })
        .await?;

        // Best-effort: copy the enriched file to Trash before deleting.
        if let Some(ref file_name) = enriched_filename {
            if let Err(e) = self.file_store.copy_to_trash(book_token, file_name).await {
                warn!(book_token = %book_token, file_name, error = %e, "failed to copy enriched file to Trash");
            }
        }

        self.file_store.delete_book(book_token).await?;
        for filename in original_filenames {
            self.file_store.delete_original_file(&filename).await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{LibraryService, LibraryServiceImpl};
    use crate::{
        Error, RepositoryError,
        book::{
            AuthorId, BookAuthor, BookFile, BookId, BookStatus, BookToken, FileRole,
            repository::{author::MockAuthorRepository, book::MockBookRepository},
        },
        import::{ImportJob, ImportJobId, ImportJobToken, ImportStatus, repository::import_job::MockImportJobRepository},
        library::MockLibraryRepository,
        storage::store::MockFileStoreService,
    };

    // ─── Service builder ──────────────────────────────────────────────────────

    fn create_service(
        book_repo: MockBookRepository,
        author_repo: MockAuthorRepository,
        job_repo: MockImportJobRepository,
        library_repo: MockLibraryRepository,
        file_store: MockFileStoreService,
    ) -> LibraryServiceImpl {
        let repository_service = Arc::new(
            crate::repository::testing::default_repository_service_builder()
                .author_repository(Arc::new(author_repo))
                .book_repository(Arc::new(book_repo))
                .import_job_repository(Arc::new(job_repo))
                .library_repository(Arc::new(library_repo))
                .build()
                .expect("all fields provided"),
        );
        LibraryServiceImpl::new(repository_service, Arc::new(file_store))
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
        let mut library_repo = MockLibraryRepository::new();
        library_repo.expect_count_available_books().returning(|_| Box::pin(async { Ok(5) }));
        library_repo.expect_count_authors().returning(|_| Box::pin(async { Ok(3) }));

        let svc = create_service(
            MockBookRepository::new(),
            MockAuthorRepository::new(),
            MockImportJobRepository::new(),
            library_repo,
            MockFileStoreService::new(),
        );

        let stats = svc.library_stats().await.unwrap();

        assert_eq!(stats.books, 5);
        assert_eq!(stats.authors, 3);
    }

    #[tokio::test]
    async fn library_stats_returns_zeroes_when_empty() {
        let mut library_repo = MockLibraryRepository::new();
        library_repo.expect_count_available_books().returning(|_| Box::pin(async { Ok(0) }));
        library_repo.expect_count_authors().returning(|_| Box::pin(async { Ok(0) }));

        let svc = create_service(
            MockBookRepository::new(),
            MockAuthorRepository::new(),
            MockImportJobRepository::new(),
            library_repo,
            MockFileStoreService::new(),
        );

        let stats = svc.library_stats().await.unwrap();

        assert_eq!(stats.books, 0);
        assert_eq!(stats.authors, 0);
    }

    // ─── delete_book ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn delete_book_returns_not_found_when_book_missing() {
        let mut book_repo = MockBookRepository::new();
        book_repo.expect_find_by_token().returning(|_, _| Box::pin(async { Ok(None) }));

        let svc = create_service(
            book_repo,
            MockAuthorRepository::new(),
            MockImportJobRepository::new(),
            MockLibraryRepository::new(),
            MockFileStoreService::new(),
        );
        let token = BookToken::new(99);

        let result = svc.delete_book(token).await;

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

        let mut store = MockFileStoreService::new();
        store.expect_delete_book().returning(|_| Box::pin(async { Ok(()) }));

        let svc = create_service(book_repo, MockAuthorRepository::new(), job_repo, MockLibraryRepository::new(), store);
        let token = BookToken::new(book_id);

        svc.delete_book(token).await.unwrap();
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

        let mut store = MockFileStoreService::new();
        store.expect_delete_book().returning(|_| Box::pin(async { Ok(()) }));

        let svc = create_service(book_repo, author_repo, job_repo, MockLibraryRepository::new(), store);
        let token = BookToken::new(book_id);

        svc.delete_book(token).await.unwrap();
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

        let mut store = MockFileStoreService::new();
        store.expect_delete_book().returning(|_| Box::pin(async { Ok(()) }));

        let svc = create_service(book_repo, author_repo, job_repo, MockLibraryRepository::new(), store);
        let token = BookToken::new(book_id);

        svc.delete_book(token).await.unwrap();
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

        let mut store = MockFileStoreService::new();
        store.expect_delete_book().returning(|_| Box::pin(async { Ok(()) }));

        let svc = create_service(book_repo, MockAuthorRepository::new(), job_repo, MockLibraryRepository::new(), store);
        let token = BookToken::new(book_id);

        svc.delete_book(token).await.unwrap();
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

        let mut store = MockFileStoreService::new();
        store.expect_delete_book().returning(|_| Box::pin(async { Ok(()) }));
        store
            .expect_delete_original_file()
            .withf(|path| path == "Originals/test.epub")
            .times(1)
            .returning(|_| Box::pin(async { Ok(()) }));

        let svc = create_service(book_repo, MockAuthorRepository::new(), job_repo, MockLibraryRepository::new(), store);
        let token = BookToken::new(book_id);

        svc.delete_book(token).await.unwrap();
        // `.times(1)` on `delete_original_file` verifies the file was removed
        // from the store
    }

    #[tokio::test]
    async fn delete_book_copies_enriched_file_to_trash() {
        let book_id: BookId = 1;
        let book = fake_book_with_id(book_id);

        let mut enriched = BookFile::fake(book_id, "epub");
        enriched.file_role = FileRole::Enriched;
        enriched.path = "BK_00001/my-book.epub".to_owned();

        let mut book_repo = MockBookRepository::new();
        book_repo.expect_find_by_token().returning(move |_, _| {
            let b = book.clone();
            Box::pin(async move { Ok(Some(b)) })
        });
        book_repo.expect_authors_for_book().returning(|_, _| Box::pin(async { Ok(vec![]) }));
        book_repo.expect_files_for_book().returning(move |_, _| {
            let f = enriched.clone();
            Box::pin(async move { Ok(vec![f]) })
        });
        book_repo.expect_delete_book_authors().returning(|_, _| Box::pin(async { Ok(()) }));
        book_repo.expect_delete_book_identifiers().returning(|_, _| Box::pin(async { Ok(()) }));
        book_repo.expect_delete_book().returning(|_, _| Box::pin(async { Ok(()) }));

        let mut job_repo = MockImportJobRepository::new();
        job_repo.expect_find_by_candidate_book_id().returning(|_, _| Box::pin(async { Ok(None) }));

        let mut store = MockFileStoreService::new();
        store
            .expect_copy_to_trash()
            .withf(|_, name| name == "my-book.epub")
            .times(1)
            .returning(|_, _| Box::pin(async { Ok(()) }));
        store.expect_delete_book().returning(|_| Box::pin(async { Ok(()) }));

        let svc = create_service(book_repo, MockAuthorRepository::new(), job_repo, MockLibraryRepository::new(), store);
        let token = BookToken::new(book_id);

        svc.delete_book(token).await.unwrap();
        // `.times(1)` on `copy_to_trash` verifies it was called
    }

    #[tokio::test]
    async fn delete_book_skips_trash_when_no_enriched_file() {
        let book_id: BookId = 1;
        let book = fake_book_with_id(book_id);

        let mut original = BookFile::fake(book_id, "epub");
        original.path = "Originals/test.epub".to_owned();

        let mut book_repo = MockBookRepository::new();
        book_repo.expect_find_by_token().returning(move |_, _| {
            let b = book.clone();
            Box::pin(async move { Ok(Some(b)) })
        });
        book_repo.expect_authors_for_book().returning(|_, _| Box::pin(async { Ok(vec![]) }));
        book_repo.expect_files_for_book().returning(move |_, _| {
            let f = original.clone();
            Box::pin(async move { Ok(vec![f]) })
        });
        book_repo.expect_delete_book_authors().returning(|_, _| Box::pin(async { Ok(()) }));
        book_repo.expect_delete_book_identifiers().returning(|_, _| Box::pin(async { Ok(()) }));
        book_repo.expect_delete_book().returning(|_, _| Box::pin(async { Ok(()) }));

        let mut job_repo = MockImportJobRepository::new();
        job_repo.expect_find_by_candidate_book_id().returning(|_, _| Box::pin(async { Ok(None) }));

        let mut store = MockFileStoreService::new();
        // No expectation on copy_to_trash — mockall panics if it is called
        store.expect_delete_book().returning(|_| Box::pin(async { Ok(()) }));
        store.expect_delete_original_file().returning(|_| Box::pin(async { Ok(()) }));

        let svc = create_service(book_repo, MockAuthorRepository::new(), job_repo, MockLibraryRepository::new(), store);
        let token = BookToken::new(book_id);

        svc.delete_book(token).await.unwrap();
    }

    #[tokio::test]
    async fn delete_book_succeeds_when_trash_copy_fails() {
        let book_id: BookId = 1;
        let book = fake_book_with_id(book_id);

        let mut enriched = BookFile::fake(book_id, "epub");
        enriched.file_role = FileRole::Enriched;
        enriched.path = "BK_00001/my-book.epub".to_owned();

        let mut book_repo = MockBookRepository::new();
        book_repo.expect_find_by_token().returning(move |_, _| {
            let b = book.clone();
            Box::pin(async move { Ok(Some(b)) })
        });
        book_repo.expect_authors_for_book().returning(|_, _| Box::pin(async { Ok(vec![]) }));
        book_repo.expect_files_for_book().returning(move |_, _| {
            let f = enriched.clone();
            Box::pin(async move { Ok(vec![f]) })
        });
        book_repo.expect_delete_book_authors().returning(|_, _| Box::pin(async { Ok(()) }));
        book_repo.expect_delete_book_identifiers().returning(|_, _| Box::pin(async { Ok(()) }));
        book_repo.expect_delete_book().returning(|_, _| Box::pin(async { Ok(()) }));

        let mut job_repo = MockImportJobRepository::new();
        job_repo.expect_find_by_candidate_book_id().returning(|_, _| Box::pin(async { Ok(None) }));

        let mut store = MockFileStoreService::new();
        store
            .expect_copy_to_trash()
            .returning(|_, _| Box::pin(async { Err(crate::Error::Infrastructure("disk full".to_owned())) }));
        store.expect_delete_book().returning(|_| Box::pin(async { Ok(()) }));

        let svc = create_service(book_repo, MockAuthorRepository::new(), job_repo, MockLibraryRepository::new(), store);
        let token = BookToken::new(book_id);

        // Should succeed despite trash copy failure
        svc.delete_book(token).await.unwrap();
    }
}
