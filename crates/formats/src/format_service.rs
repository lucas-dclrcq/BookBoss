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
            _ => None,
        }
    }

    async fn extract_metadata(&self, path: &Path) -> Result<(FileFormat, ExtractedMetadata), Error> {
        let format = self
            .detect_format(path)
            .ok_or_else(|| Error::Infrastructure(format!("unrecognised file format: {}", path.display())))?;

        let extracted = match format {
            FileFormat::Epub => {
                let path = path.to_path_buf();
                tokio::task::spawn_blocking(move || crate::epub::extract_epub_metadata(&path))
                    .await
                    .map_err(|e| Error::Infrastructure(e.to_string()))??
            }
            // Unsupported formats return empty metadata — the provider
            // enrichment step fills the gaps.
            _ => ExtractedMetadata::default(),
        };

        Ok((format, extracted))
    }

    async fn enrich(&self, request: &EnrichmentRequest) -> Result<(), Error> {
        let has_epub_output = request.outputs.iter().any(|o| o.format == FileFormat::Epub);
        let kepub_output = request.outputs.iter().find(|o| o.format == FileFormat::Kepub);

        // Kepub requires an epub source.
        if kepub_output.is_some() && request.source.format != FileFormat::Epub {
            return Err(Error::Infrastructure("KEPUB output requires an EPUB source".to_string()));
        }

        // Step 1: Enrich epub if requested.
        let epub_path_for_kepub;
        if has_epub_output {
            let epub_output = request.outputs.iter().find(|o| o.format == FileFormat::Epub).expect("checked above");

            let source = request.source.path.clone();
            let dest = epub_output.path.clone();
            let sidecar = request.sidecar.clone();
            let cover_bytes = read_cover_bytes(request.cover_path.as_ref()).await?;

            tokio::task::spawn_blocking(move || crate::epub_enrich::enrich_epub(&source, &dest, &sidecar, cover_bytes.as_deref()))
                .await
                .map_err(|e| Error::Infrastructure(format!("enrichment task panicked: {e}")))?
                .map_err(|e| Error::Infrastructure(e.to_string()))?;

            epub_path_for_kepub = epub_output.path.clone();
        } else {
            // No epub output — kepub derives from source directly.
            epub_path_for_kepub = request.source.path.clone();
        }

        // Step 2: Convert to kepub if requested.
        if let Some(kepub) = kepub_output {
            let dest = kepub.path.clone();
            let source = epub_path_for_kepub;

            tokio::task::spawn_blocking(move || crate::kepub_convert::convert_to_kepub(&source, &dest))
                .await
                .map_err(|e| Error::Infrastructure(format!("kepub conversion task panicked: {e}")))?
                .map_err(|e| Error::Infrastructure(e.to_string()))?;
        }

        Ok(())
    }

    async fn write_sidecar(&self, path: &Path, sidecar: &BookSidecar) -> Result<(), Error> {
        let bytes = crate::opf::write_sidecar(sidecar).map_err(|e| Error::Infrastructure(e.to_string()))?;

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| Error::Infrastructure(format!("create sidecar dir: {e}")))?;
        }

        tokio::fs::write(path, bytes)
            .await
            .map_err(|e| Error::Infrastructure(format!("write sidecar: {e}")))?;

        Ok(())
    }

    async fn read_sidecar(&self, path: &Path) -> Result<BookSidecar, Error> {
        let bytes = tokio::fs::read(path).await.map_err(|e| Error::Infrastructure(format!("read sidecar: {e}")))?;

        crate::opf::parse_sidecar(&bytes).map_err(|e| Error::Infrastructure(e.to_string()))
    }

    async fn read_raw_opf(&self, path: &Path) -> Result<Option<String>, Error> {
        if self.detect_format(path) != Some(FileFormat::Epub) {
            return Ok(None);
        }
        let path = path.to_path_buf();
        let xml = tokio::task::spawn_blocking(move || crate::epub::read_epub_opf_xml(&path))
            .await
            .map_err(|e| Error::Infrastructure(format!("read_raw_opf task panicked: {e}")))?
            .map_err(|e| Error::Infrastructure(e.to_string()))?;
        Ok(Some(xml))
    }
}

