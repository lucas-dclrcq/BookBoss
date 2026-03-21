use std::sync::Arc;

use serde::Serialize;

use crate::{
    Error,
    jobs::Enqueueable,
    repository::{RepositoryService, read_only_transaction, transaction},
};

/// Service port for enqueuing and counting background jobs.
///
/// Abstracts transaction management away from adapter crates — callers receive
/// an `Arc<dyn JobService>` and use [`JobServiceExt::enqueue`] without needing
/// to manage their own `Repository` or `Transaction` references.
#[async_trait::async_trait]
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
pub trait JobService: Send + Sync {
    /// Enqueue a raw job by type string and pre-serialised JSON payload.
    ///
    /// Prefer [`JobServiceExt::enqueue`] for typed payloads.
    async fn enqueue_raw(&self, job_type: &str, payload: serde_json::Value, priority: i16) -> Result<(), Error>;

    /// Count jobs of the given type that are currently pending or running.
    async fn count_pending_by_type(&self, job_type: &str) -> Result<u64, Error>;
}

/// Extension methods on [`JobService`] for typed enqueueing.
///
/// Blanket-implemented for all `JobService` impls — no manual work per job
/// type. Mirrors the [`JobRepositoryExt`] pattern but at the service layer.
pub trait JobServiceExt: JobService {
    fn enqueue<P: Enqueueable + Serialize + Send + Sync>(&self, payload: &P) -> impl std::future::Future<Output = Result<(), Error>> + Send {
        let value = serde_json::to_value(payload);
        async move {
            let value = value.map_err(|e| Error::Infrastructure(format!("failed to serialize job payload: {e}")))?;
            self.enqueue_raw(P::JOB_TYPE, value, P::DEFAULT_PRIORITY).await
        }
    }
}

impl<S: JobService + ?Sized> JobServiceExt for S {}

pub(crate) struct JobServiceImpl {
    repository_service: Arc<RepositoryService>,
}

impl JobServiceImpl {
    pub(crate) fn new(repository_service: Arc<RepositoryService>) -> Self {
        Self { repository_service }
    }
}

#[async_trait::async_trait]
impl JobService for JobServiceImpl {
    async fn enqueue_raw(&self, job_type: &str, payload: serde_json::Value, priority: i16) -> Result<(), Error> {
        let job_type = job_type.to_owned();
        let job_repo = self.repository_service.job_repository().clone();
        transaction(&**self.repository_service.repository(), |tx| {
            let job_repo = job_repo.clone();
            let job_type = job_type.clone();
            let payload = payload.clone();
            Box::pin(async move {
                job_repo.enqueue_raw(tx, &job_type, payload, priority).await?;
                Ok(())
            })
        })
        .await
    }

    async fn count_pending_by_type(&self, job_type: &str) -> Result<u64, Error> {
        let job_type = job_type.to_owned();
        let job_repo = self.repository_service.job_repository().clone();
        read_only_transaction(&**self.repository_service.repository(), |tx| {
            let job_repo = job_repo.clone();
            let job_type = job_type.clone();
            Box::pin(async move { job_repo.count_pending_by_type(tx, &job_type).await })
        })
        .await
    }
}

/// Creates a `JobService` backed by the given `RepositoryService`.
///
/// Called from the application wiring layer (e.g. `bookboss`) before
/// `CoreServices` is built, so that adapters like `ConversionServiceImpl` can
/// receive the service without circular dependency on `CoreServices`.
#[must_use]
pub fn create_job_service(repository_service: Arc<RepositoryService>) -> Arc<dyn JobService> {
    Arc::new(JobServiceImpl::new(repository_service))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::repository::MockJobRepository;

    fn create_service(mock: MockJobRepository) -> JobServiceImpl {
        let repository_service = Arc::new(
            crate::repository::testing::default_repository_service_builder()
                .job_repository(Arc::new(mock))
                .build()
                .expect("all fields provided"),
        );
        JobServiceImpl::new(repository_service)
    }

    #[tokio::test]
    async fn test_count_pending_by_type_delegates_to_repo() {
        let mut mock = MockJobRepository::new();
        mock.expect_count_pending_by_type().returning(|_, _| Box::pin(async { Ok(7) }));
        let svc = create_service(mock);

        let result = svc.count_pending_by_type("enrich_epub").await;

        assert_eq!(result.unwrap(), 7);
    }

    #[tokio::test]
    async fn test_count_pending_propagates_error() {
        let mut mock = MockJobRepository::new();
        mock.expect_count_pending_by_type()
            .returning(|_, _| Box::pin(async { Err(Error::RepositoryError(crate::RepositoryError::Database("db".into()))) }));
        let svc = create_service(mock);

        let result = svc.count_pending_by_type("enrich_epub").await;

        assert!(matches!(result, Err(Error::RepositoryError(_))));
    }
}
