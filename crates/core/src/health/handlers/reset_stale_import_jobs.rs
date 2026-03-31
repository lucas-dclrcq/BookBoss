use std::sync::Arc;

use chrono::{Duration, Utc};

use crate::{
    CoreServices, Error,
    import::ImportStatus,
    jobs::JobHandler,
    message::{MessageSeverity, NewSystemMessage},
    repository::{read_only_transaction, transaction},
};

pub struct ResetStaleImportJobsHandler {
    core: Arc<CoreServices>,
}

impl ResetStaleImportJobsHandler {
    #[must_use]
    pub fn new(core: Arc<CoreServices>) -> Self {
        Self { core }
    }
}

impl JobHandler for ResetStaleImportJobsHandler {
    const JOB_TYPE: &'static str = "health.reset_stale_import_jobs";
    const DISPLAY_NAME: &'static str = "Reset Stale Import Jobs";
    type Payload = serde_json::Value;

    async fn handle(&self, _payload: serde_json::Value) -> Result<(), Error> {
        let cutoff = Utc::now() - Duration::hours(24);

        let import_repo = self.core.repository_service.import_job_repository().clone();
        let stale_jobs = read_only_transaction(&**self.core.repository_service.repository(), |tx| {
            let import_repo = import_repo.clone();
            Box::pin(async move { import_repo.find_stale_non_terminal_jobs(tx, cutoff).await })
        })
        .await?;

        if stale_jobs.is_empty() {
            tracing::info!("no stale import jobs found");
            return Ok(());
        }

        // Pre-check file existence for stuck in-progress jobs so we can move
        // missing-file jobs to Error rather than endlessly resetting them.
        let mut missing_file_jobs: Vec<(String, String)> = Vec::new();
        let mut jobs_to_update = stale_jobs.clone();
        for job in &mut jobs_to_update {
            if matches!(job.status, ImportStatus::Extracting | ImportStatus::Identifying) {
                if std::path::Path::new(&job.file_path).exists() {
                    job.status = ImportStatus::Pending;
                    job.error_message = Some("Reset by health check: stuck in processing state".to_string());
                } else {
                    let file_name = std::path::Path::new(&job.file_path)
                        .file_name()
                        .map_or_else(|| job.file_path.clone(), |n| n.to_string_lossy().into_owned());
                    missing_file_jobs.push((file_name, job.file_path.clone()));
                    job.status = ImportStatus::Error;
                    job.error_message = Some(format!("file no longer exists at {}", job.file_path));
                }
            } else if matches!(job.status, ImportStatus::Pending | ImportStatus::NeedsReview) {
                // These are "stale" by age but not stuck — log but don't change status.
                job.error_message = Some("Flagged by health check: stale for >24h".to_string());
            }
        }

        let count = jobs_to_update.len();
        let import_repo = self.core.repository_service.import_job_repository().clone();

        transaction(&**self.core.repository_service.repository(), |tx| {
            let import_repo = import_repo.clone();
            let jobs_to_update = jobs_to_update.clone();
            Box::pin(async move {
                for job in jobs_to_update {
                    import_repo.update_job(tx, job).await?;
                }
                Ok(())
            })
        })
        .await?;

        // Post individual error messages for jobs whose file has gone missing.
        for (file_name, file_path) in &missing_file_jobs {
            tracing::error!(%file_name, %file_path, "import job moved to Error: file no longer exists");
            self.core
                .system_message_service
                .add_message(NewSystemMessage {
                    source_task: Self::JOB_TYPE.to_string(),
                    severity: MessageSeverity::Error,
                    message: format!("Import job for \"{file_name}\" moved to Error: original file no longer exists"),
                })
                .await?;
        }

        tracing::warn!(count, "processed stale import jobs");

        if count > missing_file_jobs.len() {
            self.core
                .system_message_service
                .add_message(NewSystemMessage {
                    source_task: Self::JOB_TYPE.to_string(),
                    severity: MessageSeverity::Warning,
                    message: format!("Found {count} stale import job(s) — reset stuck jobs to Pending"),
                })
                .await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        book::FileFormat,
        import::{ImportJob, ImportJobToken, repository::import_job::MockImportJobRepository},
        message::repository::MockSystemMessageRepository,
        repository::testing::default_repository_service_builder,
        test_support::*,
    };

    fn make_stale_job(id: u64, status: ImportStatus, file_path: impl Into<String>) -> ImportJob {
        ImportJob {
            id,
            version: 0,
            token: ImportJobToken::new(id),
            file_path: file_path.into(),
            file_hash: format!("hash_{id}"),
            file_format: FileFormat::Epub,
            detected_at: Utc::now(),
            status,
            candidate_book_id: None,
            metadata_source: None,
            error_message: None,
            reviewed_by: None,
            reviewed_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn resets_stuck_extracting_jobs() {
        let mut import_repo = MockImportJobRepository::new();
        let mut msg_repo = MockSystemMessageRepository::new();

        // Use a real temp file so the existence check passes → reset to Pending
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let stale = make_stale_job(1, ImportStatus::Extracting, tmp.path().to_str().unwrap());

        import_repo.expect_find_stale_non_terminal_jobs().returning(move |_, _| {
            let stale = stale.clone();
            Box::pin(std::future::ready(Ok(vec![stale])))
        });

        import_repo.expect_update_job().returning(|_, job| {
            assert_eq!(job.status, ImportStatus::Pending);
            Box::pin(std::future::ready(Ok(job)))
        });

        msg_repo.expect_add_message().once().returning(|_, msg| {
            assert_eq!(msg.severity, MessageSeverity::Warning);
            let msg = crate::message::SystemMessage {
                id: 1,
                source_task: msg.source_task,
                severity: msg.severity,
                message: msg.message,
                created_at: chrono::Utc::now(),
            };
            Box::pin(std::future::ready(Ok(msg)))
        });

        let repo_service = Arc::new(
            default_repository_service_builder()
                .import_job_repository(Arc::new(import_repo))
                .system_message_repository(Arc::new(msg_repo))
                .build()
                .unwrap(),
        );

        let core = crate::create_services(
            default_external_services_builder().repository_service(repo_service).build().unwrap(),
            "test-secret",
        )
        .unwrap();

        let handler = ResetStaleImportJobsHandler::new(core);
        handler.handle(serde_json::json!({})).await.unwrap();
    }

    #[tokio::test]
    async fn moves_to_error_when_file_missing() {
        let mut import_repo = MockImportJobRepository::new();
        let mut msg_repo = MockSystemMessageRepository::new();

        // Non-existent path → should move to Error
        let stale = make_stale_job(2, ImportStatus::Extracting, "/nonexistent/missing.epub");

        import_repo.expect_find_stale_non_terminal_jobs().returning(move |_, _| {
            let stale = stale.clone();
            Box::pin(std::future::ready(Ok(vec![stale])))
        });

        import_repo.expect_update_job().returning(|_, job| {
            assert_eq!(job.status, ImportStatus::Error);
            Box::pin(std::future::ready(Ok(job)))
        });

        msg_repo.expect_add_message().once().returning(|_, msg| {
            assert_eq!(msg.severity, MessageSeverity::Error);
            assert!(msg.message.contains("missing.epub"), "message should include file name");
            let msg = crate::message::SystemMessage {
                id: 1,
                source_task: msg.source_task,
                severity: msg.severity,
                message: msg.message,
                created_at: chrono::Utc::now(),
            };
            Box::pin(std::future::ready(Ok(msg)))
        });

        let repo_service = Arc::new(
            default_repository_service_builder()
                .import_job_repository(Arc::new(import_repo))
                .system_message_repository(Arc::new(msg_repo))
                .build()
                .unwrap(),
        );

        let core = crate::create_services(
            default_external_services_builder().repository_service(repo_service).build().unwrap(),
            "test-secret",
        )
        .unwrap();

        let handler = ResetStaleImportJobsHandler::new(core);
        handler.handle(serde_json::json!({})).await.unwrap();
    }

    #[tokio::test]
    async fn noop_when_no_stale_jobs() {
        let mut import_repo = MockImportJobRepository::new();

        import_repo
            .expect_find_stale_non_terminal_jobs()
            .returning(|_, _| Box::pin(std::future::ready(Ok(vec![]))));

        let repo_service = Arc::new(
            default_repository_service_builder()
                .import_job_repository(Arc::new(import_repo))
                .build()
                .unwrap(),
        );

        let core = crate::create_services(
            default_external_services_builder().repository_service(repo_service).build().unwrap(),
            "test-secret",
        )
        .unwrap();

        let handler = ResetStaleImportJobsHandler::new(core);
        handler.handle(serde_json::json!({})).await.unwrap();
    }
}
