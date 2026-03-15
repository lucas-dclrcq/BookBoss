//! Kobo server configuration and URL helpers.
//!
//! [`KoboConfig`] is registered as an `Arc<KoboConfig>` axum [`Extension`] so
//! that server functions can build Kobo-protocol URLs without knowing the
//! internal storage layout.

use std::sync::Arc;

/// Kobo-specific server configuration.
///
/// Registered as `Arc<KoboConfig>` in the axum extension layer so it is
/// available to both axum handlers and Dioxus server functions.
pub struct KoboConfig {
    pub base_url: String,
}

impl KoboConfig {
    pub fn new(base_url: impl Into<String>) -> Arc<Self> {
        Arc::new(Self { base_url: base_url.into() })
    }

    fn base(&self) -> &str {
        self.base_url.trim_end_matches('/')
    }

    /// Full sync URL for a device: the root the Kobo firmware uses as its
    /// API base during the sync session.
    ///
    /// `sync_token` is the device token with the `DV_` prefix stripped.
    pub fn sync_url(&self, sync_token: &str) -> String {
        format!("{}/kobo/{sync_token}", self.base())
    }
}
