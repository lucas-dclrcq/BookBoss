mod annas_archive;

use std::sync::Arc;

pub use annas_archive::AnnasArchiveAdapter;
use serde::Deserialize;

/// Default HTTP request timeout for the download client. Generous enough to
/// pull a multi-megabyte EPUB over a slow mirror, but bounded so a stuck
/// download cannot block the single job worker forever.
pub(crate) const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

/// Default Anna's Archive mirror used when none is configured.
pub(crate) const MIRROR_DEFAULT: &str = "https://annas-archive.org";

/// Configuration for the Anna's Archive download source.
///
/// Populated from `BOOKBOSS__ANNAS_ARCHIVE__*` environment variables. The
/// feature stays disabled unless `enabled` is true *and* an `api_key` is set
/// (a donation-backed API key is required for the fast-download JSON API).
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct AnnasArchiveConfig {
    /// Master on/off switch for the direct-download feature.
    pub enabled: bool,
    /// Base URL of the Anna's Archive mirror. Accepts a full URL
    /// (`https://annas-archive.org`) or a bare domain. Defaults to
    /// [`MIRROR_DEFAULT`] when unset.
    pub mirror_url: Option<String>,
    /// Secret API key granting access to the fast-download JSON API.
    pub api_key: Option<String>,
}

/// Register the Anna's Archive download provider into the core registry.
///
/// Called once at startup after `CoreServices` is built. The provider is only
/// registered when the feature is enabled and an API key is present; otherwise
/// the registry stays empty and the feature is inert (UI hidden, requests
/// rejected).
pub fn before_start(core: &Arc<bb_core::CoreServices>, config: &AnnasArchiveConfig) {
    if !config.enabled {
        return;
    }
    let Some(api_key) = config.api_key.clone().filter(|k| !k.trim().is_empty()) else {
        tracing::warn!("Anna's Archive download is enabled but BOOKBOSS__ANNAS_ARCHIVE__API_KEY is not set — feature disabled");
        return;
    };
    let adapter = AnnasArchiveAdapter::new(config.mirror_url.as_deref(), api_key);
    tracing::info!(mirror = %adapter.base_url(), "registered Anna's Archive download source");
    core.download_source_service.register(Arc::new(adapter));
}
