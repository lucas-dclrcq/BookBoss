use chrono::{DateTime, Utc};

use crate::{
    Error,
    import::{ImportJob, ImportJobId, ImportJobToken, ImportStatus, NewImportJob},
    repository::Transaction,
};

#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait ImportJobRepository: Send + Sync {
    async fn add_job(&self, transaction: &dyn Transaction, job: NewImportJob) -> Result<ImportJob, Error>;
    async fn update_job(&self, transaction: &dyn Transaction, job: ImportJob) -> Result<ImportJob, Error>;
    async fn find_by_id(&self, transaction: &dyn Transaction, id: ImportJobId) -> Result<Option<ImportJob>, Error>;
    async fn find_by_token(&self, transaction: &dyn Transaction, token: ImportJobToken) -> Result<Option<ImportJob>, Error>;
    async fn find_active_by_hash(&self, transaction: &dyn Transaction, file_hash: &str) -> Result<Option<ImportJob>, Error>;
    async fn list_by_status(
        &self,
        transaction: &dyn Transaction,
        status: ImportStatus,
        start_id: Option<ImportJobId>,
        page_size: Option<u64>,
    ) -> Result<Vec<ImportJob>, Error>;

    /// Reset any import jobs left in `Extracting` or `Identifying` state back
    /// to `Pending`. Called on startup to recover from a previous crash.
    /// Returns the number of jobs reset.
    async fn reset_in_progress_to_pending(&self, transaction: &dyn Transaction) -> Result<u64, Error>;

    /// Finds the import job whose `candidate_book_id` matches the given book.
    async fn find_by_candidate_book_id(&self, transaction: &dyn Transaction, book_id: crate::book::BookId) -> Result<Option<ImportJob>, Error>;

    /// Permanently deletes an import job record.
    async fn delete_job(&self, transaction: &dyn Transaction, job_id: ImportJobId) -> Result<(), Error>;

    /// Sets the job status to `Approved` and records who approved it.
    async fn approve_job(&self, transaction: &dyn Transaction, job_id: ImportJobId, reviewer_id: crate::user::UserId) -> Result<(), Error>;

    /// Delete import jobs in terminal states (Approved/Rejected) older than the
    /// cutoff. Returns the number of jobs deleted.
    async fn delete_old_terminal_jobs(&self, transaction: &dyn Transaction, cutoff: DateTime<Utc>) -> Result<u64, Error>;

    /// Find import jobs stuck in non-terminal states
    /// (Pending/Extracting/Identifying/NeedsReview) that were last updated
    /// before the cutoff.
    async fn find_stale_non_terminal_jobs(&self, transaction: &dyn Transaction, cutoff: DateTime<Utc>) -> Result<Vec<ImportJob>, Error>;
}
