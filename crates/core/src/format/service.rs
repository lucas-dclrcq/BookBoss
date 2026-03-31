use std::path::Path;

use async_trait::async_trait;

use crate::{Error, book::FileFormat, format::EnrichmentRequest, pipeline::ExtractedMetadata, storage::BookSidecar};

/// Port trait for all e-book file format operations.
///
/// This is the sole interface to format-specific functionality (parsing,
/// enrichment, sidecar serialisation). Implementations live in the `formats`
/// adapter crate. Adding a new file format only requires changes there.
#[async_trait]
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
#[allow(unused_lifetimes, reason = "async_trait + mockall expansion emits a spurious 'life0 parameter")]
pub trait FormatService: Send + Sync {
    /// Detect the file format from a path's extension.
    /// Returns `None` for unrecognised formats.
    fn detect_format(&self, path: &Path) -> Option<FileFormat>;

    /// Extract metadata from a book file.
    ///
    /// The format is detected automatically from the file extension and
    /// returned alongside the extracted metadata. Returns an error if the
    /// format is unrecognised or the file cannot be parsed.
    async fn extract_metadata(&self, path: &Path) -> Result<(FileFormat, ExtractedMetadata), Error>;

    /// Enrich book files: embed sidecar metadata and optional cover image
    /// into the source file, writing outputs to the requested destinations.
    ///
    /// Processes epub first (if requested), then derives kepub from the
    /// enriched epub. Returns an error if kepub is requested without an
    /// epub source.
    async fn enrich(&self, request: &EnrichmentRequest) -> Result<(), Error>;

    /// Write a `BookSidecar` to a file in OPF format.
    async fn write_sidecar(&self, path: &Path, sidecar: &BookSidecar) -> Result<(), Error>;

    /// Read a `BookSidecar` from an OPF file.
    async fn read_sidecar(&self, path: &Path) -> Result<BookSidecar, Error>;

    /// Read the raw OPF XML from a book file.
    /// Returns `Some(xml)` for EPUB, `None` for unrecognised formats.
    async fn read_raw_opf(&self, path: &Path) -> Result<Option<String>, Error>;
}
