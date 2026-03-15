use std::{
    io::{Read, Write},
    path::Path,
};

use bb_core::storage::BookSidecar;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{
    Error,
    epub::{find_opf_path, resolve_zip_path},
    opf::{extract_cover_href, write_metadata_xml},
};

/// Produces an enriched copy of an EPUB file at `dest`.
///
/// - Rewrites the embedded OPF metadata from `sidecar`.
/// - If `cover` is `Some`: replaces the existing cover image, or adds a new
///   `cover.jpg` entry (with EPUB3 manifest declaration) if none was present.
/// - All other entries are copied verbatim, preserving their compression.
/// - `mimetype` is always written first as an uncompressed entry, as required
///   by the EPUB spec.
pub fn enrich_epub(source: &Path, dest: &Path, sidecar: &BookSidecar, cover: Option<&[u8]>) -> Result<(), Error> {
    let src_file = std::fs::File::open(source)?;
    let mut src = ZipArchive::new(src_file)?;

    // ── 1. Locate OPF and existing cover within the archive ─────────────────

    let opf_path = {
        let mut c = src.by_name("META-INF/container.xml")?;
        let mut buf = Vec::new();
        c.read_to_end(&mut buf)?;
        find_opf_path(&buf)?
    };

    let opf_dir = match opf_path.rfind('/') {
        Some(pos) => opf_path[..pos].to_string(),
        None => String::new(),
    };

    // Path within the ZIP of the existing cover image, if any.
    let existing_cover_zip_path: Option<String> = {
        let mut f = src.by_name(&opf_path)?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        extract_cover_href(&buf).map(|href| resolve_zip_path(&opf_dir, &href))
    };

    // ── 2. Build updated OPF (splice new metadata; optionally inject cover) ─

    let new_opf: String = {
        let mut f = src.by_name(&opf_path)?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        let opf_str = std::str::from_utf8(&buf)?;
        let mut updated = replace_opf_metadata(opf_str, sidecar)?;
        if cover.is_some() && existing_cover_zip_path.is_none() {
            updated = inject_cover_manifest_entry(&updated, "cover.jpg")?;
        }
        updated
    };

    // If we're adding a cover that didn't exist before, this is its ZIP path.
    let new_cover_zip_path: Option<String> = if cover.is_some() && existing_cover_zip_path.is_none() {
        Some(resolve_zip_path(&opf_dir, "cover.jpg"))
    } else {
        None
    };

    // ── 3. Enumerate source entries ──────────────────────────────────────────

    let entry_count = src.len();
    let mut entries: Vec<(usize, String)> = Vec::with_capacity(entry_count);
    for i in 0..entry_count {
        let f = src.by_index(i)?;
        entries.push((i, f.name().to_string()));
    }

    // ── 4. Write destination archive ─────────────────────────────────────────

    let dest_file = std::fs::File::create(dest)?;
    let mut dst = ZipWriter::new(std::io::BufWriter::new(dest_file));

    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    // `mimetype` must be the very first entry and must be uncompressed.
    if let Some(&(idx, _)) = entries.iter().find(|(_, n)| n == "mimetype") {
        let mut f = src.by_index(idx)?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        dst.start_file("mimetype", stored)?;
        dst.write_all(&buf)?;
    }

    for (i, name) in &entries {
        if name == "mimetype" {
            continue; // already written first
        }

        if name == &opf_path {
            dst.start_file(name, deflated)?;
            dst.write_all(new_opf.as_bytes())?;
        } else if Some(name) == existing_cover_zip_path.as_ref() {
            // Replace or preserve existing cover entry.
            if let Some(cover_bytes) = cover {
                dst.start_file(name, stored)?;
                dst.write_all(cover_bytes)?;
            } else {
                copy_entry(&mut src, &mut dst, *i)?;
            }
        } else {
            copy_entry(&mut src, &mut dst, *i)?;
        }
    }

    // If we're adding a cover to an EPUB that had none, write the new entry.
    if let (Some(new_path), Some(cover_bytes)) = (&new_cover_zip_path, cover) {
        dst.start_file(new_path, stored)?;
        dst.write_all(cover_bytes)?;
    }

    let mut buf_writer = dst.finish()?;
    buf_writer.flush()?;

    Ok(())
}

