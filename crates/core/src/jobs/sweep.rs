use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{CoreServices, Error, book::BookId, jobs::PRIORITY_SWEEP, repository::read_only_transaction};

/// Shared payload shape for all cursor sweep jobs.
///
/// Every `BookIdSweep` implementation uses this as its `JobHandler::Payload`.
/// Each sweep type has its own `Enqueueable` with its own `JOB_TYPE`, so
/// different sweeps are independent queue entries despite sharing this shape.
///
/// `#[serde(default)]` lets the health-check trigger fire with `{}` (empty
/// JSON) — missing fields resolve via `Default`, giving `after_id = None`
/// (start from the beginning) and `batch_size = 100`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct BookSweepPayload {
    /// Resume from this book ID (exclusive). `None` means start from the
    /// beginning.
    pub after_id: Option<BookId>,
    /// Maximum number of books to process in this slice.
    pub batch_size: u64,
}

impl Default for BookSweepPayload {
    fn default() -> Self {
        Self {
            after_id: None,
            batch_size: 100,
        }
    }
}

/// Trait for cursor sweep jobs that iterate over Available books in bounded
/// batches.
///
/// Implement [`process_batch`] with the per-batch work. The shared runner
/// [`run_book_id_sweep`] handles fetching IDs, invoking `process_batch`, and
/// self-re-enqueueing a delayed continuation when the batch is full.
///
/// Override [`fetch_batch`] to restrict which books are included (e.g. only
/// books with a stale fingerprint).
#[async_trait::async_trait]
pub trait BookIdSweep: Send + Sync {
    /// The job type string registered for this sweep — used when re-enqueueing
    /// the continuation.
    fn job_type(&self) -> &'static str;

    /// Delay between batches. Gives higher-priority jobs a window to run before
    /// the next sweep slice is claimed. Default: 5 minutes.
    fn continuation_delay(&self) -> chrono::Duration {
        chrono::Duration::minutes(5)
    }

    /// Fetch the next batch of book IDs.
    ///
    /// Default: all Available books with `id > after_id`, ordered by id ASC,
    /// limited to `batch_size`. Override to restrict to a subset.
    async fn fetch_batch(&self, core: &CoreServices, after_id: Option<BookId>, batch_size: u64) -> Result<Vec<BookId>, Error> {
        let book_repo = core.repository_service.book_repository().clone();
        read_only_transaction(&**core.repository_service.repository(), |tx| {
            Box::pin(async move { book_repo.find_available_books_for_sweep(tx, after_id, batch_size).await })
        })
        .await
    }

    /// Perform the work for one batch of book IDs.
    async fn process_batch(&self, core: &Arc<CoreServices>, ids: Vec<BookId>) -> Result<(), Error>;
}

