use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(super) struct OlAuthor {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct OlPublisher {
    pub name: String,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct OlCover {
    pub large: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct OlIdentifiers {
    pub openlibrary: Option<Vec<String>>,
}

/// Top-level response from `/search.json`.
#[derive(Debug, Deserialize)]
pub(super) struct OlSearchResponse {
    pub docs: Vec<OlSearchDoc>,
}

/// A single result document from `/search.json`.
///
/// Field completeness varies; all are optional.
#[derive(Debug, Deserialize)]
pub(super) struct OlSearchDoc {
    pub title: Option<String>,
    /// Flat list of author name strings (not objects).
    pub author_name: Option<Vec<String>>,
    /// All ISBNs associated with the work (mix of ISBN-10 and ISBN-13).
    pub isbn: Option<Vec<String>>,
    pub first_publish_year: Option<i32>,
    pub publisher: Option<Vec<String>>,
    /// Cover image ID — used to construct the cover URL.
    pub cover_i: Option<i64>,
}

/// Subset of the Open Library Books API response (`jscmd=data`) used by the
/// adapter.
///
/// All fields are optional — OL record completeness varies widely.
#[derive(Debug, Deserialize)]
pub(super) struct OlBookData {
    pub title: Option<String>,
    pub authors: Option<Vec<OlAuthor>>,
    pub publishers: Option<Vec<OlPublisher>>,
    pub publish_date: Option<String>,
    pub cover: Option<OlCover>,
    pub identifiers: Option<OlIdentifiers>,
    pub subjects: Option<Vec<OlSubject>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OlSubject {
    pub name: String,
}
