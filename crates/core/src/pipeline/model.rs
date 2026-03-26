use rust_decimal::Decimal;

use crate::{
    book::{AuthorRole, IdentifierType},
    import::ImportSource,
};

#[derive(Debug, Clone)]
pub struct ExtractedAuthor {
    pub name: String,
    pub role: Option<AuthorRole>,
    pub sort_order: i32,
}

#[derive(Debug, Clone)]
pub struct ExtractedIdentifier {
    pub identifier_type: IdentifierType,
    pub value: String,
}

/// Metadata extracted directly from an e-book file's embedded headers or OPF.
///
/// All fields are `Option` — embedded metadata is frequently incomplete.
#[derive(Debug, Clone, Default)]
pub struct ExtractedMetadata {
    pub title: Option<String>,
    pub authors: Option<Vec<ExtractedAuthor>>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    /// Publication year.
    pub published_date: Option<i32>,
    pub language: Option<String>,
    pub identifiers: Option<Vec<ExtractedIdentifier>>,
    pub series_name: Option<String>,
    pub series_number: Option<Decimal>,
    /// Genres extracted from `dc:subject` elements or the `spinnaker:metadata`
    /// metadata blob.
    pub genres: Vec<String>,
    /// Tags extracted from the `spinnaker:metadata` metadata blob.
    pub tags: Vec<String>,
    /// Page count extracted from the `spinnaker:metadata` metadata blob.
    pub page_count: Option<i32>,
    /// `true` when a `spinnaker:metadata` blob was found in the OPF.
    ///
    /// Used by the pipeline to skip external metadata providers — if the file
    /// was already enriched by BookBoss the embedded metadata is authoritative.
    pub has_spinnaker_metadata: bool,
    /// Cover image bytes extracted directly from the e-book file, if present.
    pub cover_bytes: Option<Vec<u8>>,
}

/// Enriched metadata returned by an external metadata provider.
///
/// Wraps [`ExtractedMetadata`] and adds cover art bytes fetched by the
/// provider. `source` identifies which provider produced the result so the
/// pipeline can record provenance without needing a separate construction-time
/// hint. The pipeline never makes HTTP calls directly.
#[derive(Debug, Clone)]
pub struct ProviderBook {
    pub metadata: ExtractedMetadata,
    pub cover_bytes: Option<Vec<u8>>,
    pub source: ImportSource,
}
