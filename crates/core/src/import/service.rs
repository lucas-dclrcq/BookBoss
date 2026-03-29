use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::{
    Error,
    book::FileFormat,
    import::{ImportJob, ImportJobId, ImportJobToken, ImportStatus, NewImportJob, ProcessImportPayload, scanner::ScanTrigger},
    jobs::JobRepositoryExt,
    repository::{RepositoryService, read_only_transaction, transaction},
    user::UserId,
    with_read_only_transaction, with_transaction,
};

/// Describes the outcome of [`ImportJobService::queue_file_if_new`].
#[derive(Debug, PartialEq)]
pub enum FileQueueStatus {
    /// The file was new; an import job has been created and enqueued.
    Queued,
    /// The file hash matches a record already in `book_files`.
    DuplicateLibraryFile { title: String, author: String },
    /// The file hash matches a record already in `import_jobs`.
    DuplicateIncomingQueue,
}

#[async_trait::async_trait]
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
#[allow(unused_lifetimes, reason = "async_trait + mockall expansion emits a spurious 'life0 parameter")]
pub trait ImportJobService: Send + Sync {
    async fn list_pending(&self, start_id: Option<ImportJobId>, page_size: Option<u64>) -> Result<Vec<ImportJob>, Error>;
    async fn list_needs_review(&self, start_id: Option<ImportJobId>, page_size: Option<u64>) -> Result<Vec<ImportJob>, Error>;
    async fn find_by_token(&self, token: ImportJobToken) -> Result<Option<ImportJob>, Error>;
    async fn find_by_id(&self, id: ImportJobId) -> Result<Option<ImportJob>, Error>;
    async fn approve_job(&self, job: ImportJob, reviewer_id: UserId) -> Result<ImportJob, Error>;
    async fn reject_job(&self, job: ImportJob, reviewer_id: UserId) -> Result<ImportJob, Error>;
    /// Atomically: check for an existing job with this hash (skip if found),
    /// create a new `ImportJob`, and enqueue a `ProcessImportPayload`
    /// background task — all within one database transaction.
    async fn queue_file_if_new(
        &self,
        file_path: String,
        file_hash: String,
        file_format: FileFormat,
        detected_at: DateTime<Utc>,
    ) -> Result<FileQueueStatus, Error>;
    /// On startup crash-recovery: resets any `Extracting`/`Identifying` jobs
    /// back to `Pending`, then re-enqueues every `Pending` job so none are
    /// lost if the queue lost its entries.
    async fn recover_on_startup(&self) -> Result<(), Error>;

    /// Triggers an on-demand bookdrop scan. Non-blocking: if a scan is already
    /// queued, the call is silently dropped.
    fn trigger_scan(&self);
}

pub(crate) struct ImportJobServiceImpl {
    repository_service: Arc<RepositoryService>,
    scan_trigger: Option<ScanTrigger>,
}

impl ImportJobServiceImpl {
    pub(super) fn new(repository_service: Arc<RepositoryService>, scan_trigger: Option<ScanTrigger>) -> Self {
        Self {
            repository_service,
            scan_trigger,
        }
    }
}

/// Creates an [`ImportJobService`] with no bookdrop scan wiring.
///
/// Use [`create_bookdrop_scan_subsystem`](super::scanner::create_bookdrop_scan_subsystem)
/// when bookdrop scanning is required.
#[must_use]
pub(crate) fn create_import_job_service(repository_service: Arc<RepositoryService>) -> Arc<dyn ImportJobService> {
    Arc::new(ImportJobServiceImpl::new(repository_service, None))
}

#[async_trait::async_trait]
impl ImportJobService for ImportJobServiceImpl {
    async fn list_pending(&self, start_id: Option<ImportJobId>, page_size: Option<u64>) -> Result<Vec<ImportJob>, Error> {
        with_read_only_transaction!(self, import_job_repository, |tx| {
            import_job_repository.list_by_status(tx, ImportStatus::Pending, start_id, page_size).await
        })
    }

    async fn list_needs_review(&self, start_id: Option<ImportJobId>, page_size: Option<u64>) -> Result<Vec<ImportJob>, Error> {
        with_read_only_transaction!(self, import_job_repository, |tx| {
            import_job_repository.list_by_status(tx, ImportStatus::NeedsReview, start_id, page_size).await
        })
    }