/// Copy a single ZIP entry (by index) from `src` to `dst`, preserving the
/// original compression method.
fn copy_entry<R, W>(src: &mut ZipArchive<R>, dst: &mut ZipWriter<W>, index: usize) -> Result<(), Error>
where
    R: Read + std::io::Seek,
    W: Write + std::io::Seek,
{
    let mut f = src.by_index(index)?;
    let method = f.compression();
    let name = f.name().to_string();
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    let opts = SimpleFileOptions::default().compression_method(method);
    dst.start_file(name, opts)?;
    dst.write_all(&buf)?;
    Ok(())
}

/// Replace the `<metadata>…</metadata>` block in `opf_xml` with freshly
/// serialised metadata from `sidecar`. Manifest, spine, and all other
/// elements are left untouched.
fn replace_opf_metadata(opf_xml: &str, sidecar: &BookSidecar) -> Result<String, Error> {
    let new_meta = write_metadata_xml(sidecar)?;
    let new_meta_str = std::str::from_utf8(&new_meta)?;

    let start = opf_xml
        .find("<metadata")
        .ok_or_else(|| Error::InvalidValue("OPF: no <metadata> element".into()))?;
    let end_tag = "</metadata>";
    let end = opf_xml
        .find(end_tag)
        .ok_or_else(|| Error::InvalidValue("OPF: no </metadata> closing tag".into()))?;

    let mut result = String::with_capacity(opf_xml.len() + new_meta_str.len());
    result.push_str(&opf_xml[..start]);
    result.push_str(new_meta_str);
    result.push_str(&opf_xml[end + end_tag.len()..]);
    Ok(result)
}