/// Read cover bytes from an optional path. Returns `None` if no path given or
/// file not found (non-fatal).
async fn read_cover_bytes(cover_path: Option<&std::path::PathBuf>) -> Result<Option<Vec<u8>>, Error> {
    let Some(path) = cover_path else {
        return Ok(None);
    };
    match tokio::fs::read(path).await {
        Ok(data) => Ok(Some(data)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::warn!(path = %path.display(), "cover file not found, enriching without cover");
            Ok(None)
        }
        Err(e) => Err(Error::Infrastructure(format!("read cover: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use bb_core::format::EBookFile;

    use super::*;
    use crate::test_support::{build_test_epub, make_sidecar};

    #[test]
    fn detect_epub_is_supported() {
        let svc = FormatServiceImpl;
        assert_eq!(svc.detect_format(Path::new("book.epub")), Some(FileFormat::Epub));
    }

    #[test]
    fn detect_non_epub_returns_none() {
        let svc = FormatServiceImpl;
        // Known e-book formats not yet supported for import
        assert_eq!(svc.detect_format(Path::new("book.mobi")), None);
        assert_eq!(svc.detect_format(Path::new("book.pdf")), None);
        assert_eq!(svc.detect_format(Path::new("book.cbz")), None);
        assert_eq!(svc.detect_format(Path::new("book.azw3")), None);
        // Truly unrecognised extensions
        assert_eq!(svc.detect_format(Path::new("book.txt")), None);
        assert_eq!(svc.detect_format(Path::new("book.zip")), None);
        assert_eq!(svc.detect_format(Path::new("no_extension")), None);
    }

    #[tokio::test]
    async fn extract_metadata_unrecognised_format() {
        let svc = FormatServiceImpl;
        svc.extract_metadata(Path::new("book.txt")).await.unwrap_err();
    }

    #[tokio::test]
    async fn extract_metadata_epub() {
        let epub_bytes = build_test_epub();
        let path = std::env::temp_dir().join("format_svc_extract_test.epub");
        std::fs::write(&path, &epub_bytes).unwrap();

        let svc = FormatServiceImpl;
        let (format, meta) = svc.extract_metadata(&path).await.unwrap();

        assert_eq!(format, FileFormat::Epub);
        assert_eq!(meta.title.as_deref(), Some("Dune"));

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn enrich_epub_output() {
        let epub_bytes = build_test_epub();
        let source_path = std::env::temp_dir().join("format_svc_enrich_source.epub");
        std::fs::write(&source_path, &epub_bytes).unwrap();

        let dest_path = std::env::temp_dir().join("format_svc_enrich_dest.epub");

        let svc = FormatServiceImpl;
        let request = EnrichmentRequest {
            source: EBookFile {
                format: FileFormat::Epub,
                path: source_path.clone(),
            },
            sidecar: make_sidecar("Test Book"),
            cover_path: None,
            outputs: vec![EBookFile {
                format: FileFormat::Epub,
                path: dest_path.clone(),
            }],
        };

        svc.enrich(&request).await.unwrap();
        assert!(dest_path.exists(), "enriched epub should be written");

        let _ = std::fs::remove_file(&source_path);
        let _ = std::fs::remove_file(&dest_path);
    }

    #[tokio::test]
    async fn enrich_epub_and_kepub() {
        let epub_bytes = build_test_epub();
        let source_path = std::env::temp_dir().join("format_svc_enrich_ek_source.epub");
        std::fs::write(&source_path, &epub_bytes).unwrap();

        let epub_dest = std::env::temp_dir().join("format_svc_enrich_ek_epub.epub");
        let kepub_dest = std::env::temp_dir().join("format_svc_enrich_ek.kepub.epub");

        let svc = FormatServiceImpl;
        let request = EnrichmentRequest {
            source: EBookFile {
                format: FileFormat::Epub,
                path: source_path.clone(),
            },
            sidecar: make_sidecar("Test Book"),
            cover_path: None,
            outputs: vec![
                EBookFile {
                    format: FileFormat::Epub,
                    path: epub_dest.clone(),
                },
                EBookFile {
                    format: FileFormat::Kepub,
                    path: kepub_dest.clone(),
                },
            ],
        };

        svc.enrich(&request).await.unwrap();
        assert!(epub_dest.exists(), "enriched epub should be written");
        assert!(kepub_dest.exists(), "kepub should be written");

        let _ = std::fs::remove_file(&source_path);
        let _ = std::fs::remove_file(&epub_dest);
        let _ = std::fs::remove_file(&kepub_dest);
    }

    #[tokio::test]
    async fn enrich_kepub_only_from_source() {
        let epub_bytes = build_test_epub();
        let source_path = std::env::temp_dir().join("format_svc_enrich_kepub_only_source.epub");
        std::fs::write(&source_path, &epub_bytes).unwrap();

        let kepub_dest = std::env::temp_dir().join("format_svc_enrich_kepub_only.kepub.epub");

        let svc = FormatServiceImpl;
        let request = EnrichmentRequest {
            source: EBookFile {
                format: FileFormat::Epub,
                path: source_path.clone(),
            },
            sidecar: make_sidecar("Test Book"),
            cover_path: None,
            outputs: vec![EBookFile {
                format: FileFormat::Kepub,
                path: kepub_dest.clone(),
            }],
        };

        svc.enrich(&request).await.unwrap();
        assert!(kepub_dest.exists(), "kepub should be written from source");

        let _ = std::fs::remove_file(&source_path);
        let _ = std::fs::remove_file(&kepub_dest);
    }

    #[tokio::test]
    async fn enrich_rejects_kepub_without_epub_source() {
        let svc = FormatServiceImpl;
        let request = EnrichmentRequest {
            source: EBookFile {
                format: FileFormat::Pdf,
                path: "/tmp/book.pdf".into(),
            },
            sidecar: make_sidecar("Test Book"),
            cover_path: None,
            outputs: vec![EBookFile {
                format: FileFormat::Kepub,
                path: "/tmp/out.kepub.epub".into(),
            }],
        };

        let err = svc.enrich(&request).await.unwrap_err();
        assert!(err.to_string().contains("KEPUB output requires an EPUB source"));
    }

    #[tokio::test]
    async fn sidecar_roundtrip() {
        let svc = FormatServiceImpl;
        let sidecar = make_sidecar("Test Book");

        let path = std::env::temp_dir().join("format_svc_sidecar_test.opf");
        svc.write_sidecar(&path, &sidecar).await.unwrap();
        assert!(path.exists());

        let parsed = svc.read_sidecar(&path).await.unwrap();
        assert_eq!(parsed.title, sidecar.title);

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn read_raw_opf_epub_returns_some() {
        let epub_bytes = build_test_epub();
        let path = std::env::temp_dir().join("fmt_svc_read_raw_opf.epub");
        std::fs::write(&path, &epub_bytes).unwrap();
        let svc = FormatServiceImpl;
        let result = svc.read_raw_opf(&path).await.unwrap();
        assert!(result.is_some());
        let xml = result.unwrap();
        assert!(xml.contains("<metadata"));
        assert!(xml.contains("Dune"));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn read_raw_opf_nonepub_returns_none() {
        let svc = FormatServiceImpl;
        let result = svc.read_raw_opf(std::path::Path::new("/tmp/book.pdf")).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn extract_metadata_missing_file() {
        let svc = FormatServiceImpl;
        let result = svc.extract_metadata(std::path::Path::new("/tmp/nonexistent_bookboss_test.epub")).await;
        result.unwrap_err();
    }
}
