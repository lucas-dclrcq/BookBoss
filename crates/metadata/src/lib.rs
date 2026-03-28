mod google_books;
mod hardcover;
mod open_library;

use std::sync::Arc;

/// Default HTTP request timeout for all metadata provider clients.
pub(crate) const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

use bb_core::metadata::MetadataService;
pub use google_books::GoogleBooksAdapter;
pub use hardcover::HardcoverAdapter;
pub use open_library::OpenLibraryAdapter;
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct MetadataConfig {
    pub hardcover_api_token: Option<String>,
    pub googlebooks_api_token: Option<String>,
}

/// Register configured metadata providers into the registry.
///
/// Called once at startup after `CoreServices` is built. Providers are
/// registered in priority order: Hardcover → Google Books →
/// Open Library (always the final fallback).
pub fn before_start(service: &dyn MetadataService, config: &MetadataConfig) {
    if let Some(token) = &config.hardcover_api_token {
        service.register(Arc::new(HardcoverAdapter::new(token.clone())));
    }
    if let Some(token) = &config.googlebooks_api_token {
        service.register(Arc::new(GoogleBooksAdapter::new(token.clone())));
    }
    service.register(Arc::new(OpenLibraryAdapter::new()));
}