/// Inject a cover image manifest entry into an OPF's `<manifest>` block.
///
/// Handles both `<manifest>…</manifest>` (inserts before the closing tag) and
/// `<manifest/>` (self-closing empty manifest — expands it).
fn inject_cover_manifest_entry(opf_xml: &str, cover_href: &str) -> Result<String, Error> {
    let cover_item = format!(r#"<item id="cover-image" href="{cover_href}" media-type="image/jpeg" properties="cover-image"/>"#);

    if let Some(close_pos) = opf_xml.find("</manifest>") {
        let mut result = String::with_capacity(opf_xml.len() + cover_item.len());
        result.push_str(&opf_xml[..close_pos]);
        result.push_str(&cover_item);
        result.push_str(&opf_xml[close_pos..]);
        return Ok(result);
    }

    if opf_xml.contains("<manifest/>") {
        let replacement = format!("<manifest>{cover_item}</manifest>");
        return Ok(opf_xml.replacen("<manifest/>", &replacement, 1));
    }

    Err(Error::InvalidValue("OPF: no <manifest> element found".into()))
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};

    use bb_core::{
        book::{BookStatus, FileFormat},
        storage::{BookSidecar, SidecarFile},
    };
    use tempfile::tempdir;

    use super::enrich_epub;
    use crate::opf::parse_sidecar;

    const CONTAINER_XML: &[u8] = br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;

    const FAKE_JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F'];

    fn make_sidecar(title: &str) -> BookSidecar {
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
            rating: None,
            status: BookStatus::Available,
            metadata_source: None,
            files: vec![SidecarFile {
                format: FileFormat::Epub,
                hash: "abc".to_string(),
            }],
        }
    }

    fn build_epub_no_cover(title: &str) -> Vec<u8> {
        let opf = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:title>{title}</dc:title>
  </metadata>
  <manifest/>
  <spine/>
</package>"#
        );
        build_epub_zip(&opf, None)
    }

    fn build_epub2_with_cover(title: &str) -> Vec<u8> {
        let opf = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:title>{title}</dc:title>
    <meta name="cover" content="cover-img"/>
  </metadata>
  <manifest>
    <item id="cover-img" href="cover.jpg" media-type="image/jpeg"/>
  </manifest>
  <spine/>
</package>"#
        );
        build_epub_zip(&opf, Some(("cover.jpg", FAKE_JPEG)))
    }

    fn build_epub3_with_cover(title: &str) -> Vec<u8> {
        let opf = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>{title}</dc:title>
  </metadata>
  <manifest>
    <item id="cover" href="images/cover.jpg" media-type="image/jpeg" properties="cover-image"/>
  </manifest>
  <spine/>
</package>"#
        );
        build_epub_zip(&opf, Some(("images/cover.jpg", FAKE_JPEG)))
    }

    fn build_epub_zip(opf: &str, cover: Option<(&str, &[u8])>) -> Vec<u8> {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(cursor);
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

        zip.start_file("mimetype", stored).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();

        zip.start_file("META-INF/container.xml", stored).unwrap();
        zip.write_all(CONTAINER_XML).unwrap();

        zip.start_file("content.opf", stored).unwrap();
        zip.write_all(opf.as_bytes()).unwrap();

        if let Some((path, bytes)) = cover {
            zip.start_file(path, stored).unwrap();
            zip.write_all(bytes).unwrap();
        }

        zip.finish().unwrap().into_inner()
    }

    const NEW_COVER: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'N', b'E', b'W', b'!'];

    fn read_opf_from_epub(epub_path: &std::path::Path) -> Vec<u8> {
        let file = std::fs::File::open(epub_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let container_xml = {
            let mut c = archive.by_name("META-INF/container.xml").unwrap();
            let mut buf = Vec::new();
            c.read_to_end(&mut buf).unwrap();
            buf
        };
        let opf_path = crate::epub::find_opf_path(&container_xml).unwrap();
        let mut opf_entry = archive.by_name(&opf_path).unwrap();
        let mut buf = Vec::new();
        opf_entry.read_to_end(&mut buf).unwrap();
        buf
    }

    #[test]
    fn rewrites_metadata_no_cover() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src.epub");
        let dst = dir.path().join("dst.epub");
        std::fs::write(&src, build_epub_no_cover("Old Title")).unwrap();

        let sidecar = make_sidecar("New Title");
        enrich_epub(&src, &dst, &sidecar, None).unwrap();

        let opf_bytes = read_opf_from_epub(&dst);
        let parsed = parse_sidecar(&opf_bytes).unwrap();
        assert_eq!(parsed.title, "New Title");
    }

    #[test]
    fn replaces_existing_epub2_cover() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src.epub");
        let dst = dir.path().join("dst.epub");
        std::fs::write(&src, build_epub2_with_cover("Dune")).unwrap();

        let sidecar = make_sidecar("Dune");
        enrich_epub(&src, &dst, &sidecar, Some(NEW_COVER)).unwrap();

        // Read cover entry from dst
        let file = std::fs::File::open(&dst).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut cover_entry = archive.by_name("cover.jpg").unwrap();
        let mut buf = Vec::new();
        cover_entry.read_to_end(&mut buf).unwrap();
        assert_eq!(buf, NEW_COVER);
    }

    #[test]
    fn replaces_existing_epub3_cover() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src.epub");
        let dst = dir.path().join("dst.epub");
        std::fs::write(&src, build_epub3_with_cover("Foundation")).unwrap();

        let sidecar = make_sidecar("Foundation");
        enrich_epub(&src, &dst, &sidecar, Some(NEW_COVER)).unwrap();

        let file = std::fs::File::open(&dst).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut cover_entry = archive.by_name("images/cover.jpg").unwrap();
        let mut buf = Vec::new();
        cover_entry.read_to_end(&mut buf).unwrap();
        assert_eq!(buf, NEW_COVER);
    }

    #[test]
    fn adds_cover_to_epub_without_one() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src.epub");
        let dst = dir.path().join("dst.epub");
        std::fs::write(&src, build_epub_no_cover("Neuromancer")).unwrap();

        let sidecar = make_sidecar("Neuromancer");
        enrich_epub(&src, &dst, &sidecar, Some(NEW_COVER)).unwrap();

        let file = std::fs::File::open(&dst).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();

        let cover_buf = {
            let mut cover_entry = archive.by_name("cover.jpg").unwrap();
            let mut buf = Vec::new();
            cover_entry.read_to_end(&mut buf).unwrap();
            buf
        };
        assert_eq!(cover_buf, NEW_COVER);

        // OPF should have cover-image manifest entry
        let opf_str = {
            let mut opf_entry = archive.by_name("content.opf").unwrap();
            let mut buf = Vec::new();
            opf_entry.read_to_end(&mut buf).unwrap();
            String::from_utf8(buf).unwrap()
        };
        assert!(opf_str.contains("cover-image"), "OPF should declare cover-image");
    }

    #[test]
    fn preserves_non_opf_entries() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src.epub");
        let dst = dir.path().join("dst.epub");

        // Build EPUB with an extra chapter file
        let opf = r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:title>Book</dc:title>
  </metadata>
  <manifest>
    <item id="ch1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="ch1"/></spine>
