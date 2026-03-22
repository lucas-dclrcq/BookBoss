use chrono::{DateTime, Duration, Utc};
use tokio::sync::RwLock;

/// Static configuration for a health task.
#[derive(Debug, Clone)]
pub struct HealthTaskConfig {
    pub name: String,
    pub job_type: String,
    pub run_on_startup: bool,
    pub interval_minutes: u64,
}

/// Runtime state for a single task, exposed to the frontend.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HealthTaskInfo {
    pub name: String,
    pub job_type: String,
    pub run_on_startup: bool,
    pub interval_minutes: u64,
    pub last_run_at: Option<DateTime<Utc>>,
    pub next_run_at: DateTime<Utc>,
}

/// Shared scheduling state for all health tasks.
///
/// Held as `Arc<HealthTaskState>` and injected as an axum Extension so
/// both the scheduler subsystem and server functions can access it.
pub struct HealthTaskState {
    tasks: RwLock<Vec<HealthTaskInfo>>,
}

impl HealthTaskState {
    #[must_use]
    pub fn new(configs: Vec<HealthTaskConfig>) -> Self {
        let now = Utc::now();
        let tasks = configs
            .into_iter()
            .map(|c| {
                let next = if c.run_on_startup {
                    now // due immediately
                } else {
                    now + Duration::minutes(c.interval_minutes as i64)
                };
                HealthTaskInfo {
                    name: c.name,
                    job_type: c.job_type,
                    run_on_startup: c.run_on_startup,
                    interval_minutes: c.interval_minutes,
                    last_run_at: None,
                    next_run_at: next,
                }
            })
            .collect();
        Self { tasks: RwLock::new(tasks) }
    }

    /// Returns a snapshot of all task info for display in the UI.
    pub async fn list_tasks(&self) -> Vec<HealthTaskInfo> {
        self.tasks.read().await.clone()
    }

    /// Returns the `job_type` values of tasks whose `next_run_at` has passed.
    pub async fn due_tasks(&self) -> Vec<String> {
        let now = Utc::now();
        let tasks = self.tasks.read().await;
        tasks.iter().filter(|t| now >= t.next_run_at).map(|t| t.job_type.clone()).collect()
    }

    /// Mark a task as having been run: sets `last_run_at` to now and
    /// computes the next `next_run_at`.
    pub async fn mark_run(&self, job_type: &str) {
        let now = Utc::now();
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.job_type == job_type) {
            task.last_run_at = Some(now);
            task.next_run_at = now + Duration::minutes(task.interval_minutes as i64);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_configs() -> Vec<HealthTaskConfig> {
        vec![
            HealthTaskConfig {
                name: "Startup Task".into(),
                job_type: "health.startup".into(),
                run_on_startup: true,
                interval_minutes: 60,
            },
            HealthTaskConfig {
                name: "Periodic Task".into(),
                job_type: "health.periodic".into(),
                run_on_startup: false,
                interval_minutes: 360,
            },
        ]
    }

    #[tokio::test]
    async fn new_sets_startup_tasks_immediately_due() {
        let state = HealthTaskState::new(sample_configs());
        let tasks = state.list_tasks().await;

        let startup = tasks.iter().find(|t| t.job_type == "health.startup").unwrap();
        assert!(startup.last_run_at.is_none());
        // Startup task should be due now (next_run_at <= now)
        assert!(startup.next_run_at <= Utc::now());
    }

    #[tokio::test]
    async fn new_sets_non_startup_tasks_due_later() {
        let state = HealthTaskState::new(sample_configs());
        let tasks = state.list_tasks().await;

        let periodic = tasks.iter().find(|t| t.job_type == "health.periodic").unwrap();
        assert!(periodic.last_run_at.is_none());
        // Non-startup task should be due in the future
        assert!(periodic.next_run_at > Utc::now());
    }

    #[tokio::test]
    async fn due_tasks_returns_only_overdue() {
        let state = HealthTaskState::new(sample_configs());
        let due = state.due_tasks().await;

        // Only the startup task should be due
        assert_eq!(due.len(), 1);
        assert_eq!(due[0], "health.startup");
    }

    #[tokio::test]
    async fn mark_run_updates_last_run_and_next_run() {
        let state = HealthTaskState::new(sample_configs());

        state.mark_run("health.startup").await;

        let tasks = state.list_tasks().await;
        let startup = tasks.iter().find(|t| t.job_type == "health.startup").unwrap();

        assert!(startup.last_run_at.is_some());
        // next_run_at should be ~60 minutes in the future
        let diff = startup.next_run_at - Utc::now();
        assert!(diff.num_minutes() >= 59 && diff.num_minutes() <= 60);
    }

    #[tokio::test]
    async fn mark_run_for_unknown_job_type_is_noop() {
        let state = HealthTaskState::new(sample_configs());
        state.mark_run("health.nonexistent").await;

        let tasks = state.list_tasks().await;
        // Nothing should have changed
        assert!(tasks.iter().all(|t| t.last_run_at.is_none()));
    }

    #[tokio::test]
    async fn after_mark_run_task_is_no_longer_due() {
        let state = HealthTaskState::new(sample_configs());

        // Initially due
        assert_eq!(state.due_tasks().await.len(), 1);

        state.mark_run("health.startup").await;

        // No longer due
        assert!(state.due_tasks().await.is_empty());
    }
}
