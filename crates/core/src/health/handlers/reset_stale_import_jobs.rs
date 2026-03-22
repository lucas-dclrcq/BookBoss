use std::sync::Arc;

use chrono::{Duration, Utc};

use crate::{
    Error,
    import::ImportStatus,
    jobs::JobHandler,
    message::{MessageSeverity, NewSystemMessage, SystemMessageService},
    repository::{RepositoryService, read_only_transaction, transaction},
};

pub struct ResetStaleImportJobsHandler {
    repository_service: Arc<RepositoryService>,
    system_message_service: Arc<dyn SystemMessageService>,
}

impl ResetStaleImportJobsHandler {
    #[must_use]
    pub fn new(repository_service: Arc<RepositoryService>, system_message_service: Arc<dyn SystemMessageService>) -> Self {
        Self {
            repository_service,
            system_message_service,
        }
    }
}

impl JobHandler for ResetStaleImportJobsHandler {
    const JOB_TYPE: &'static str = "health.reset_stale_import_jobs";
    type Payload = serde_json::Value;

    async fn handle(&self, _payload: serde_json::Value) -> Result<(), Error> {
        let cutoff = Utc::now() - Duration::hours(24);

        let import_repo = self.repository_service.import_job_repository().clone();
        let stale_jobs = read_only_transaction(&**self.repository_service.repository(), |tx| {
            let import_repo = import_repo.clone();
            Box::pin(async move { import_repo.find_stale_non_terminal_jobs(tx, cutoff).await })
        })
        .await?;

        if stale_jobs.is_empty() {
            tracing::info!("no stale import jobs found");
            return Ok(());
        }

        let count = stale_jobs.len();
        let import_repo = self.repository_service.import_job_repository().clone();

        // Reset stale jobs: Pending/NeedsReview stay as-is (they may just be waiting),
        // but Extracting/Identifying are stuck in-progress and should be reset to
        // Pending.
        transaction(&**self.repository_service.repository(), |tx| {
            let import_repo = import_repo.clone();
            let stale_jobs = stale_jobs.clone();
            Box::pin(async move {
                for mut job in stale_jobs {
                    match job.status {
                        ImportStatus::Extracting | ImportStatus::Identifying => {
                            job.status = ImportStatus::Pending;
                            job.error_message = Some("Reset by health check: stuck in processing state".to_string());
                            import_repo.update_job(tx, job).await?;
                        }
                        ImportStatus::Pending | ImportStatus::NeedsReview => {
                            // These are "stale" by age but not stuck — log but don't change.
                            job.error_message = Some("Flagged by health check: stale for >24h".to_string());
                            import_repo.update_job(tx, job).await?;
                        }
                        _ => {}
                    }
                }
                Ok(())
            })
        })
        .await?;

        tracing::warn!(count, "reset stale import jobs");

        self.system_message_service
            .add_message(NewSystemMessage {
                source_task: Self::JOB_TYPE.to_string(),
                severity: MessageSeverity::Warning,
                message: format!("Found {count} stale import job(s) — reset stuck jobs to Pending"),
            })
            .await?;

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
    };

    fn make_stale_job(id: u64, status: ImportStatus) -> ImportJob {
        ImportJob {
            id,
            version: 0,
            token: ImportJobToken::new(id),
            file_path: format!("/watch/stale_{id}.epub"),
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

        let stale = make_stale_job(1, ImportStatus::Extracting);

        import_repo.expect_find_stale_non_terminal_jobs().returning(move |_, _| {
            let stale = stale.clone();
            Box::pin(std::future::ready(Ok(vec![stale])))
        });

        import_repo.expect_update_job().returning(|_, job| {
            assert_eq!(job.status, ImportStatus::Pending);
            Box::pin(std::future::ready(Ok(job)))
        });

        msg_repo.expect_add_message().returning(|_, msg| {
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

        let sms = crate::message::SystemMessageServiceImpl::new(repo_service.clone(), crate::test_support::nop_event_service());
        let handler = ResetStaleImportJobsHandler::new(repo_service, Arc::new(sms));
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

        let sms = crate::message::SystemMessageServiceImpl::new(repo_service.clone(), crate::test_support::nop_event_service());
        let handler = ResetStaleImportJobsHandler::new(repo_service, Arc::new(sms));
        handler.handle(serde_json::json!({})).await.unwrap();
    }
}
