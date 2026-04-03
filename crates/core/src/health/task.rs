use std::sync::RwLock;

use chrono::{DateTime, Duration, Utc};

/// Static configuration for a health task.
#[derive(Debug, Clone)]
pub struct HealthTaskConfig {
    pub name: String,
    pub job_type: String,
    pub run_on_startup: bool,
    /// How often to re-run after each execution. `None` means manual-only —
    /// the task never auto-schedules and must be triggered via `mark_due_now`.
    pub interval_minutes: Option<u64>,
    /// Job queue priority for this task. Defaults to `PRIORITY_HEALTH`.
    pub priority: i16,
}

/// Runtime state for a single task, exposed to the frontend.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HealthTaskInfo {
    pub name: String,
    pub job_type: String,
    pub run_on_startup: bool,
    /// `None` for manual-only tasks.
    pub interval_minutes: Option<u64>,
    pub priority: i16,
    pub last_run_at: Option<DateTime<Utc>>,
    /// `None` for manual-only tasks that have not been triggered.
    pub next_run_at: Option<DateTime<Utc>>,
}

/// Shared scheduling state for all health tasks.
///
/// Wrapped by `HealthServiceImpl` — not used directly outside the health
/// module.
pub struct HealthTaskState {
    tasks: RwLock<Vec<HealthTaskInfo>>,
}

impl HealthTaskState {
    /// Creates an empty `HealthTaskState`. Tasks are added via `register_task`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tasks: RwLock::new(Vec::new()),
        }
    }

    /// Register a health task from its config. Computes the initial
    /// `next_run_at` based on `run_on_startup` and `interval_minutes`.
    pub fn register_task(&self, config: HealthTaskConfig) {
        let now = Utc::now();
        let next = if config.run_on_startup {
            Some(now)
        } else {
            config.interval_minutes.map(|m| now + Duration::minutes(m as i64))
        };
        let info = HealthTaskInfo {
            name: config.name,
            job_type: config.job_type,
            run_on_startup: config.run_on_startup,
            interval_minutes: config.interval_minutes,
            priority: config.priority,
            last_run_at: None,
            next_run_at: next,
        };
        self.tasks.write().expect("task lock poisoned").push(info);
    }

    /// Returns a snapshot of all task info for display in the UI.
    pub fn list_tasks(&self) -> Vec<HealthTaskInfo> {
        self.tasks.read().expect("task lock poisoned").clone()
    }

    /// Returns the priority for the given `job_type`, or `PRIORITY_HEALTH` if
    /// not found.
    pub fn task_priority(&self, job_type: &str) -> i16 {
        use crate::jobs::PRIORITY_HEALTH;
        self.tasks
            .read()
            .expect("task lock poisoned")
            .iter()
            .find(|t| t.job_type == job_type)
            .map_or(PRIORITY_HEALTH, |t| t.priority)
    }

    /// Returns the `job_type` values of tasks whose `next_run_at` has passed.
    /// Tasks with `next_run_at = None` (manual-only) are never returned here.
    pub fn due_tasks(&self) -> Vec<String> {
        let now = Utc::now();
        let tasks = self.tasks.read().expect("task lock poisoned");
        tasks
            .iter()
            .filter(|t| t.next_run_at.is_some_and(|next| now >= next))
            .map(|t| t.job_type.clone())
            .collect()
    }

    /// Mark a task as having been run: sets `last_run_at` to now and
    /// computes the next `next_run_at`. Manual-only tasks (`interval_minutes =
    /// None`) return to `next_run_at = None` after running.
    pub fn mark_run(&self, job_type: &str) {
        let now = Utc::now();
        let mut tasks = self.tasks.write().expect("task lock poisoned");
        if let Some(task) = tasks.iter_mut().find(|t| t.job_type == job_type) {
            task.last_run_at = Some(now);
            task.next_run_at = task.interval_minutes.map(|m| now + Duration::minutes(m as i64));
        }
    }

    /// Set a task's `next_run_at` to now so the scheduler picks it up on its
    /// next poll cycle. Used by the "Run Now" admin action.
    pub fn mark_due_now(&self, job_type: &str) {
        let now = Utc::now();
        let mut tasks = self.tasks.write().expect("task lock poisoned");
        if let Some(task) = tasks.iter_mut().find(|t| t.job_type == job_type) {
            task.next_run_at = Some(now);
        }
    }
}