/// Execute one slice of a cursor sweep.
///
/// 1. Fetches up to `payload.batch_size` books via
///    [`BookIdSweep::fetch_batch`].
/// 2. Calls [`BookIdSweep::process_batch`] with the result.
/// 3. If the batch was full, self-re-enqueues the continuation at
///    `PRIORITY_SWEEP` with the configured delay and `after_id` set to the last
///    processed ID.
/// 4. If the batch was partial (end of library), exits — the sweep is complete.
pub async fn run_book_id_sweep<S: BookIdSweep>(sweep: &S, core: &Arc<CoreServices>, payload: BookSweepPayload) -> Result<(), Error> {
    let ids = sweep.fetch_batch(core, payload.after_id, payload.batch_size).await?;
    let batch_len = ids.len() as u64;

    sweep.process_batch(core, ids.clone()).await?;

    if batch_len == payload.batch_size {
        let last_id = *ids.last().expect("non-empty when batch_len == batch_size");
        let continuation = BookSweepPayload {
            after_id: Some(last_id),
            batch_size: payload.batch_size,
        };
        let value = serde_json::to_value(&continuation).map_err(|e| Error::Infrastructure(format!("sweep payload serialize failed: {e}")))?;
        core.job_service
            .enqueue_raw_delayed(sweep.job_type(), value, PRIORITY_SWEEP, sweep.continuation_delay())
            .await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        book::repository::book::MockBookRepository, jobs::repository::MockJobRepository, repository::testing::default_repository_service_builder,
        test_support::*,
    };

    struct CountingProcessor {
        processed: std::sync::Arc<std::sync::Mutex<Vec<BookId>>>,
    }

    #[async_trait::async_trait]
    impl BookIdSweep for CountingProcessor {
        fn job_type(&self) -> &'static str {
            "test.sweep"
        }

        async fn process_batch(&self, _core: &Arc<CoreServices>, ids: Vec<BookId>) -> Result<(), Error> {
            self.processed.lock().unwrap().extend(&ids);
            Ok(())
        }
    }

    fn make_core(book_repo: MockBookRepository, job_repo: MockJobRepository) -> Arc<CoreServices> {
        let repo_service = Arc::new(
            default_repository_service_builder()
                .book_repository(Arc::new(book_repo))
                .job_repository(Arc::new(job_repo))
                .build()
                .unwrap(),
        );
        crate::create_services(
            default_external_services_builder().repository_service(repo_service).build().unwrap(),
            "test-secret",
        )
        .unwrap()
    }

    fn fake_job() -> crate::jobs::Job {
        crate::jobs::Job {
            id: 1,
            job_type: "test.sweep".into(),
            payload: serde_json::json!({}),
            status: crate::jobs::JobStatus::Pending,
            priority: PRIORITY_SWEEP,
            attempt: 0,
            max_attempts: 3,
            version: 0,
            scheduled_at: chrono::Utc::now(),
            started_at: None,
            completed_at: None,
            error_message: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn full_batch_re_enqueues_continuation() {
        let mut book_repo = MockBookRepository::new();
        // Return exactly batch_size (3) IDs — full batch.
        book_repo
            .expect_find_available_books_for_sweep()
            .returning(|_, _, _| Box::pin(async { Ok(vec![1, 2, 3]) }));

        let mut job_repo = MockJobRepository::new();
        // Expect one delayed enqueue for the continuation.
        job_repo.expect_enqueue_delayed().once().returning(|_, job_type, payload, priority, _delay| {
            assert_eq!(job_type, "test.sweep");
            assert_eq!(priority, PRIORITY_SWEEP);
            let p: BookSweepPayload = serde_json::from_value(payload).unwrap();
            assert_eq!(p.after_id, Some(3));
            Box::pin(async { Ok(fake_job()) })
        });

        let core = make_core(book_repo, job_repo);
        let processed = Arc::new(std::sync::Mutex::new(vec![]));
        let sweep = CountingProcessor { processed: processed.clone() };

        run_book_id_sweep(&sweep, &core, BookSweepPayload { after_id: None, batch_size: 3 })
            .await
            .unwrap();

        assert_eq!(*processed.lock().unwrap(), vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn partial_batch_does_not_re_enqueue() {
        let mut book_repo = MockBookRepository::new();
        // Return fewer than batch_size — partial, sweep is done.
        book_repo
            .expect_find_available_books_for_sweep()
            .returning(|_, _, _| Box::pin(async { Ok(vec![1, 2]) }));

        // No expectations on job_repo — any enqueue call would panic.
        let job_repo = MockJobRepository::new();

        let core = make_core(book_repo, job_repo);
        let processed = Arc::new(std::sync::Mutex::new(vec![]));
        let sweep = CountingProcessor { processed: processed.clone() };

        run_book_id_sweep(&sweep, &core, BookSweepPayload { after_id: None, batch_size: 3 })
            .await
            .unwrap();

        assert_eq!(*processed.lock().unwrap(), vec![1, 2]);
    }

    #[tokio::test]
    async fn empty_batch_does_not_re_enqueue() {
        let mut book_repo = MockBookRepository::new();
        book_repo
            .expect_find_available_books_for_sweep()
            .returning(|_, _, _| Box::pin(async { Ok(vec![]) }));

        let job_repo = MockJobRepository::new();

        let core = make_core(book_repo, job_repo);
        let processed = Arc::new(std::sync::Mutex::new(vec![]));
        let sweep = CountingProcessor { processed: processed.clone() };

        run_book_id_sweep(&sweep, &core, BookSweepPayload { after_id: None, batch_size: 3 })
            .await
            .unwrap();

        assert!(processed.lock().unwrap().is_empty());
    }
}
