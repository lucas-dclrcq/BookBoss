use super::task::HealthTaskConfig;

/// Returns the default set of health check task definitions.
#[must_use]
pub fn default_health_tasks() -> Vec<HealthTaskConfig> {
    vec![
        HealthTaskConfig {
            name: "Recover Enrichments".into(),
            job_type: "health.recover_enrichments".into(),
            run_on_startup: true,
            interval_minutes: 60,
        },
        HealthTaskConfig {
            name: "Ensure Enrichments".into(),
            job_type: "health.ensure_enrichments".into(),
            run_on_startup: true,
            interval_minutes: 120,
        },
        HealthTaskConfig {
            name: "Cleanup Orphan Authors".into(),
            job_type: "health.cleanup_orphan_authors".into(),
            run_on_startup: false,
            interval_minutes: 360,
        },
        HealthTaskConfig {
            name: "Cleanup Orphan Series".into(),
            job_type: "health.cleanup_orphan_series".into(),
            run_on_startup: false,
            interval_minutes: 360,
        },
        HealthTaskConfig {
            name: "Cleanup Orphan Publishers".into(),
            job_type: "health.cleanup_orphan_publishers".into(),
            run_on_startup: false,
            interval_minutes: 360,
        },
        HealthTaskConfig {
            name: "Cleanup Old Jobs".into(),
            job_type: "health.cleanup_old_jobs".into(),
            run_on_startup: false,
            interval_minutes: 1440,
        },
        HealthTaskConfig {
            name: "Cleanup Old Import Jobs".into(),
            job_type: "health.cleanup_old_import_jobs".into(),
            run_on_startup: false,
            interval_minutes: 1440,
        },
        HealthTaskConfig {
            name: "Cleanup Old System Messages".into(),
            job_type: "health.cleanup_old_system_messages".into(),
            run_on_startup: false,
            interval_minutes: 1440,
        },
        HealthTaskConfig {
            name: "Verify Library File Integrity".into(),
            job_type: "health.verify_file_integrity".into(),
            run_on_startup: false,
            interval_minutes: 720,
        },
        HealthTaskConfig {
            name: "Reset Stale Import Jobs".into(),
            job_type: "health.reset_stale_import_jobs".into(),
            run_on_startup: true,
            interval_minutes: 360,
        },
        HealthTaskConfig {
            name: "Cleanup Expired Sessions".into(),
            job_type: "health.cleanup_expired_sessions".into(),
            run_on_startup: false,
            interval_minutes: 1440,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_health_tasks_returns_10_tasks() {
        let tasks = default_health_tasks();
        assert_eq!(tasks.len(), 11);
    }

    #[test]
    fn all_job_types_are_unique() {
        let tasks = default_health_tasks();
        let mut types: Vec<_> = tasks.iter().map(|t| &t.job_type).collect();
        types.sort();
        types.dedup();
        assert_eq!(types.len(), tasks.len());
    }

    #[test]
    fn startup_tasks_have_correct_flag() {
        let tasks = default_health_tasks();
        let startup: Vec<_> = tasks.iter().filter(|t| t.run_on_startup).collect();
        assert_eq!(startup.len(), 3);
        assert!(startup.iter().any(|t| t.job_type == "health.recover_enrichments"));
        assert!(startup.iter().any(|t| t.job_type == "health.ensure_enrichments"));
        assert!(startup.iter().any(|t| t.job_type == "health.reset_stale_import_jobs"));
    }
}
