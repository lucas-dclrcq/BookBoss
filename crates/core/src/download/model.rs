//! Domain models for the direct-download feature.

/// A single search result from a download provider (e.g. Anna's Archive),
/// already filtered to a downloadable ebook format (EPUB).
#[derive(Debug, Clone, PartialEq)]
pub struct DownloadCandidate {
    /// Provider-specific identifier used to fetch the file. For Anna's Archive
    /// this is the MD5 hash of the record.
    pub external_id: String,
    pub title: String,
    pub authors: String,
    pub publisher: Option<String>,
    pub language: Option<String>,
    pub format: String,
    pub size: Option<String>,
}

/// A downloaded file, ready to be handed to the import pipeline via
/// [`ImportJobService::queue_bytes_if_new`](crate::import::ImportJobService::queue_bytes_if_new).
#[derive(Debug, Clone)]
pub struct DownloadedFile {
    pub filename: String,
    pub bytes: Vec<u8>,
}
