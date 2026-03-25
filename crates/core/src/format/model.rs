use std::path::PathBuf;

use crate::{book::FileFormat, storage::BookSidecar};

/// A book file identified by its format and absolute path.
#[derive(Debug, Clone)]
pub struct EBookFile {
    pub format: FileFormat,
    pub path: PathBuf,
}

/// Request to enrich book files: embed sidecar metadata and optional cover
/// image into the source file, writing outputs to the requested destinations.
///
/// The `outputs` vec controls what gets produced:
/// - Source=Epub, Outputs=\[Epub, Kepub\] — enrich epub, then derive kepub
///   (normal case)
/// - Source=Epub, Outputs=\[Epub\] — enrich epub only, no kepub
/// - Source=Epub, Outputs=\[Kepub\] — derive kepub directly from source epub
#[derive(Debug, Clone)]
pub struct EnrichmentRequest {
    /// The original book file to enrich.
    pub source: EBookFile,
    /// Metadata to embed in the output files.
    pub sidecar: BookSidecar,
    /// Optional cover image to embed.
    pub cover_path: Option<PathBuf>,
    /// Requested output formats and their destination paths.
    pub outputs: Vec<EBookFile>,
}
