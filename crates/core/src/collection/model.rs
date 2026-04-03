use rust_decimal::Decimal;

use crate::book::IdentifierType;

/// Edits submitted by the user during the import review step or when editing
/// an existing library book.
///
/// Carries all mutable book fields. `CollectionService::approve_book` commits
/// these to the database and transitions the book to `Available`;
/// `CollectionService::edit_book` applies them to an already-available book.
#[derive(Debug, Clone)]
pub struct BookEdit {
    pub title: String,
    pub description: Option<String>,
    pub published_date: Option<i32>,
    pub language: Option<String>,
    pub series_name: Option<String>,
    pub series_number: Option<Decimal>,
    pub publisher_name: Option<String>,
    pub page_count: Option<i32>,
    /// Primary authors in display order (comma-separated in UI, split before
    /// submission).
    pub authors: Vec<String>,
    /// Identifiers keyed by type; duplicates within the same type are ignored.
    pub identifiers: Vec<(IdentifierType, String)>,
    /// If `true`, the cover fetched by `fetch_from_provider` replaces the
    /// existing cover. The bytes are held in the server-side temp store keyed
    /// by the cover key passed to `fetch_from_provider`; no bytes are
    /// round-tripped through this struct.
    pub use_fetched_cover: bool,
    /// Genre names to assign to this book (find-or-create on save).
    pub genres: Vec<String>,
    /// Tag names to assign to this book (find-or-create on save).
    pub tags: Vec<String>,
}
