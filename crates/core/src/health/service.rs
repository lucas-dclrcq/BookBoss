use std::sync::Arc;

use tokio::sync::mpsc;

use super::task::{HealthTaskConfig, HealthTaskInfo, HealthTaskState};

/// Port trait for health task scheduling and management.
///
/// Wraps task registration, scheduling state, and the "Run Now" trigger
/// channel into a single service. Created via [`create_health_service`].
#[async_trait::async_trait]
pub trait HealthService: Send + Sync {
    /// Register a health task. Called during crate `before_start()` hooks.
    fn register_task(&self, config: HealthTaskConfig);

    /// Returns a snapshot of all task info for display in the UI.
    async fn list_tasks(&self) -> Vec<HealthTaskInfo>;

    /// Returns the `job_type` values of tasks whose `next_run_at` has passed.
    async fn due_tasks(&self) -> Vec<String>;

    /// Mark a task as having been run and compute the next run time.
    async fn mark_run(&self, job_type: &str);

    /// Set a task's `next_run_at` to now so it's picked up on the next poll.
    async fn mark_due_now(&self, job_type: &str);

    /// Request that the health subsystem enqueue and run the given task now.
    ///
    /// Non-blocking: if the channel buffer is full the request is silently
    /// dropped — the subsystem will pick it up on the next poll cycle.
    fn kick(&self, job_type: String);
}

struct HealthServiceImpl {
    state: HealthTaskState,
    kick_tx: mpsc::Sender<String>,
}

#[async_trait::async_trait]
impl HealthService for HealthServiceImpl {
    fn register_task(&self, config: HealthTaskConfig) {
        self.state.register_task(config);
    }

    async fn list_tasks(&self) -> Vec<HealthTaskInfo> {
        self.state.list_tasks()
    }

    async fn due_tasks(&self) -> Vec<String> {
        self.state.due_tasks()
    }

    async fn mark_run(&self, job_type: &str) {
        self.state.mark_run(job_type);
    }

    async fn mark_due_now(&self, job_type: &str) {
        self.state.mark_due_now(job_type);
    }

    fn kick(&self, job_type: String) {
        let _ = self.kick_tx.try_send(job_type);
    }
}

/// Receiving end of the kick channel, consumed by `HealthCheckSubsystem`.
pub(super) struct HealthKickReceiver(pub(super) mpsc::Receiver<String>);

/// Creates a `HealthService` and its paired kick receiver.
///
/// Private to the `health` module — external code uses
/// [`create_health_subsystem`](super::create_health_subsystem) which
/// keeps the channel as an internal implementation detail.
/// Channel capacity is 16 — enough to buffer a burst of "Run Now" clicks.
#[must_use]
pub(super) fn create_health_service() -> (Arc<dyn HealthService>, HealthKickReceiver) {
    let (tx, rx) = mpsc::channel(16);
    let service = HealthServiceImpl {
        state: HealthTaskState::new(),
        kick_tx: tx,
    };
    (Arc::new(service), HealthKickReceiver(rx))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config() -> HealthTaskConfig {
        HealthTaskConfig {
            name: "Test Task".into(),
            job_type: "health.test".into(),
            run_on_startup: true,
            interval_minutes: Some(60),
        }
    }

    #[tokio::test]
    async fn register_and_list() {
        let (svc, _rx) = create_health_service();
        svc.register_task(sample_config());

        let tasks = svc.list_tasks().await;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].job_type, "health.test");
    }

    #[tokio::test]
    async fn kick_sends_to_receiver() {
        let (svc, mut rx) = create_health_service();
        svc.register_task(sample_config());
        svc.kick("health.test".to_string());

        let received = rx.0.recv().await;
        assert_eq!(received, Some("health.test".to_string()));
    }

    #[tokio::test]
    async fn due_tasks_includes_startup() {
        let (svc, _rx) = create_health_service();
        svc.register_task(sample_config());

        let due = svc.due_tasks().await;
        assert!(due.contains(&"health.test".to_string()));
    }

    #[tokio::test]
    async fn mark_run_clears_due() {
        let (svc, _rx) = create_health_service();
        svc.register_task(sample_config());

        svc.mark_run("health.test").await;

        let due = svc.due_tasks().await;
        assert!(due.is_empty());
    }
}