    async fn find_by_token(&self, token: ImportJobToken) -> Result<Option<ImportJob>, Error> {
        with_read_only_transaction!(self, import_job_repository, |tx| import_job_repository.find_by_token(tx, token).await)
    }

    async fn find_by_id(&self, id: ImportJobId) -> Result<Option<ImportJob>, Error> {
        with_read_only_transaction!(self, import_job_repository, |tx| import_job_repository.find_by_id(tx, id).await)
    }

    async fn approve_job(&self, job: ImportJob, reviewer_id: UserId) -> Result<ImportJob, Error> {
        if job.status != ImportStatus::NeedsReview {
            return Err(Error::Validation(format!("cannot approve job with status {:?}", job.status)));
        }
        let approved = ImportJob {
            status: ImportStatus::Approved,
            reviewed_by: Some(reviewer_id),
            reviewed_at: Some(Utc::now()),
            ..job
        };
        with_transaction!(self, import_job_repository, |tx| import_job_repository.update_job(tx, approved).await)
    }

    async fn reject_job(&self, job: ImportJob, reviewer_id: UserId) -> Result<ImportJob, Error> {
        if job.status != ImportStatus::NeedsReview {
            return Err(Error::Validation(format!("cannot reject job with status {:?}", job.status)));
        }
        let rejected = ImportJob {
            status: ImportStatus::Rejected,
            reviewed_by: Some(reviewer_id),
            reviewed_at: Some(Utc::now()),
            ..job
        };
        with_transaction!(self, import_job_repository, |tx| import_job_repository.update_job(tx, rejected).await)
    }

    async fn queue_file_if_new(
        &self,
        file_path: String,
        file_hash: String,
        file_format: FileFormat,
        detected_at: DateTime<Utc>,
    ) -> Result<FileQueueStatus, Error> {
        with_transaction!(self, import_job_repository, book_repository, author_repository, job_repository, |tx| {
            if let Some(book_file) = book_repository.find_file_by_hash(tx, &file_hash).await? {
                tracing::debug!(hash = %file_hash, "file already in book_files — skipping");
                let book = book_repository.find_by_id(tx, book_file.book_id).await?.map(|b| b.title).unwrap_or_default();
                let book_authors = book_repository.authors_for_book(tx, book_file.book_id).await?;
                let author = if let Some(ba) = book_authors.first() {
                    author_repository.find_by_id(tx, ba.author_id).await?.map(|a| a.name).unwrap_or_default()
                } else {
                    String::new()
                };
                return Ok(FileQueueStatus::DuplicateLibraryFile { title: book, author });
            }
            if import_job_repository.find_by_hash(tx, &file_hash).await?.is_some() {
                tracing::debug!(hash = %file_hash, "file already in import_jobs — skipping");
                return Ok(FileQueueStatus::DuplicateIncomingQueue);
            }
            let job = import_job_repository
                .add_job(
                    tx,
                    NewImportJob {
                        file_path,
                        file_hash,
                        file_format,
                        detected_at,
                    },
                )
                .await?;
            job_repository.enqueue(tx, &ProcessImportPayload { import_job_id: job.id }).await?;
            tracing::info!(token = %job.token, "queued import job");
            Ok(FileQueueStatus::Queued)
        })
    }

    fn trigger_scan(&self) {
        if let Some(trigger) = &self.scan_trigger {
            trigger.trigger();
        } else {
            tracing::debug!("trigger_scan called but scan channel not wired");
        }
    }

