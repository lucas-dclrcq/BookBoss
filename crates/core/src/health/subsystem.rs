use std::{sync::Arc, time::Duration};

use tokio_graceful_shutdown::{IntoSubsystem, SubsystemHandle};

use crate::{
    Error,
    event::EventService,
    health::HealthTaskState,
    jobs::JobRepository,
    repository::{Repository, transaction},
};

const POLL_INTERVAL: Duration = Duration::from_secs(30);

pub struct HealthCheckSubsystem {
    state: Arc<HealthTaskState>,
    repository: Arc<dyn Repository>,
    job_repo: Arc<dyn JobRepository>,
    event_service: Arc<dyn EventService>,
}

impl HealthCheckSubsystem {
    #[must_use]
    pub fn new(state: Arc<HealthTaskState>, repository: Arc<dyn Repository>, job_repo: Arc<dyn JobRepository>, event_service: Arc<dyn EventService>) -> Self {
        Self {
            state,
            repository,
            job_repo,
            event_service,
        }
    }

    /// Enqueue a health check job for the given `job_type`.
    ///
    /// Uses `count_pending_by_type` to avoid duplicate enqueues — if a job of
    /// the same type is already pending/running, we skip it.
    async fn enqueue_task(&self, job_type: &str) -> Result<(), Error> {
        let job_repo = self.job_repo.clone();
        let jt = job_type.to_string();

        let pending = transaction(&*self.repository, |tx| {
            let job_repo = job_repo.clone();
            let jt = jt.clone();
            Box::pin(async move { job_repo.count_pending_by_type(tx, &jt).await })
        })
        .await?;

        if pending > 0 {
            tracing::debug!(job_type, "health task already pending/running, skipping");
            return Ok(());
        }

        let job_repo = self.job_repo.clone();
        let jt = job_type.to_string();
        transaction(&*self.repository, |tx| {
            let job_repo = job_repo.clone();
            let jt = jt.clone();
            Box::pin(async move { job_repo.enqueue_raw(tx, &jt, serde_json::json!({}), 0).await })
        })
        .await?;

        tracing::info!(job_type, "enqueued health check task");
        self.event_service.notify_jobs_changed();
        Ok(())
    }
}

impl IntoSubsystem<Error> for HealthCheckSubsystem {
    async fn run(self, subsys: &mut SubsystemHandle) -> Result<(), Error> {
        // Enqueue all due tasks (startup tasks are due immediately).
        let due = self.state.due_tasks().await;
        for job_type in &due {
            if let Err(e) = self.enqueue_task(job_type).await {
                tracing::error!(job_type, error = %e, "failed to enqueue startup health task");
            }
            self.state.mark_run(job_type).await;
        }

        if !due.is_empty() {
            tracing::info!(count = due.len(), "enqueued startup health tasks");
        }

        tracing::info!("HealthCheckSubsystem started");

        // Poll loop: check for due tasks every POLL_INTERVAL.
        loop {
            tokio::select! {
                () = subsys.on_shutdown_requested() => {
                    tracing::info!("HealthCheckSubsystem shutting down...");
                    break;
                }
                () = tokio::time::sleep(POLL_INTERVAL) => {
                    let due = self.state.due_tasks().await;
                    for job_type in &due {
                        if let Err(e) = self.enqueue_task(job_type).await {
                            tracing::error!(job_type, error = %e, "failed to enqueue health task");
                        }
                        self.state.mark_run(job_type).await;
                    }
                }
            }
        }

        Ok(())
    }
}

#[must_use]
pub fn create_health_subsystem(
    state: Arc<HealthTaskState>,
    repository: Arc<dyn Repository>,
    job_repo: Arc<dyn JobRepository>,
    event_service: Arc<dyn EventService>,
) -> HealthCheckSubsystem {
    HealthCheckSubsystem::new(state, repository, job_repo, event_service)
}
