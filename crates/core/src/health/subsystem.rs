use std::{sync::Arc, time::Duration};

use tokio_graceful_shutdown::{IntoSubsystem, SubsystemHandle};

use crate::{
    Error,
    event::EventService,
    health::service::{HealthKickReceiver, HealthService},
    jobs::JobService,
};

const POLL_INTERVAL: Duration = Duration::from_secs(30);

// ── Subsystem ────────────────────────────────────────────────────────────────

pub struct HealthCheckSubsystem {
    health_service: Arc<dyn HealthService>,
    job_service: Arc<dyn JobService>,
    event_service: Arc<dyn EventService>,
    kick_rx: HealthKickReceiver,
}

impl HealthCheckSubsystem {
    /// Enqueue a health check job for the given `job_type`.
    ///
    /// Uses `count_pending_by_type` to avoid duplicate enqueues — if a job of
    /// the same type is already pending/running, we skip it.
    async fn enqueue_task(&self, job_type: &str) -> Result<(), Error> {
        let pending = self.job_service.count_pending_by_type(job_type).await?;

        if pending > 0 {
            tracing::debug!(job_type, "health task already pending/running, skipping");
            return Ok(());
        }

        self.job_service.enqueue_raw(job_type, serde_json::json!({}), 0).await?;

        tracing::info!(job_type, "enqueued health check task");
        self.event_service.notify_jobs_changed();
        Ok(())
    }

    /// Enqueue + mark_run for a batch of due tasks.
    async fn process_due_tasks(&self, due: &[String]) {
        for job_type in due {
            if let Err(e) = self.enqueue_task(job_type).await {
                tracing::error!(job_type, error = %e, "failed to enqueue health task");
            }
            self.health_service.mark_run(job_type).await;
        }
    }
}

impl IntoSubsystem<Error> for HealthCheckSubsystem {
    async fn run(mut self, subsys: &mut SubsystemHandle) -> Result<(), Error> {
        // Enqueue all due tasks (startup tasks are due immediately).
        let due = self.health_service.due_tasks().await;
        self.process_due_tasks(&due).await;

        if !due.is_empty() {
            tracing::info!(count = due.len(), "enqueued startup health tasks");
        }

        tracing::info!("HealthCheckSubsystem started");

        // Main loop: respond to manual kicks and periodic polls.
        loop {
            tokio::select! {
                () = subsys.on_shutdown_requested() => {
                    tracing::info!("HealthCheckSubsystem shutting down...");
                    break;
                }
                // Manual trigger from "Run Now" button.
                Some(job_type) = self.kick_rx.0.recv() => {
                    self.health_service.mark_due_now(&job_type).await;
                    self.process_due_tasks(&[job_type]).await;
                }
                // Periodic poll for scheduled tasks.
                () = tokio::time::sleep(POLL_INTERVAL) => {
                    let due = self.health_service.due_tasks().await;
                    self.process_due_tasks(&due).await;
                }
            }
        }

        Ok(())
    }
}

#[must_use]
pub fn create_health_subsystem(
    health_service: Arc<dyn HealthService>,
    job_service: Arc<dyn JobService>,
    event_service: Arc<dyn EventService>,
    kick_rx: HealthKickReceiver,
) -> HealthCheckSubsystem {
    HealthCheckSubsystem {
        health_service,
        job_service,
        event_service,
        kick_rx,
    }
}