    async fn recover_on_startup(&self) -> Result<(), Error> {
        // Reset any import jobs left in Extracting/Identifying state from a previous
        // crash.
        let reset = with_transaction!(self, import_job_repository, |tx| {
            import_job_repository.reset_in_progress_to_pending(tx).await
        })?;
        if reset > 0 {
            tracing::warn!(count = reset, "reset in-progress import jobs to pending after startup");
        }

        // Re-enqueue all Pending jobs. Covers both the jobs reset above and any
        // that lost their queue entry (e.g. exhausted retries, manual cleanup).
        let mut enqueued: u64 = 0;
        let mut next_id = None;
        loop {
            let import_job_repo = self.repository_service.import_job_repository().clone();
            let ni = next_id;
            let batch = read_only_transaction(&**self.repository_service.repository(), |tx| {
                let import_job_repo = import_job_repo.clone();
                Box::pin(async move { import_job_repo.list_by_status(tx, ImportStatus::Pending, ni, None).await })
            })
            .await?;

            if batch.is_empty() {
                break;
            }

            let exhausted = batch.len() < 50;
            next_id = batch.last().map(|j| j.id + 1);
            let ids: Vec<ImportJobId> = batch.iter().map(|j| j.id).collect();

            let job_repo = self.repository_service.job_repository().clone();
            transaction(&**self.repository_service.repository(), |tx| {
                let job_repo = job_repo.clone();
                let ids = ids.clone();
                Box::pin(async move {
                    for import_job_id in ids {
                        job_repo.enqueue(tx, &ProcessImportPayload { import_job_id }).await?;
                    }
                    Ok(())
                })
            })
            .await?;

            enqueued += ids.len() as u64;
            if exhausted {
                break;
            }
        }

        if enqueued > 0 {
            tracing::info!(count = enqueued, "re-enqueued pending import jobs on startup");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::Utc;

    use super::{FileQueueStatus, ImportJobService, ImportJobServiceImpl};
    use crate::{
        Error, RepositoryError,
        book::repository::{author::MockAuthorRepository, book::MockBookRepository},
        import::{ImportJob, ImportJobId, ImportJobToken, ImportStatus, repository::import_job::MockImportJobRepository},
        jobs::repository::MockJobRepository,
    };

    // ─── Helpers ──────────────────────────────────────────────────────────────

    fn create_service(mock: MockImportJobRepository) -> ImportJobServiceImpl {
        let repository_service = Arc::new(
            crate::repository::testing::default_repository_service_builder()
                .import_job_repository(Arc::new(mock))
                .build()
                .expect("all fields provided"),
        );
        ImportJobServiceImpl::new(repository_service, None)
    }

    fn create_service_with_all_repos(import_mock: MockImportJobRepository, book_mock: MockBookRepository, job_mock: MockJobRepository) -> ImportJobServiceImpl {
        let repository_service = Arc::new(
            crate::repository::testing::default_repository_service_builder()
                .import_job_repository(Arc::new(import_mock))
                .book_repository(Arc::new(book_mock))
                .job_repository(Arc::new(job_mock))
                .build()
                .expect("all fields provided"),
        );
        ImportJobServiceImpl::new(repository_service, None)
    }

    fn create_service_with_author_repos(
        import_mock: MockImportJobRepository,
        book_mock: MockBookRepository,
        author_mock: MockAuthorRepository,
        job_mock: MockJobRepository,
    ) -> ImportJobServiceImpl {
        let repository_service = Arc::new(
            crate::repository::testing::default_repository_service_builder()
                .import_job_repository(Arc::new(import_mock))
                .book_repository(Arc::new(book_mock))
                .author_repository(Arc::new(author_mock))
                .job_repository(Arc::new(job_mock))
                .build()
                .expect("all fields provided"),
        );
        ImportJobServiceImpl::new(repository_service, None)
    }

    fn fake_job(status: ImportStatus) -> ImportJob {
        let id: ImportJobId = 1;
        ImportJob {
            id,
            version: 1,
            token: ImportJobToken::new(id),
            file_path: "/watch/test.epub".to_owned(),
            file_hash: "abc123".to_owned(),
            file_format: crate::book::FileFormat::Epub,
            detected_at: Utc::now(),
            status,
            candidate_book_id: None,
            metadata_source: None,
            error_message: None,
            reviewed_by: None,
            reviewed_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    // ─── list_pending ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_list_pending_returns_jobs() {
        let jobs = vec![fake_job(ImportStatus::Pending)];
        let mut mock = MockImportJobRepository::new();
        mock.expect_list_by_status().returning(move |_, _, _, _| {
            let jobs = jobs.clone();
            Box::pin(async move { Ok(jobs) })
        });
        let svc = create_service(mock);

        let result = svc.list_pending(None, None).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_list_pending_returns_empty() {
        let mut mock = MockImportJobRepository::new();
        mock.expect_list_by_status().returning(|_, _, _, _| Box::pin(async { Ok(vec![]) }));
        let svc = create_service(mock);

        let result = svc.list_pending(None, None).await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_list_pending_propagates_error() {
        let mut mock = MockImportJobRepository::new();
        mock.expect_list_by_status()
            .returning(|_, _, _, _| Box::pin(async { Err(Error::RepositoryError(RepositoryError::Database("db error".into()))) }));
        let svc = create_service(mock);

        let result = svc.list_pending(None, None).await;

        assert!(matches!(result, Err(Error::RepositoryError(RepositoryError::Database(_)))));
    }

    // ─── find_by_token ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_find_by_token_found() {
        let job = fake_job(ImportStatus::Pending);
        let token = job.token;
        let mut mock = MockImportJobRepository::new();
        mock.expect_find_by_token().returning(move |_, _| {
            let job = job.clone();
            Box::pin(async move { Ok(Some(job)) })
        });
        let svc = create_service(mock);

        let result = svc.find_by_token(token).await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_find_by_token_not_found() {
        let token = ImportJobToken::new(99);
        let mut mock = MockImportJobRepository::new();
        mock.expect_find_by_token().returning(|_, _| Box::pin(async { Ok(None) }));
        let svc = create_service(mock);

        let result = svc.find_by_token(token).await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_find_by_token_propagates_error() {
        let token = ImportJobToken::new(1);
        let mut mock = MockImportJobRepository::new();
        mock.expect_find_by_token()
            .returning(|_, _| Box::pin(async { Err(Error::RepositoryError(RepositoryError::Database("db error".into()))) }));
        let svc = create_service(mock);

        let result = svc.find_by_token(token).await;

        assert!(matches!(result, Err(Error::RepositoryError(RepositoryError::Database(_)))));
    }

    // ─── find_by_id ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_find_by_id_found() {
        let job = fake_job(ImportStatus::Pending);
        let id = job.id;
        let mut mock = MockImportJobRepository::new();
        mock.expect_find_by_id().returning(move |_, _| {
            let job = job.clone();
            Box::pin(async move { Ok(Some(job)) })
        });
        let svc = create_service(mock);

        let result = svc.find_by_id(id).await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_find_by_id_not_found() {
        let mut mock = MockImportJobRepository::new();
        mock.expect_find_by_id().returning(|_, _| Box::pin(async { Ok(None) }));
        let svc = create_service(mock);

        let result = svc.find_by_id(999).await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    // ─── queue_file_if_new ────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_queue_file_if_new_skips_existing_hash() {
        let existing = fake_job(ImportStatus::Pending);
        let mut import_mock = MockImportJobRepository::new();
        import_mock.expect_find_by_hash().returning(move |_, _| {
            let j = existing.clone();
            Box::pin(async move { Ok(Some(j)) })
        });
        let mut book_mock = MockBookRepository::new();
        book_mock.expect_find_file_by_hash().returning(|_, _| Box::pin(async { Ok(None) }));
        let job_mock = MockJobRepository::new(); // enqueue must NOT be called
        let svc = create_service_with_all_repos(import_mock, book_mock, job_mock);

        let result = svc
            .queue_file_if_new("/watch/test.epub".into(), "abc123".into(), crate::book::FileFormat::Epub, Utc::now())
            .await;

        result.unwrap();
    }

    #[tokio::test]
    async fn test_queue_file_if_new_skips_existing_book_file_hash() {
        let mut import_mock = MockImportJobRepository::new();
        import_mock.expect_find_by_hash().returning(|_, _| Box::pin(async { Ok(None) }));
        let mut book_mock = MockBookRepository::new();
        book_mock
            .expect_find_file_by_hash()
            .returning(|_, _| Box::pin(async { Ok(Some(crate::book::model::book_file::BookFile::fake(1, "epub"))) }));
        book_mock.expect_find_by_id().returning(|_, _| {
            Box::pin(async {
                Ok(Some(crate::book::model::book::Book::fake(
                    1,
                    "Test Book",
                    crate::book::model::book::BookStatus::Available,
                )))
            })
        });
        book_mock.expect_authors_for_book().returning(|_, _| Box::pin(async { Ok(vec![]) }));
        let author_mock = MockAuthorRepository::new();
        let job_mock = MockJobRepository::new(); // enqueue must NOT be called
        let svc = create_service_with_author_repos(import_mock, book_mock, author_mock, job_mock);

        let result = svc
            .queue_file_if_new("/watch/test.epub".into(), "abc123".into(), crate::book::FileFormat::Epub, Utc::now())
            .await;

        result.unwrap();
    }

    #[tokio::test]
    async fn test_queue_file_if_new_creates_and_enqueues() {
        let job = fake_job(ImportStatus::Pending);
        let mut import_mock = MockImportJobRepository::new();
        import_mock.expect_find_by_hash().returning(|_, _| Box::pin(async { Ok(None) }));
        let mut book_mock = MockBookRepository::new();
        book_mock.expect_find_file_by_hash().returning(|_, _| Box::pin(async { Ok(None) }));
        import_mock.expect_add_job().returning(move |_, _| {
            let j = job.clone();
            Box::pin(async move { Ok(j) })
        });
        let mut job_mock = MockJobRepository::new();
        job_mock.expect_enqueue_raw().returning(|_, _, _, _| {
            Box::pin(async {
                Ok(crate::jobs::model::Job {
                    id: 1,
                    job_type: "process_import".into(),
                    payload: serde_json::Value::Null,
                    status: crate::jobs::JobStatus::Pending,
                    priority: 1,
                    attempt: 0,
                    max_attempts: 3,
                    version: 1,
                    scheduled_at: Utc::now(),
                    started_at: None,
                    completed_at: None,
                    error_message: None,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                })
            })
        });
        let svc = create_service_with_all_repos(import_mock, book_mock, job_mock);

        let result = svc
            .queue_file_if_new("/watch/new.epub".into(), "newHash".into(), crate::book::FileFormat::Epub, Utc::now())
            .await;

        result.unwrap();
    }

    #[tokio::test]
    async fn test_queue_file_if_new_returns_duplicate_library_file_status() {
        let mut import_mock = MockImportJobRepository::new();
        import_mock.expect_find_by_hash().returning(|_, _| Box::pin(async { Ok(None) }));
        let mut book_mock = MockBookRepository::new();
        book_mock
            .expect_find_file_by_hash()
            .returning(|_, _| Box::pin(async { Ok(Some(crate::book::model::book_file::BookFile::fake(1, "epub"))) }));
        book_mock.expect_find_by_id().returning(|_, _| {
            Box::pin(async {
                Ok(Some(crate::book::model::book::Book::fake(
                    1,
                    "Dune",
                    crate::book::model::book::BookStatus::Available,
                )))
            })
        });
        book_mock
            .expect_authors_for_book()
            .returning(|_, _| Box::pin(async { Ok(vec![crate::book::model::author::BookAuthor::fake(1, 42, "author", 0)]) }));
        let mut author_mock = MockAuthorRepository::new();
        author_mock
            .expect_find_by_id()
            .returning(|_, _| Box::pin(async { Ok(Some(crate::book::model::author::Author::fake(42, "Frank Herbert"))) }));
        let job_mock = MockJobRepository::new();
        let svc = create_service_with_author_repos(import_mock, book_mock, author_mock, job_mock);

        let result = svc
            .queue_file_if_new("/watch/dupe.epub".into(), "dupeHash".into(), crate::book::FileFormat::Epub, Utc::now())
            .await
            .unwrap();

        assert_eq!(
            result,
            FileQueueStatus::DuplicateLibraryFile {
                title: "Dune".into(),
                author: "Frank Herbert".into(),
            }
        );
    }

    #[tokio::test]
    async fn test_queue_file_if_new_returns_duplicate_incoming_queue_status() {
        let existing = fake_job(ImportStatus::Pending);
        let mut import_mock = MockImportJobRepository::new();
        import_mock.expect_find_by_hash().returning(move |_, _| {
            let j = existing.clone();
            Box::pin(async move { Ok(Some(j)) })
        });
        let mut book_mock = MockBookRepository::new();
        book_mock.expect_find_file_by_hash().returning(|_, _| Box::pin(async { Ok(None) }));
        let author_mock = MockAuthorRepository::new();
        let job_mock = MockJobRepository::new();
        let svc = create_service_with_author_repos(import_mock, book_mock, author_mock, job_mock);

        let result = svc
            .queue_file_if_new("/watch/dupe.epub".into(), "abc123".into(), crate::book::FileFormat::Epub, Utc::now())
            .await
            .unwrap();

        assert_eq!(result, FileQueueStatus::DuplicateIncomingQueue);
    }

    #[tokio::test]
    async fn test_queue_file_if_new_returns_queued_status() {
        let job = fake_job(ImportStatus::Pending);
        let mut import_mock = MockImportJobRepository::new();
        import_mock.expect_find_by_hash().returning(|_, _| Box::pin(async { Ok(None) }));
        let mut book_mock = MockBookRepository::new();
        book_mock.expect_find_file_by_hash().returning(|_, _| Box::pin(async { Ok(None) }));
        import_mock.expect_add_job().returning(move |_, _| {
            let j = job.clone();
            Box::pin(async move { Ok(j) })
        });
        let author_mock = MockAuthorRepository::new();
        let mut job_mock = MockJobRepository::new();
        job_mock.expect_enqueue_raw().returning(|_, _, _, _| {
            Box::pin(async {
                Ok(crate::jobs::model::Job {
                    id: 1,
                    job_type: "process_import".into(),
                    payload: serde_json::Value::Null,
                    status: crate::jobs::JobStatus::Pending,
                    priority: 1,
                    attempt: 0,
                    max_attempts: 3,
                    version: 1,
                    scheduled_at: Utc::now(),
                    started_at: None,
                    completed_at: None,
                    error_message: None,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                })
            })
        });
        let svc = create_service_with_author_repos(import_mock, book_mock, author_mock, job_mock);

        let result = svc
            .queue_file_if_new("/watch/new.epub".into(), "newHash".into(), crate::book::FileFormat::Epub, Utc::now())
            .await
            .unwrap();

        assert_eq!(result, FileQueueStatus::Queued);
    }

    // ─── approve_job ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_approve_job_success() {
        let job = fake_job(ImportStatus::NeedsReview);
        let approved = ImportJob {
            status: ImportStatus::Approved,
            ..job.clone()
        };
        let mut mock = MockImportJobRepository::new();
        mock.expect_update_job().returning(move |_, _| {
            let approved = approved.clone();
            Box::pin(async move { Ok(approved) })
        });
        let svc = create_service(mock);

        let result = svc.approve_job(job, 1).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().status, ImportStatus::Approved);
    }

    #[tokio::test]
    async fn test_approve_job_wrong_status_returns_validation_error() {
        let svc = create_service(MockImportJobRepository::new());

        for status in [ImportStatus::Pending, ImportStatus::Approved, ImportStatus::Rejected, ImportStatus::Error] {
            let label = format!("{status:?}");
            let result = svc.approve_job(fake_job(status), 1).await;
            assert!(matches!(result, Err(Error::Validation(_))), "expected Validation error for {label}");
        }
    }

    #[tokio::test]
    async fn test_approve_job_propagates_update_error() {
        let job = fake_job(ImportStatus::NeedsReview);
        let mut mock = MockImportJobRepository::new();
        mock.expect_update_job()
            .returning(|_, _| Box::pin(async { Err(Error::RepositoryError(RepositoryError::Conflict)) }));
        let svc = create_service(mock);

        let result = svc.approve_job(job, 1).await;

        assert!(matches!(result, Err(Error::RepositoryError(RepositoryError::Conflict))));
    }

    // ─── reject_job ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_reject_job_success() {
        let job = fake_job(ImportStatus::NeedsReview);
        let rejected = ImportJob {
            status: ImportStatus::Rejected,
            ..job.clone()
        };
        let mut mock = MockImportJobRepository::new();
        mock.expect_update_job().returning(move |_, _| {
            let rejected = rejected.clone();
            Box::pin(async move { Ok(rejected) })
        });
        let svc = create_service(mock);

        let result = svc.reject_job(job, 1).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().status, ImportStatus::Rejected);
    }

    #[tokio::test]
    async fn test_reject_job_wrong_status_returns_validation_error() {
        let svc = create_service(MockImportJobRepository::new());

        for status in [ImportStatus::Pending, ImportStatus::Approved, ImportStatus::Rejected, ImportStatus::Error] {
            let label = format!("{status:?}");
            let result = svc.reject_job(fake_job(status), 1).await;
            assert!(matches!(result, Err(Error::Validation(_))), "expected Validation error for {label}");
        }
    }

    #[tokio::test]
    async fn test_reject_job_propagates_update_error() {
        let job = fake_job(ImportStatus::NeedsReview);
        let mut mock = MockImportJobRepository::new();
        mock.expect_update_job()
            .returning(|_, _| Box::pin(async { Err(Error::RepositoryError(RepositoryError::Conflict)) }));
        let svc = create_service(mock);

        let result = svc.reject_job(job, 1).await;

        assert!(matches!(result, Err(Error::RepositoryError(RepositoryError::Conflict))));
    }
}
