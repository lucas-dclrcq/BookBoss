//! Shared test fixtures for the formats crate.
use std::io::Write as _;

use bb_core::{
    book::{BookStatus, FileFormat},
    storage::{BookSidecar, SidecarFile},
};

pub(crate) const CONTAINER_XML: &[u8] = br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;

pub(crate) const CONTENT_OPF: &[u8] = br#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/"
            xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:title>Dune</dc:title>
    <dc:creator opf:role="aut" opf:file-as="Herbert, Frank">Frank Herbert</dc:creator>
  </metadata>
  <manifest/>
  <spine/>
</package>"#;

pub(crate) fn build_test_epub() -> Vec<u8> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(cursor);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("mimetype", opts).unwrap();
    zip.write_all(b"application/epub+zip").unwrap();
    zip.start_file("META-INF/container.xml", opts).unwrap();
    zip.write_all(CONTAINER_XML).unwrap();
    zip.start_file("content.opf", opts).unwrap();
    zip.write_all(CONTENT_OPF).unwrap();
    zip.finish().unwrap().into_inner()
}

pub(crate) fn make_sidecar(title: &str) -> BookSidecar {
    BookSidecar {
        title: title.to_string(),
        authors: vec![],
        description: None,
        publisher: None,
        published_date: None,
        language: None,
        identifiers: vec![],
        series: None,
        genres: vec![],
        tags: vec![],
        page_count: None,
        status: BookStatus::Available,
        metadata_source: None,
        files: vec![SidecarFile {
            format: FileFormat::Epub,
            hash: "abc".to_string(),
        }],
    }
}
