use std::{sync::Arc, time::Duration};

use tokio::sync::mpsc;
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemHandle};

use crate::{
    Error,
    event::EventService,
    health::HealthTaskState,
    jobs::JobRepository,
    repository::{Repository, transaction},
};

const POLL_INTERVAL: Duration = Duration::from_secs(30);

// ── Trigger channel ──────────────────────────────────────────────────────────

/// Lightweight, cloneable handle that kicks the health subsystem to run a
/// specific task immediately. Mirrors the `ScanTrigger` pattern from the
/// import crate.
#[derive(Clone)]
pub struct HealthTrigger {
    tx: mpsc::Sender<String>,
}

impl HealthTrigger {
    /// Request that the subsystem enqueue and run the given task now.
    ///
    /// Non-blocking: if the channel is full the call is silently dropped —
    /// the subsystem will pick up the task on its next poll cycle.
    pub fn kick(&self, job_type: String) {
        let _ = self.tx.try_send(job_type);
    }
}

/// Receiving end of the trigger channel, consumed by `HealthCheckSubsystem`.
pub struct HealthTriggerReceiver(mpsc::Receiver<String>);

/// Creates a matched `(HealthTrigger, HealthTriggerReceiver)` pair.
///
/// Channel capacity is 16 — enough to buffer a burst of "Run Now" clicks
/// without blocking.
#[must_use]
pub fn create_health_trigger() -> (HealthTrigger, HealthTriggerReceiver) {
    let (tx, rx) = mpsc::channel(16);
    (HealthTrigger { tx }, HealthTriggerReceiver(rx))
}

// ── Subsystem ────────────────────────────────────────────────────────────────

pub struct HealthCheckSubsystem {
    state: Arc<HealthTaskState>,
    repository: Arc<dyn Repository>,
    job_repo: Arc<dyn JobRepository>,
    event_service: Arc<dyn EventService>,
    trigger_rx: HealthTriggerReceiver,
}

impl HealthCheckSubsystem {
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

    /// Enqueue + mark_run for a batch of due tasks.
    async fn process_due_tasks(&self, due: &[String]) {
        for job_type in due {
            if let Err(e) = self.enqueue_task(job_type).await {
                tracing::error!(job_type, error = %e, "failed to enqueue health task");
            }
            self.state.mark_run(job_type).await;
        }
    }
}

impl IntoSubsystem<Error> for HealthCheckSubsystem {
    async fn run(mut self, subsys: &mut SubsystemHandle) -> Result<(), Error> {
        // Enqueue all due tasks (startup tasks are due immediately).
        let due = self.state.due_tasks().await;
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
                Some(job_type) = self.trigger_rx.0.recv() => {
                    self.state.mark_due_now(&job_type).await;
                    self.process_due_tasks(&[job_type]).await;
                }
                // Periodic poll for scheduled tasks.
                () = tokio::time::sleep(POLL_INTERVAL) => {
                    let due = self.state.due_tasks().await;
                    self.process_due_tasks(&due).await;
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
    trigger_rx: HealthTriggerReceiver,
) -> HealthCheckSubsystem {
    HealthCheckSubsystem {
        state,
        repository,
        job_repo,
        event_service,
        trigger_rx,
    }
}
