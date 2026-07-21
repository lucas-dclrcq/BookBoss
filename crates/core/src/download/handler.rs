use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{
    CoreServices, Error,
    jobs::{Enqueueable, JobHandler},
    message::{MessageSeverity, NewSystemMessage},
};

/// Payload for the Anna's Archive download job.
///
/// `title`/`authors`/`language` are best-effort labels carried from the search
/// result so the Incoming tab can display what is being downloaded while the
/// job is in flight; they are not authoritative — the real metadata comes from
/// the downloaded EPUB via the normal import pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnasDownloadPayload {
    /// Provider identifier (MD5 hash) of the file to download.
    pub external_id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub authors: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
}

impl Enqueueable for AnnasDownloadPayload {
    const JOB_TYPE: &'static str = "annas_download";
    const DEFAULT_PRIORITY: i16 = crate::jobs::PRIORITY_USER;
}

/// Background handler that downloads a book from the configured download source
/// and feeds the bytes into the shared import pipeline.
pub struct AnnasDownloadHandler {
    core: Arc<CoreServices>,
}

impl AnnasDownloadHandler {
    #[must_use]
    pub fn new(core: Arc<CoreServices>) -> Self {
        Self { core }
    }

    async fn run(&self, payload: &AnnasDownloadPayload) -> Result<(), Error> {
        let provider = self
            .core
            .download_source_service
            .provider()
            .ok_or_else(|| Error::Validation("download source is not configured".into()))?;

        let file = provider.fetch(&payload.external_id).await?;

        // Hand the bytes to the shared import pipeline — identical path to a
        // manual upload. Any non-error queue status (queued or already present)
        // is treated as success.
        self.core
            .import_job_service
            .queue_bytes_if_new(file.filename, file.bytes, crate::import::ImportOrigin::AnnasArchive)
            .await?;
        Ok(())
    }
}

impl JobHandler for AnnasDownloadHandler {
    const JOB_TYPE: &'static str = "annas_download";
    const DISPLAY_NAME: &'static str = "Download from Anna's Archive";
    type Payload = AnnasDownloadPayload;

    async fn handle(&self, payload: AnnasDownloadPayload) -> Result<(), Error> {
        let result = self.run(&payload).await;
        if let Err(ref e) = result {
            let label = payload.title.clone().unwrap_or_else(|| payload.external_id.clone());
            tracing::error!(external_id = %payload.external_id, error = %e, "annas_download failed");
            let _ = self
                .core
                .system_message_service
                .add_message(NewSystemMessage {
                    source_task: Self::JOB_TYPE.to_string(),
                    severity: MessageSeverity::Error,
                    message: format!("Download failed for \"{label}\": {e}"),
                })
                .await;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{message::repository::MockSystemMessageRepository, repository::testing::default_repository_service_builder, test_support::*};

    /// With no download provider registered, the handler must fail with a
    /// validation error and surface a system message.
    #[tokio::test]
    async fn posts_system_message_when_source_not_configured() {
        let mut msg_repo = MockSystemMessageRepository::new();
        msg_repo.expect_add_message().once().returning(|_, msg| {
            assert_eq!(msg.severity, MessageSeverity::Error);
            assert!(msg.message.contains("Moby"), "message should include the title label");
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
                .system_message_repository(Arc::new(msg_repo))
                .build()
                .unwrap(),
        );

        let core = crate::create_services(
            default_external_services_builder().repository_service(repo_service).build().unwrap(),
            "test-secret",
        )
        .unwrap();

        let handler = AnnasDownloadHandler::new(core);
        let result = handler
            .handle(AnnasDownloadPayload {
                external_id: "deadbeef".into(),
                title: Some("Moby Dick".into()),
                authors: None,
                language: None,
            })
            .await;

        assert!(result.is_err(), "handle should propagate the error when the source is unconfigured");
    }
}
