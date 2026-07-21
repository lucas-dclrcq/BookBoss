use async_trait::async_trait;

use crate::{
    Error,
    download::{DownloadCandidate, DownloadedFile},
};

/// Port trait for an external book-download source (e.g. Anna's Archive).
///
/// Implemented by `crates/download/`. `search` returns candidates already
/// filtered to EPUB; `fetch` resolves the provider's download API and returns
/// the file bytes, ready to be pushed through the shared import pipeline.
///
/// `name()` returns a human-readable label used by the UI to identify the
/// source.
#[async_trait]
pub trait DownloadProvider: Send + Sync {
    fn name(&self) -> &'static str;

    /// Search the provider for EPUB books matching `query`, optionally filtered
    /// by a two-letter language code (e.g. `"en"`, `"fr"`).
    async fn search(&self, query: &str, language: Option<&str>) -> Result<Vec<DownloadCandidate>, Error>;

    /// Download the file identified by `external_id` (Anna's Archive: MD5
    /// hash).
    async fn fetch(&self, external_id: &str) -> Result<DownloadedFile, Error>;
}
