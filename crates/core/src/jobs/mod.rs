pub mod handler;
pub mod model;
pub mod repository;
pub mod service;
pub mod sweep;
pub mod worker;

pub use handler::{ErasedJobHandler, JobHandler};
pub use model::{Job, JobId, JobStatus};
pub use priority::{PRIORITY_HEALTH, PRIORITY_NORMAL, PRIORITY_SWEEP, PRIORITY_USER};
pub use repository::{Enqueueable, JobRepository, JobRepositoryExt};
pub use service::{JobService, JobServiceExt, create_job_service};
pub use sweep::{BookIdSweep, BookSweepPayload, run_book_id_sweep};
pub use worker::JobWorker;

pub mod priority {
    /// Background-only sweep jobs — only runs when nothing else is waiting.
    pub const PRIORITY_SWEEP: i16 = 0;
    /// Periodic health checks.
    pub const PRIORITY_HEALTH: i16 = 5;
    /// Standard pipeline work (enrich, convert).
    pub const PRIORITY_NORMAL: i16 = 10;
    /// User-initiated actions.
    pub const PRIORITY_USER: i16 = 20;
}
