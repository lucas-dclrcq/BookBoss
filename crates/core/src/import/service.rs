use std::sync::Arc;

use chrono::Utc;

use crate::{
    Error,
    import::{ImportJob, ImportJobId, ImportJobToken, ImportStatus},
    repository::RepositoryService,
    user::UserId,
    with_read_only_transaction, with_transaction,
};

#[async_trait::async_trait]
pub trait ImportJobService: Send + Sync {
    async fn list_pending(&self, start_id: Option<ImportJobId>, page_size: Option<u64>) -> Result<Vec<ImportJob>, Error>;
    async fn list_needs_review(&self, start_id: Option<ImportJobId>, page_size: Option<u64>) -> Result<Vec<ImportJob>, Error>;
    async fn find_by_token(&self, token: &ImportJobToken) -> Result<Option<ImportJob>, Error>;
    async fn approve_job(&self, job: ImportJob, reviewer_id: UserId) -> Result<ImportJob, Error>;
    async fn reject_job(&self, job: ImportJob, reviewer_id: UserId) -> Result<ImportJob, Error>;
}

pub(crate) struct ImportJobServiceImpl {
    repository_service: Arc<RepositoryService>,
}

impl ImportJobServiceImpl {
    pub(crate) fn new(repository_service: Arc<RepositoryService>) -> Self {
        Self { repository_service }
    }
}

#[async_trait::async_trait]
impl ImportJobService for ImportJobServiceImpl {
    #[tracing::instrument(level = "trace", skip(self))]
    async fn list_pending(&self, start_id: Option<ImportJobId>, page_size: Option<u64>) -> Result<Vec<ImportJob>, Error> {
        with_read_only_transaction!(self, import_job_repository, |tx| {
            import_job_repository.list_by_status(tx, ImportStatus::Pending, start_id, page_size).await
        })
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn list_needs_review(&self, start_id: Option<ImportJobId>, page_size: Option<u64>) -> Result<Vec<ImportJob>, Error> {
        with_read_only_transaction!(self, import_job_repository, |tx| {
            import_job_repository.list_by_status(tx, ImportStatus::NeedsReview, start_id, page_size).await
        })
    }

    #[tracing::instrument(level = "trace", skip(self, token))]
    async fn find_by_token(&self, token: &ImportJobToken) -> Result<Option<ImportJob>, Error> {
        let token = *token;
        with_read_only_transaction!(self, import_job_repository, |tx| import_job_repository.find_by_token(tx, &token).await)
    }

    #[tracing::instrument(level = "trace", skip(self, job), fields(jobToken = %job.token))]
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

    #[tracing::instrument(level = "trace", skip(self, job), fields(jobToken = %job.token))]
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
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::Utc;

    use super::{ImportJobService, ImportJobServiceImpl};
    use crate::{
        Error, RepositoryError,
        import::{ImportJob, ImportJobId, ImportJobToken, ImportStatus, repository::import_job::MockImportJobRepository},
    };

    // ─── Helper ───────────────────────────────────────────────────────────────

    fn create_service(mock: MockImportJobRepository) -> ImportJobServiceImpl {
        let repository_service = Arc::new(
            crate::repository::testing::default_repository_service_builder()
                .import_job_repository(Arc::new(mock))
                .build()
                .expect("all fields provided"),
        );
        ImportJobServiceImpl::new(repository_service)
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

        let result = svc.find_by_token(&token).await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_find_by_token_not_found() {
        let token = ImportJobToken::new(99);
        let mut mock = MockImportJobRepository::new();
        mock.expect_find_by_token().returning(|_, _| Box::pin(async { Ok(None) }));
        let svc = create_service(mock);

        let result = svc.find_by_token(&token).await;

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

        let result = svc.find_by_token(&token).await;

        assert!(matches!(result, Err(Error::RepositoryError(RepositoryError::Database(_)))));
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