</package>"#;

        let cursor = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(cursor);
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("mimetype", stored).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();
        zip.start_file("META-INF/container.xml", stored).unwrap();
        zip.write_all(CONTAINER_XML).unwrap();
        zip.start_file("content.opf", stored).unwrap();
        zip.write_all(opf.as_bytes()).unwrap();
        zip.start_file("chapter1.xhtml", stored).unwrap();
        zip.write_all(b"<html><body>Hello</body></html>").unwrap();
        std::fs::write(&src, zip.finish().unwrap().into_inner()).unwrap();

        enrich_epub(&src, &dst, &make_sidecar("Book"), None).unwrap();

        let file = std::fs::File::open(&dst).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut ch = archive.by_name("chapter1.xhtml").unwrap();
        let mut buf = Vec::new();
        ch.read_to_end(&mut buf).unwrap();
        assert_eq!(buf, b"<html><body>Hello</body></html>");
    }

    /// Verify that genres in the sidecar appear as `dc:subject` elements in
    /// the enriched EPUB's OPF. Genres stored only in the `spinnaker:metadata` blob
    /// would be invisible to e-readers.
    #[test]
    fn genres_written_as_dc_subject_in_enriched_epub() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src.epub");
        let dst = dir.path().join("dst.epub");
        std::fs::write(&src, build_epub_no_cover("Test")).unwrap();

        let mut sidecar = make_sidecar("Test");
        sidecar.genres = vec!["Fantasy".to_string(), "Epic Fantasy".to_string()];

        enrich_epub(&src, &dst, &sidecar, None).unwrap();

        let opf_bytes = read_opf_from_epub(&dst);
        let opf_str = std::str::from_utf8(&opf_bytes).expect("utf8");

        for genre in &sidecar.genres {
            let expected = format!("<dc:subject>{genre}</dc:subject>");
            assert!(opf_str.contains(&expected), "expected {expected:?} in enriched EPUB OPF");
        }

        let parsed = parse_sidecar(&opf_bytes).expect("parse sidecar");
        assert_eq!(parsed.genres, sidecar.genres, "genres must roundtrip through enriched EPUB");
    }

    #[test]
    fn mimetype_is_first_entry() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src.epub");
        let dst = dir.path().join("dst.epub");
        std::fs::write(&src, build_epub_no_cover("Test")).unwrap();

        enrich_epub(&src, &dst, &make_sidecar("Test"), None).unwrap();

        let file = std::fs::File::open(&dst).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let first = archive.by_index(0).unwrap();
        assert_eq!(first.name(), "mimetype");
    }
}