impl Default for HealthTaskState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_configs() -> Vec<HealthTaskConfig> {
        use crate::jobs::PRIORITY_HEALTH;
        vec![
            HealthTaskConfig {
                name: "Startup Task".into(),
                job_type: "health.startup".into(),
                run_on_startup: true,
                interval_minutes: Some(60),
                priority: PRIORITY_HEALTH,
            },
            HealthTaskConfig {
                name: "Periodic Task".into(),
                job_type: "health.periodic".into(),
                run_on_startup: false,
                interval_minutes: Some(360),
                priority: PRIORITY_HEALTH,
            },
        ]
    }

    fn manual_config() -> HealthTaskConfig {
        use crate::jobs::PRIORITY_HEALTH;
        HealthTaskConfig {
            name: "Manual Task".into(),
            job_type: "health.manual".into(),
            run_on_startup: false,
            interval_minutes: None,
            priority: PRIORITY_HEALTH,
        }
    }

    fn state_with_samples() -> HealthTaskState {
        let state = HealthTaskState::new();
        for config in sample_configs() {
            state.register_task(config);
        }
        state
    }

    #[test]
    fn register_sets_startup_tasks_immediately_due() {
        let state = state_with_samples();
        let tasks = state.list_tasks();

        let startup = tasks.iter().find(|t| t.job_type == "health.startup").unwrap();
        assert!(startup.last_run_at.is_none());
        assert!(startup.next_run_at.unwrap() <= Utc::now());
    }

    #[test]
    fn register_sets_non_startup_tasks_due_later() {
        let state = state_with_samples();
        let tasks = state.list_tasks();

        let periodic = tasks.iter().find(|t| t.job_type == "health.periodic").unwrap();
        assert!(periodic.last_run_at.is_none());
        assert!(periodic.next_run_at.unwrap() > Utc::now());
    }

    #[test]
    fn register_manual_task_has_no_next_run() {
        let state = HealthTaskState::new();
        state.register_task(manual_config());

        let tasks = state.list_tasks();
        let manual = tasks.iter().find(|t| t.job_type == "health.manual").unwrap();
        assert!(manual.next_run_at.is_none());
        assert!(manual.interval_minutes.is_none());
    }

    #[test]
    fn due_tasks_returns_only_overdue() {
        let state = state_with_samples();
        let due = state.due_tasks();

        assert_eq!(due.len(), 1);
        assert_eq!(due[0], "health.startup");
    }

    #[test]
    fn mark_run_updates_last_run_and_next_run() {
        let state = state_with_samples();

        state.mark_run("health.startup");

        let tasks = state.list_tasks();
        let startup = tasks.iter().find(|t| t.job_type == "health.startup").unwrap();

        assert!(startup.last_run_at.is_some());
        let diff = startup.next_run_at.unwrap() - Utc::now();
        assert!(diff.num_minutes() >= 59 && diff.num_minutes() <= 60);
    }

    #[test]
    fn mark_run_for_unknown_job_type_is_noop() {
        let state = state_with_samples();
        state.mark_run("health.nonexistent");

        let tasks = state.list_tasks();
        assert!(tasks.iter().all(|t| t.last_run_at.is_none()));
    }

    #[test]
    fn after_mark_run_task_is_no_longer_due() {
        let state = state_with_samples();

        assert_eq!(state.due_tasks().len(), 1);
        state.mark_run("health.startup");
        assert!(state.due_tasks().is_empty());
    }

    #[test]
    fn mark_due_now_makes_non_startup_task_due() {
        let state = state_with_samples();

        let due = state.due_tasks();
        assert!(!due.contains(&"health.periodic".to_string()));

        state.mark_due_now("health.periodic");

        let due = state.due_tasks();
        assert!(due.contains(&"health.periodic".to_string()));
    }

    #[test]
    fn manual_task_never_due_until_triggered() {
        let state = HealthTaskState::new();
        state.register_task(manual_config());

        assert!(!state.due_tasks().contains(&"health.manual".to_string()));

        state.mark_due_now("health.manual");

        assert!(state.due_tasks().contains(&"health.manual".to_string()));
    }

    #[test]
    fn manual_task_returns_to_none_after_mark_run() {
        let state = HealthTaskState::new();
        state.register_task(manual_config());

        state.mark_due_now("health.manual");
        assert!(state.due_tasks().contains(&"health.manual".to_string()));

        state.mark_run("health.manual");

        let tasks = state.list_tasks();
        let manual = tasks.iter().find(|t| t.job_type == "health.manual").unwrap();
        assert!(manual.next_run_at.is_none());
        assert!(manual.last_run_at.is_some());
        assert!(!state.due_tasks().contains(&"health.manual".to_string()));
    }
}
