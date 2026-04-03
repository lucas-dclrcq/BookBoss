use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// All identifiers are represented as a map from `IdentifierType` serde name
/// (e.g. `"Isbn13"`, `"Hardcover"`) to value string.
pub(crate) type IdentifierMap = HashMap<String, String>;

/// All data needed to populate the review page on initial load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct BookReviewData {
    pub job_token: String,
    pub book_token: String,
    pub updated_at: String,
    pub title: String,
    pub description: String,
    pub published_date: String,
    pub language: String,
    pub series_name: String,
    pub series_number: String,
    pub publisher_name: String,
    pub page_count: String,
    /// Author names in sort order.
    pub authors: Vec<String>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub identifiers: IdentifierMap,
    /// Provider names in priority order (for rendering provider buttons).
    pub provider_names: Vec<String>,
    /// Pixel dimensions (width, height) of the stored cover, if any.
    pub cover_dimensions: Option<(u32, u32)>,
    /// True when the original source file is missing from disk.
    /// When set, the book cannot be approved — only rejection is available.
    pub original_missing: bool,
}

/// Metadata returned by a single provider fetch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ProviderResult {
    pub title: String,
    pub description: String,
    pub published_date: String,
    pub language: String,
    pub series_name: String,
    pub series_number: String,
    pub publisher_name: String,
    pub page_count: String,
    pub authors: Vec<String>,
    pub identifiers: IdentifierMap,
    /// Base64 encoded cover from the provider, if any.
    pub cover_thumbnail: Option<String>,
    /// Pixel dimensions (width, height) of the provider cover, if any.
    pub cover_dimensions: Option<(u32, u32)>,
}

/// All edit fields submitted to the server on approval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct BookEditFields {
    pub job_token: String,
    pub title: String,
    pub description: String,
    pub published_date: String,
    pub language: String,
    pub series_name: String,
    pub series_number: String,
    pub publisher_name: String,
    pub page_count: String,
    pub authors: Vec<String>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub identifiers: IdentifierMap,
    pub use_fetched_cover: bool,
}

/// Bulk edit fields — all optional. `None` = don't change, `Some` = replace
/// (even if the value is empty, meaning "clear this field on all books").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct BulkEditFields {
    pub authors: Option<Vec<String>>,
    pub publisher: Option<String>,
    pub language: Option<String>,
    pub series_name: Option<String>,
    pub genres: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    /// Non-system library tokens to assign to all selected books. `None` = leave
    /// memberships unchanged.
    pub library_tokens: Option<Vec<String>>,
}

/// Picklist option for a series (name + suggested next book number).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SeriesOption {
    pub name: String,
    pub next_number: u32,
}

/// All data needed to populate pick lists on the metadata edit page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PicklistData {
    pub authors: Vec<String>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub series: Vec<SeriesOption>,
    pub publishers: Vec<String>,
}
