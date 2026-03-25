use std::path::Path;

use async_trait::async_trait;
use bb_core::{
    Error,
    book::FileFormat,
    format::{EnrichmentRequest, FormatService},
    pipeline::ExtractedMetadata,
    storage::BookSidecar,
};

/// Stateless implementation of [`FormatService`].
///
/// All format-specific logic (EPUB parsing, OPF sidecar serialisation,
/// enrichment, KEPUB conversion) is delegated to internal modules.
struct FormatServiceImpl;

/// Create a new [`FormatService`] implementation.
pub fn create_format_service() -> impl FormatService {
    FormatServiceImpl
}

#[async_trait]
impl FormatService for FormatServiceImpl {
    fn detect_format(&self, path: &Path) -> Option<FileFormat> {
        match path.extension()?.to_str()? {
            "epub" => Some(FileFormat::Epub),
            "mobi" => Some(FileFormat::Mobi),
            "pdf" => Some(FileFormat::Pdf),
            "cbz" => Some(FileFormat::Cbz),
            "azw3" => Some(FileFormat::Azw3),
            _ => None,
        }
    }

    async fn extract_metadata(&self, _path: &Path) -> Result<(FileFormat, ExtractedMetadata), Error> {
        todo!("Phase 2: dispatch to format-specific extractors")
    }

    async fn enrich(&self, _request: &EnrichmentRequest) -> Result<(), Error> {
        todo!("Phase 2: implement enrichment pipeline")
    }

    async fn write_sidecar(&self, _path: &Path, _sidecar: &BookSidecar) -> Result<(), Error> {
        todo!("Phase 2: implement sidecar write")
    }

    async fn read_sidecar(&self, _path: &Path) -> Result<BookSidecar, Error> {
        todo!("Phase 2: implement sidecar read")
    }
}
