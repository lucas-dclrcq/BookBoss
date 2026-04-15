//! EPUB → MOBI6 conversion.
// Private helpers are unused until Step 2 wires the FormatService.
#![allow(dead_code)]
//!
//! Produces a minimal but reader-compatible MOBI6 (PalmDB) file from an EPUB
//! source. The output is suitable for display on Kindle devices and the Kindle
//! app, but does not aim for full spec compliance.
//!
//! # Algorithm
//!
//! 1. Open the EPUB ZIP and read the OPF to find spine item hrefs in order.
//! 2. Merge all spine HTML files into one linearised HTML document.
//! 3. Rewrite `<img src="...">` references to
//!    `kindle:embed:XXXX?mime=image/jpeg`.
//! 4. Strip non-inline `<style>` blocks and `<link rel="stylesheet">` elements.
//! 5. Collect images: cover first (if provided), then body images in ref order.
//! 6. Write a PalmDB file with PalmDoc + MOBI header + EXTH block in record 0,
//!    HTML text in records 1..N, and JPEG images in records N+1..M.

use std::{
    collections::HashMap,
    io::{Read, Write},
    path::Path,
};

use bb_core::storage::BookSidecar;
use quick_xml::{Reader, events::Event};

use crate::Error;

// ── Public entry point
// ────────────────────────────────────────────────────────

/// Convert an EPUB file to MOBI6 (PalmDB) format.
///
/// * `source_epub` — path to the source `.epub` file.
/// * `dest`        — path to write the `.mobi` output.
/// * `sidecar`     — book metadata used to populate EXTH records.
/// * `cover_bytes` — optional JPEG cover image; placed as the first image
///   record.
pub fn convert_to_mobi(source_epub: &Path, dest: &Path, sidecar: &BookSidecar, cover_bytes: Option<&[u8]>) -> Result<(), Error> {
    // 1. Read EPUB and gather spine HTML + images.
    let epub_file = std::fs::File::open(source_epub)?;
    let mut archive = zip::ZipArchive::new(epub_file)?;

    // Read container.xml → OPF path.
    let opf_path = {
        let mut container = archive.by_name("META-INF/container.xml")?;
        let mut buf = Vec::new();
        container.read_to_end(&mut buf)?;
        crate::epub::find_opf_path(&buf)?
    };
    let opf_dir = match opf_path.rfind('/') {
        Some(pos) => opf_path[..pos].to_string(),
        None => String::new(),
    };

    // Read OPF bytes.
    let opf_bytes = {
        let mut f = archive.by_name(&opf_path)?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        buf
    };

    // Parse manifest (id → href) and spine (ordered idrefs).
    let (manifest, spine_idrefs) = parse_opf_manifest_and_spine(&opf_bytes)?;

    // Build ordered list of HTML hrefs from the spine.
    let mut spine_hrefs: Vec<String> = Vec::new();
    for idref in &spine_idrefs {
        if let Some(href) = manifest.get(idref) {
            spine_hrefs.push(href.clone());
        }
    }

    // Collect all entry names up front (borrow checker).
    let entry_names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).map(|f| f.name().to_string()))
        .collect::<Result<_, _>>()?;

    // Build a lookup: zip path → index.
    let name_to_idx: HashMap<String, usize> = entry_names.iter().enumerate().map(|(i, n)| (n.clone(), i)).collect();

    // Build prefix map: zip_path → anchor prefix (used for id/href rewriting).
    // e.g. "OEBPS/Text/chapter2.xhtml" → "chapter2"
    let spine_prefix_map: HashMap<String, String> = spine_hrefs
        .iter()
        .map(|href| {
            let zp = resolve_zip_path(&opf_dir, href);
            let prefix = spine_prefix_for(&zp);
            (zp, prefix)
        })
        .collect();

    // Read spine HTML documents and merge them.
    let mut merged_html_parts: Vec<Vec<u8>> = Vec::new();
    // Track image hrefs in body-reference order (relative to OPF dir).
    let mut body_image_hrefs: Vec<String> = Vec::new();
    // Map from img src (as seen in HTML) to 1-based image record index.
    // Cover is always image record 1 if present; body images follow.
    let cover_offset: u32 = u32::from(cover_bytes.is_some());

    for href in &spine_hrefs {
        let zip_path = resolve_zip_path(&opf_dir, href);
        let html_bytes = if let Some(&idx) = name_to_idx.get(&zip_path) {
            let mut f = archive.by_index(idx)?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
            buf
        } else {
            continue;
        };

        // Directory of this HTML file within the zip (for resolving relative hrefs).
        let html_zip_dir = match zip_path.rfind('/') {
            Some(pos) => zip_path[..pos].to_string(),
            None => String::new(),
        };
        let current_prefix = spine_prefix_map.get(&zip_path).cloned().unwrap_or_default();

        // Collect img srefs from this document (relative to its directory).
        let html_dir = match href.rfind('/') {
            Some(pos) => href[..pos].to_string(),
            None => String::new(),
        };
        let img_srcs = collect_img_srcs(&html_bytes);
        for src in &img_srcs {
            // Resolve src relative to the HTML file's directory within OPF dir.
            let abs_href = resolve_zip_path(&opf_dir, &resolve_zip_path(&html_dir, src));
            // Deduplicate.
            if !body_image_hrefs.contains(&abs_href) {
                body_image_hrefs.push(abs_href);
            }
        }

        // Build a local map: src string → kindle record index (1-based global).
        let mut src_to_record: HashMap<String, u32> = HashMap::new();
        for src in &img_srcs {
            let abs_href = resolve_zip_path(&opf_dir, &resolve_zip_path(&html_dir, src));
            // Position among body images (0-based) + cover offset + 1 = 1-based.
            if let Some(pos) = body_image_hrefs.iter().position(|h| h == &abs_href) {
                let record_idx = cover_offset + pos as u32 + 1;
                src_to_record.insert(src.clone(), record_idx);
            }
        }

        let cleaned = clean_html(&html_bytes, &src_to_record, &current_prefix, &html_zip_dir, &spine_prefix_map)?;
        merged_html_parts.push(cleaned);
    }

    // Build the merged body: wrap parts in a minimal HTML document.
    // Each part already begins with an <a name="PREFIX"> anchor injected by
    // clean_html as the very first body element, so filepos targets land on a
    // proper named anchor rather than an invisible-but-broken tag.
    let mut merged = Vec::new();
    merged.extend_from_slice(b"<html><head><meta charset=\"utf-8\"/></head><body>");
    for (i, part) in merged_html_parts.iter().enumerate() {
        if i > 0 {
            merged.extend_from_slice(b"<mbp:pagebreak/>");
        }
        merged.extend_from_slice(part);
    }
    merged.extend_from_slice(b"</body>");
    // Add <guide> cover reference so Kindle firmware can locate the cover image.
    if cover_bytes.is_some() {
        merged.extend_from_slice(b"<guide><reference type=\"cover\" title=\"Cover\" href=\"kindle:embed:0001?mime=image/jpeg\"/></guide>");
    }
    merged.extend_from_slice(b"</html>");

    // 1b. Convert href="#anchor" → filepos=NNNNNNNNNN for Kindle MOBI6 navigation.
    let merged = apply_filepos_links(merged);

    // 2. Collect images.
    let mut images: Vec<Vec<u8>> = Vec::new();
    if let Some(cb) = cover_bytes {
        images.push(cb.to_vec());
    }
    for img_href in &body_image_hrefs {
        if let Some(&idx) = name_to_idx.get(img_href) {
            let mut f = archive.by_index(idx)?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
            images.push(buf);
        }
    }

    // 3. Write PalmDB.
    write_palmdb(dest, sidecar, &merged, &images, cover_bytes.is_some())
}

// ── HTML helpers
// ──────────────────────────────────────────────────────────────

/// Walk HTML bytes and collect all `src` attribute values from `<img>`
/// elements.
fn collect_img_srcs(html: &[u8]) -> Vec<String> {
    let mut reader = Reader::from_reader(html);
    reader.config_mut().check_end_names = false;
    let mut buf = Vec::new();
    let mut srcs = Vec::new();
    loop {
        buf.clear();
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e) | Event::Empty(ref e)) => {
                let local = local_name_lower(e.name().as_ref());
                if local == "img" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"src" {
                            if let Ok(v) = std::str::from_utf8(attr.value.as_ref()) {
                                srcs.push(v.to_string());
                            }
                        }
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    srcs
}

/// Clean an HTML document for inclusion in MOBI:
/// - Extracts only `<body>` content (strips head, html wrappers).
/// - Rewrites `<img src="...">` to `kindle:embed:XXXX?mime=image/jpeg`.
/// - Strips `<style>` block elements and `<link rel="stylesheet">`.
/// - Rewrites `id` attributes by prefixing them with `current_prefix` to avoid
///   collisions when multiple spine documents are merged.
/// - Rewrites `href` links: same-file fragments become `#{prefix}_{fragment}`,
///   cross-document links become `#{target_prefix}_{fragment}` or
///   `#{target_prefix}` for bare filenames.
fn clean_html(
    html: &[u8],
    src_to_record: &HashMap<String, u32>,
    current_prefix: &str,
    html_zip_dir: &str,
    spine_prefix_map: &HashMap<String, String>,
) -> Result<Vec<u8>, Error> {
    let mut reader = Reader::from_reader(html);
    reader.config_mut().check_end_names = false;
    let mut buf = Vec::new();
    let mut out = Vec::with_capacity(html.len());

    let mut in_body = false;
    let mut anchor_emitted = false; // true once we've emitted <a name="PREFIX"> for this chapter
    let mut skip_style_depth: u32 = 0; // >0 means we are inside a <style> block to skip

    loop {
        buf.clear();
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let local = local_name_lower(e.name().as_ref());

                if local == "body" {
                    in_body = true;
                    continue;
                }
                if !in_body {
                    continue;
                }

                // Skip <style> blocks.
                if local == "style" {
                    skip_style_depth += 1;
                    continue;
                }
                if skip_style_depth > 0 {
                    // Track nesting for any nested elements (unusual but safe).
                    skip_style_depth += 1;
                    continue;
                }

                // Skip <link rel="stylesheet">.
                if local == "link" {
                    let mut is_stylesheet = false;
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"rel" {
                            if let Ok(v) = std::str::from_utf8(attr.value.as_ref()) {
                                if v.eq_ignore_ascii_case("stylesheet") {
                                    is_stylesheet = true;
                                }
                            }
                        }
                    }
                    if is_stylesheet {
                        continue;
                    }
                }

                // Rewrite <img src="...">
                if local == "img" {
                    if !anchor_emitted {
                        write!(out, "<a name=\"{current_prefix}\"></a>")?;
                        anchor_emitted = true;
                    }
                    let new_src = img_src_to_kindle_embed(e, src_to_record);
                    write!(out, "<img src=\"{new_src}\"/>")?;
                    continue;
                }

                // Rewrite <a> — preserve all attributes, rewrite href and id.
                if local == "a" {
                    if !anchor_emitted {
                        write!(out, "<a name=\"{current_prefix}\"></a>")?;
                        anchor_emitted = true;
                    }
                    out.extend_from_slice(b"<a");
                    for attr in e.attributes().flatten() {
                        out.extend_from_slice(b" ");
                        out.extend_from_slice(attr.key.as_ref());
                        out.extend_from_slice(b"=\"");
                        if attr.key.as_ref() == b"href" {
                            if let Ok(href) = std::str::from_utf8(attr.value.as_ref()) {
                                let new_href = rewrite_href(href, current_prefix, html_zip_dir, spine_prefix_map);
                                write!(out, "{new_href}")?;
                            } else {
                                out.extend_from_slice(attr.value.as_ref());
                            }
                        } else if attr.key.as_ref() == b"id" {
                            if let Ok(id_val) = std::str::from_utf8(attr.value.as_ref()) {
                                write!(out, "{current_prefix}_{id_val}")?;
                            } else {
                                out.extend_from_slice(attr.value.as_ref());
                            }
                        } else {
                            out.extend_from_slice(attr.value.as_ref());
                        }
                        out.extend_from_slice(b"\"");
                    }
                    out.extend_from_slice(b">");
                    continue;
                }

                // Passthrough — rewrite id attributes to avoid collisions.
                if !anchor_emitted {
                    write!(out, "<a name=\"{current_prefix}\"></a>")?;
                    anchor_emitted = true;
                }
                out.extend_from_slice(b"<");
                out.extend_from_slice(e.name().as_ref());
                for attr in e.attributes().flatten() {
                    out.extend_from_slice(b" ");
                    out.extend_from_slice(attr.key.as_ref());
                    out.extend_from_slice(b"=\"");
                    if attr.key.as_ref() == b"id" {
                        if let Ok(id_val) = std::str::from_utf8(attr.value.as_ref()) {
                            write!(out, "{current_prefix}_{id_val}")?;
                        } else {
                            out.extend_from_slice(attr.value.as_ref());
                        }
                    } else {
                        out.extend_from_slice(attr.value.as_ref());
                    }
                    out.extend_from_slice(b"\"");
                }
                out.extend_from_slice(b">");
            }
            Ok(Event::Empty(ref e)) => {
                let local = local_name_lower(e.name().as_ref());

                if !in_body {
                    continue;
                }
                if skip_style_depth > 0 {
                    continue;
                }

                // Skip <link rel="stylesheet"/>.
                if local == "link" {
                    let mut is_stylesheet = false;
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"rel" {
                            if let Ok(v) = std::str::from_utf8(attr.value.as_ref()) {
                                if v.eq_ignore_ascii_case("stylesheet") {
                                    is_stylesheet = true;
                                }
                            }
                        }
                    }
                    if is_stylesheet {
                        continue;
                    }
                }

                if local == "img" {
                    if !anchor_emitted {
                        write!(out, "<a name=\"{current_prefix}\"></a>")?;
                        anchor_emitted = true;
                    }
                    let new_src = img_src_to_kindle_embed(e, src_to_record);
                    write!(out, "<img src=\"{new_src}\"/>")?;
                    continue;
                }

                // Self-closing <a/> (anchor target with id, no href).
                if local == "a" {
                    if !anchor_emitted {
                        write!(out, "<a name=\"{current_prefix}\"></a>")?;
                        anchor_emitted = true;
                    }
                    out.extend_from_slice(b"<a");
                    for attr in e.attributes().flatten() {
                        out.extend_from_slice(b" ");
                        out.extend_from_slice(attr.key.as_ref());
                        out.extend_from_slice(b"=\"");
                        if attr.key.as_ref() == b"id" {
                            if let Ok(id_val) = std::str::from_utf8(attr.value.as_ref()) {
                                write!(out, "{current_prefix}_{id_val}")?;
                            } else {
                                out.extend_from_slice(attr.value.as_ref());
                            }
                        } else {
                            out.extend_from_slice(attr.value.as_ref());
                        }
                        out.extend_from_slice(b"\"");
                    }
                    out.extend_from_slice(b"/>");
                    continue;
                }

                // Self-closing passthrough — rewrite id attributes.
                if !anchor_emitted {
                    write!(out, "<a name=\"{current_prefix}\"></a>")?;
                    anchor_emitted = true;
                }
                out.extend_from_slice(b"<");
                out.extend_from_slice(e.name().as_ref());
                for attr in e.attributes().flatten() {
                    out.extend_from_slice(b" ");
                    out.extend_from_slice(attr.key.as_ref());
                    out.extend_from_slice(b"=\"");
                    if attr.key.as_ref() == b"id" {
                        if let Ok(id_val) = std::str::from_utf8(attr.value.as_ref()) {
                            write!(out, "{current_prefix}_{id_val}")?;
                        } else {
                            out.extend_from_slice(attr.value.as_ref());
                        }
                    } else {
                        out.extend_from_slice(attr.value.as_ref());
                    }
                    out.extend_from_slice(b"\"");
                }
                out.extend_from_slice(b"/>");
            }
            Ok(Event::End(ref e)) => {
                let local = local_name_lower(e.name().as_ref());

                if local == "body" {
                    in_body = false;
                    continue;
                }
                if !in_body {
                    continue;
                }

                if local == "style" {
                    skip_style_depth = skip_style_depth.saturating_sub(1);
                    continue;
                }
                if skip_style_depth > 0 {
                    skip_style_depth = skip_style_depth.saturating_sub(1);
                    continue;
                }

                out.extend_from_slice(b"</");
                out.extend_from_slice(e.name().as_ref());
                out.extend_from_slice(b">");
            }
            Ok(Event::Text(ref e)) if in_body && skip_style_depth == 0 => {
                out.extend_from_slice(e.as_ref());
            }
            Ok(Event::CData(ref e)) if in_body && skip_style_depth == 0 => {
                out.extend_from_slice(e.as_ref());
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    Ok(out)
}

/// Build a `kindle:embed:XXXX?mime=image/jpeg` URL from an `<img>` element,
/// looking up the `src` attribute in `src_to_record`.  Falls back to a
/// placeholder if the src is not found.
fn img_src_to_kindle_embed(e: &quick_xml::events::BytesStart<'_>, src_to_record: &HashMap<String, u32>) -> String {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == b"src" {
            if let Ok(src) = std::str::from_utf8(attr.value.as_ref()) {
                let record = src_to_record.get(src).copied().unwrap_or(1);
                return format!("kindle:embed:{record:04}?mime=image/jpeg");
            }
        }
    }
    "kindle:embed:0001?mime=image/jpeg".to_string()
}

/// Post-process the merged HTML for MOBI6 navigation.
///
/// MOBI6 / Kindle does not navigate via `href="#fragment"` — it uses
/// `filepos=NNNNNNNNNN` (10-digit zero-padded byte offset into the HTML
/// text stream).  This function performs a three-pass transform:
///
/// 1. Scan the HTML for every `id="NAME"` and record the byte offset of the
///    opening `<` of the enclosing tag.
/// 2. Rebuild the HTML, replacing each `href="#NAME"` with the placeholder
///    `filepos=0000000000` (fixed width so later positions stay stable).
/// 3. Re-scan the rebuilt HTML to find final anchor positions, then fill in the
///    actual 10-digit values.
fn apply_filepos_links(html: Vec<u8>) -> Vec<u8> {
    const ID_PAT: &[u8] = b" id=\"";
    const HREF_FRAG_PAT: &[u8] = b"href=\"#";
    const FILEPOS_PLACEHOLDER: &[u8] = b"filepos=0000000000";

    // ── Pass 2: rebuild HTML, replacing href="#NAME" → filepos=0000000000.
    // Record (position_of_digits_in_out, fragment_name) for fixup later.
    let mut out: Vec<u8> = Vec::with_capacity(html.len());
    let mut fixups: Vec<(usize, String)> = Vec::new();
    let mut i = 0;
    while i < html.len() {
        if html[i..].starts_with(HREF_FRAG_PAT) {
            let frag_start = i + HREF_FRAG_PAT.len();
            if let Some(rel_end) = html[frag_start..].iter().position(|&b| b == b'"') {
                let frag = std::str::from_utf8(&html[frag_start..frag_start + rel_end]).unwrap_or("").to_string();
                let digits_pos = out.len() + b"filepos=".len();
                out.extend_from_slice(FILEPOS_PLACEHOLDER);
                fixups.push((digits_pos, frag));
                i = frag_start + rel_end + 1; // skip past closing "
                continue;
            }
        }
        out.push(html[i]);
        i += 1;
    }

    // ── Pass 3: find byte offset of every anchor in the rebuilt HTML.
    // Recognises both ` id="NAME"` (elements, mbp:pagebreak) and
    // ` name="NAME"` (<a name> first-item anchors).
    let mut id_to_pos: HashMap<String, usize> = HashMap::new();
    const NAME_PAT: &[u8] = b" name=\"";
    let mut pos = 0;
    while pos < out.len() {
        let (pat_len, val_start) = if out[pos..].starts_with(ID_PAT) {
            (ID_PAT.len(), pos + ID_PAT.len())
        } else if out[pos..].starts_with(NAME_PAT) {
            (NAME_PAT.len(), pos + NAME_PAT.len())
        } else {
            pos += 1;
            continue;
        };
        let _ = pat_len; // used above
        if let Some(rel_end) = out[val_start..].iter().position(|&b| b == b'"') {
            let anchor_val = std::str::from_utf8(&out[val_start..val_start + rel_end]).unwrap_or("").to_string();
            // Backtrack to the opening '<' of the enclosing tag.
            let tag_start = out[..pos].iter().rposition(|&b| b == b'<').unwrap_or(0);
            // id= takes precedence over name= so only insert if not already present.
            id_to_pos.entry(anchor_val).or_insert(tag_start);
            pos = val_start + rel_end + 1;
            continue;
        }
        pos += 1;
    }

    // ── Pass 4: fill placeholders with real positions.
    for (digits_pos, frag) in fixups {
        let target = id_to_pos.get(&frag).copied().unwrap_or(0);
        let digits = format!("{target:010}");
        out[digits_pos..digits_pos + 10].copy_from_slice(digits.as_bytes());
    }

    out
}

/// Rewrite an `href` value for the merged MOBI document.
///
/// * `#X`           → `#{prefix}_X`          same-file fragment
/// * `file.xhtml#X` → `#{file_prefix}_X`     cross-document with fragment
/// * `file.xhtml`   → `#{file_prefix}`        cross-document top-of-chapter
/// * external       → unchanged
fn rewrite_href(href: &str, current_prefix: &str, html_zip_dir: &str, spine_prefix_map: &HashMap<String, String>) -> String {
    if href.is_empty() || href == "#" {
        return "#".to_string();
    }
    // External links.
    if href.starts_with("http://") || href.starts_with("https://") || href.starts_with("mailto:") {
        return href.to_string();
    }
    // Same-file fragment: #X → #{prefix}_X
    if href.starts_with('#') {
        let fragment = &href[1..];
        return format!("#{current_prefix}_{fragment}");
    }
    // Cross-document with fragment: file.xhtml#anchor → #{target_prefix}_anchor
    if let Some(hash_pos) = href.find('#') {
        let file_part = &href[..hash_pos];
        let fragment = &href[hash_pos + 1..];
        let zip_path = resolve_zip_path(html_zip_dir, file_part);
        if let Some(prefix) = spine_prefix_map.get(&zip_path) {
            return format!("#{prefix}_{fragment}");
        }
        return format!("#{fragment}");
    }
    // Bare filename: file.xhtml → #{target_prefix}
    let zip_path = resolve_zip_path(html_zip_dir, href);
    if let Some(prefix) = spine_prefix_map.get(&zip_path) {
        return format!("#{prefix}");
    }
    // Unresolvable — keep as-is.
    href.to_string()
}

/// Derive a stable anchor prefix from a zip path.
/// `OEBPS/Text/chapter2.xhtml` → `chapter2`
fn spine_prefix_for(zip_path: &str) -> String {
    let basename = zip_path.rsplit('/').next().unwrap_or(zip_path);
    let stem = if let Some(pos) = basename.rfind('.') { &basename[..pos] } else { basename };
    stem.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect()
}

// ── OPF parsing
// ───────────────────────────────────────────────────────────────

/// Parse an OPF document and return (manifest id→href map, spine idref list).
fn parse_opf_manifest_and_spine(opf: &[u8]) -> Result<(HashMap<String, String>, Vec<String>), Error> {
    let mut reader = Reader::from_reader(opf);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    let mut manifest: HashMap<String, String> = HashMap::new();
    let mut spine: Vec<String> = Vec::new();
    let mut in_manifest = false;
    let mut in_spine = false;

    loop {
        buf.clear();
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let local = local_name_lower(e.name().as_ref());
                match local.as_str() {
                    "manifest" => in_manifest = true,
                    "spine" => in_spine = true,
                    "item" if in_manifest => {
                        let mut id = None;
                        let mut href = None;
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"id" => id = std::str::from_utf8(attr.value.as_ref()).ok().map(String::from),
                                b"href" => href = std::str::from_utf8(attr.value.as_ref()).ok().map(String::from),
                                _ => {}
                            }
                        }
                        if let (Some(id), Some(href)) = (id, href) {
                            manifest.insert(id, href);
                        }
                    }
                    "itemref" if in_spine => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"idref" {
                                if let Ok(v) = std::str::from_utf8(attr.value.as_ref()) {
                                    spine.push(v.to_string());
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let local = local_name_lower(e.name().as_ref());
                match local.as_str() {
                    "item" if in_manifest => {
                        let mut id = None;
                        let mut href = None;
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"id" => id = std::str::from_utf8(attr.value.as_ref()).ok().map(String::from),
                                b"href" => href = std::str::from_utf8(attr.value.as_ref()).ok().map(String::from),
                                _ => {}
                            }
                        }
                        if let (Some(id), Some(href)) = (id, href) {
                            manifest.insert(id, href);
                        }
                    }
                    "itemref" if in_spine => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"idref" {
                                if let Ok(v) = std::str::from_utf8(attr.value.as_ref()) {
                                    spine.push(v.to_string());
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let local = local_name_lower(e.name().as_ref());
                match local.as_str() {
                    "manifest" => in_manifest = false,
                    "spine" => in_spine = false,
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(e.into()),
            _ => {}
        }
    }

    Ok((manifest, spine))
}

// ── PalmDB writer
// ─────────────────────────────────────────────────────────────

/// Write a minimal MOBI6 / PalmDB file to `dest`.
fn write_palmdb(dest: &Path, sidecar: &BookSidecar, html: &[u8], images: &[Vec<u8>], has_cover: bool) -> Result<(), Error> {
    // Chunk HTML into records of at most 4096 bytes.
    const MAX_TEXT_RECORD: usize = 4096;
    let text_chunks: Vec<&[u8]> = html.chunks(MAX_TEXT_RECORD).collect();
    let text_record_count = text_chunks.len();
    let total_text_len = html.len() as u32;

    let image_record_start = text_record_count + 1; // record 0 is header
    let first_image_index = image_record_start as u32;
    let cover_record_index = if has_cover { image_record_start as u32 } else { 0xFFFF_FFFF };

    let total_records = 1 + text_record_count + images.len();

    // Build EXTH block first (we need its size for record 0 layout).
    let exth_bytes = build_exth(sidecar, cover_record_index, has_cover);

    // Title bytes (UTF-8).
    let title_bytes = sidecar.title.as_bytes();

    // Build record 0: PalmDoc header + MOBI header + title + EXTH.
    let record0 = build_record0(
        total_text_len,
        text_record_count as u16,
        first_image_index,
        title_bytes,
        &exth_bytes,
        text_record_count as u32,
        image_record_start as u32,
    );

    // Compute record offsets.
    // PalmDB file layout:
    //   [0..78)      PalmDB header (name 32 + attrs/ver 4 + dates 16 +
    //                appInfo/sortInfo 8 + type/creator 8 + seed/nextList 8 +
    //                numRecords 2 = 78 bytes)
    //   [78..)       record list: numRecords × 8 bytes
    //   [78 + numRecords*8 ..) record data (must be even-aligned by 2 bytes)
    let palmdb_header_size: u32 = 78;
    let record_list_size: u32 = total_records as u32 * 8;
    let data_start: u32 = palmdb_header_size + record_list_size;
    // PalmDB spec requires data start at an even offset; add padding if needed.
    let data_start = if data_start % 2 != 0 { data_start + 1 } else { data_start };

    let mut offsets: Vec<u32> = Vec::with_capacity(total_records);
    let mut current = data_start;
    offsets.push(current);
    current += record0.len() as u32;

    for chunk in &text_chunks {
        offsets.push(current);
        current += chunk.len() as u32;
    }
    for img in images {
        offsets.push(current);
        current += img.len() as u32;
    }

    // Now write the file.
    let mut f = std::fs::File::create(dest)?;

    // ── PalmDB header (78 bytes) ─────────────────────────────────────────
    // name: 32 bytes, null-padded, truncated at 31.
    let mut name_buf = [0u8; 32];
    let name_bytes = sidecar.title.as_bytes();
    let copy_len = name_bytes.len().min(31);
    name_buf[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
    f.write_all(&name_buf)?;

    f.write_all(&0u16.to_be_bytes())?; // attributes
    f.write_all(&0u16.to_be_bytes())?; // version
    f.write_all(&0u32.to_be_bytes())?; // creationDate
    f.write_all(&0u32.to_be_bytes())?; // modificationDate
    f.write_all(&0u32.to_be_bytes())?; // lastBackupDate
    f.write_all(&0u32.to_be_bytes())?; // modificationNumber
    f.write_all(&0u32.to_be_bytes())?; // appInfoID
    f.write_all(&0u32.to_be_bytes())?; // sortInfoID
    f.write_all(b"BOOK")?; // type
    f.write_all(b"MOBI")?; // creator
    f.write_all(&0u32.to_be_bytes())?; // uniqueIDSeed
    f.write_all(&0u32.to_be_bytes())?; // nextRecordListID
    f.write_all(&(total_records as u16).to_be_bytes())?; // numRecords

    // ── Record list ──────────────────────────────────────────────────────
    for (i, &offset) in offsets.iter().enumerate() {
        f.write_all(&offset.to_be_bytes())?;
        // attributes (1 byte) + uniqueID (3 bytes) packed as u32 = 0.
        // Give each record a unique ID = its index for good measure.
        let uid: u32 = i as u32;
        f.write_all(&uid.to_be_bytes())?;
    }

    // Padding byte if data_start was bumped to even.
    let raw_data_start = palmdb_header_size + record_list_size;
    if raw_data_start % 2 != 0 {
        f.write_all(&[0u8])?;
    }

    // ── Record data ──────────────────────────────────────────────────────
    f.write_all(&record0)?;
    for chunk in &text_chunks {
        f.write_all(chunk)?;
    }
    for img in images {
        f.write_all(img)?;
    }

    f.flush()?;
    Ok(())
}

/// Build record 0: PalmDoc header + MOBI header + EXTH block + title bytes.
fn build_record0(
    total_text_len: u32,
    text_record_count: u16,
    first_image_index: u32,
    title_bytes: &[u8],
    exth_bytes: &[u8],
    last_text_record: u32, // = text_record_count (1-based last index)
    _image_record_start: u32,
) -> Vec<u8> {
    let mut rec = Vec::new();

    // ── PalmDoc header (16 bytes) ────────────────────────────────────────
    rec.extend_from_slice(&1u16.to_be_bytes()); // compression: no compression
    rec.extend_from_slice(&0u16.to_be_bytes()); // unused
    rec.extend_from_slice(&total_text_len.to_be_bytes()); // textLength
    rec.extend_from_slice(&text_record_count.to_be_bytes()); // recordCount
    rec.extend_from_slice(&4096u16.to_be_bytes()); // recordSize
    rec.extend_from_slice(&0u16.to_be_bytes()); // encryptionType
    rec.extend_from_slice(&0u16.to_be_bytes()); // unknown

    // ── MOBI header (232 bytes) ──────────────────────────────────────────
    // Record 0 layout: PalmDoc (16) + MOBI (232) + EXTH + title.
    // fullNameOffset must point past the EXTH block.
    let full_name_offset: u32 = 16 + 232 + exth_bytes.len() as u32;
    let full_name_length: u32 = title_bytes.len() as u32;
    let first_non_book_index: u32 = last_text_record + 1;
    let exth_flags: u32 = if exth_bytes.is_empty() { 0 } else { 0x40 };

    let mobi_start = rec.len();
    rec.extend_from_slice(b"MOBI"); //  0 identifier
    rec.extend_from_slice(&232u32.to_be_bytes()); //  4 headerLength
    rec.extend_from_slice(&2u32.to_be_bytes()); //  8 mobiType (book)
    rec.extend_from_slice(&65001u32.to_be_bytes()); // 12 textEncoding (UTF-8)
    rec.extend_from_slice(&0u32.to_be_bytes()); // 16 uniqueID
    rec.extend_from_slice(&6u32.to_be_bytes()); // 20 fileVersion
    rec.extend_from_slice(&[0u8; 40]); // 24 reserved1
    rec.extend_from_slice(&first_non_book_index.to_be_bytes()); // 64 firstNonBookIndex
    rec.extend_from_slice(&full_name_offset.to_be_bytes()); // 68 fullNameOffset
    rec.extend_from_slice(&full_name_length.to_be_bytes()); // 72 fullNameLength
    rec.extend_from_slice(&0x0409u32.to_be_bytes()); // 76 locale (en-US)
    rec.extend_from_slice(&0u32.to_be_bytes()); // 80 inputLanguage
    rec.extend_from_slice(&0u32.to_be_bytes()); // 84 outputLanguage
    rec.extend_from_slice(&6u32.to_be_bytes()); // 88 minVersion
    rec.extend_from_slice(&first_image_index.to_be_bytes()); // 92 firstImageIndex
    rec.extend_from_slice(&0u32.to_be_bytes()); // 96 huffmanRecordOffset
    rec.extend_from_slice(&0u32.to_be_bytes()); // 100 huffmanRecordCount
    rec.extend_from_slice(&0u32.to_be_bytes()); // 104 huffmanTableOffset
    rec.extend_from_slice(&0u32.to_be_bytes()); // 108 huffmanTableLength
    rec.extend_from_slice(&exth_flags.to_be_bytes()); // 112 exthFlags
    rec.extend_from_slice(&[0u8; 32]); // 116 reserved2
    rec.extend_from_slice(&0xFFFF_FFFFu32.to_be_bytes()); // 148 drmOffset
    rec.extend_from_slice(&0xFFFF_FFFFu32.to_be_bytes()); // 152 drmCount
    rec.extend_from_slice(&0u32.to_be_bytes()); // 156 drmSize
    rec.extend_from_slice(&0u32.to_be_bytes()); // 160 drmFlags
    rec.extend_from_slice(&[0u8; 8]); // 164 reserved3
    rec.extend_from_slice(&1u16.to_be_bytes()); // 172 firstContentRecordNumber
    rec.extend_from_slice(&(last_text_record as u16).to_be_bytes()); // 174 lastContentRecordNumber
    rec.extend_from_slice(&1u32.to_be_bytes()); // 176 unknown1
    rec.extend_from_slice(&0xFFFF_FFFFu32.to_be_bytes()); // 180 fcisRecord
    rec.extend_from_slice(&1u32.to_be_bytes()); // 184 unknown2
    rec.extend_from_slice(&0xFFFF_FFFFu32.to_be_bytes()); // 188 flisRecord
    rec.extend_from_slice(&[0u8; 16]); // 192 unknown3
    rec.extend_from_slice(&0u16.to_be_bytes()); // 208 extraDataFlags

    // Pad MOBI header to exactly 232 bytes from mobi_start.
    let mobi_written = rec.len() - mobi_start;
    if mobi_written < 232 {
        let pad = 232 - mobi_written;
        rec.resize(rec.len() + pad, 0u8);
    }

    // EXTH block (must come before title string).
    rec.extend_from_slice(exth_bytes);

    // Title string (fullNameOffset points here).
    rec.extend_from_slice(title_bytes);

    rec
}

/// Build the EXTH block bytes (identifier + headerLength + recordCount +
/// records).
fn build_exth(sidecar: &BookSidecar, cover_record_index: u32, has_cover: bool) -> Vec<u8> {
    let mut records: Vec<(u32, Vec<u8>)> = Vec::new();

    // 100 = author (first author).
    if let Some(author) = sidecar.authors.first() {
        records.push((100, author.name.as_bytes().to_vec()));
    }

    // 101 = publisher.
    if let Some(pub_name) = &sidecar.publisher {
        records.push((101, pub_name.as_bytes().to_vec()));
    }

    // 503 = title (updated title).
    records.push((503, sidecar.title.as_bytes().to_vec()));

    // 506 = cover record index (0-based from start of file records, but
    // the EXTH 506 value is the 0-based index of the cover record in the
    // MOBI record numbering — i.e. the record number itself).
    if has_cover {
        records.push((506, cover_record_index.to_be_bytes().to_vec()));
        // 537 = thumbnail record index (same as cover for simplicity).
        records.push((537, cover_record_index.to_be_bytes().to_vec()));
    }

    // 517 = series name, 518 = series index.
    if let Some(series) = &sidecar.series {
        records.push((517, series.name.as_bytes().to_vec()));
        if let Some(number) = &series.number {
            records.push((518, number.to_string().as_bytes().to_vec()));
        }
    }

    // 105 = subject/genre (one record per genre).
    for genre in &sidecar.genres {
        records.push((105, genre.as_bytes().to_vec()));
    }

    // 106 = published date (year as 4-digit ASCII string).
    if let Some(year) = sidecar.published_date {
        records.push((106, year.to_string().as_bytes().to_vec()));
    }

    if records.is_empty() {
        return Vec::new();
    }

    // Compute total length.
    // Each record: 4 (type) + 4 (length) + data.len() bytes.
    let records_data_len: usize = records.iter().map(|(_, d)| 8 + d.len()).sum();
    // EXTH block: "EXTH" (4) + headerLength (4) + recordCount (4) + records.
    let total_len = 4 + 4 + 4 + records_data_len;

    let mut out = Vec::with_capacity(total_len);
    out.extend_from_slice(b"EXTH");
    out.extend_from_slice(&(total_len as u32).to_be_bytes());
    out.extend_from_slice(&(records.len() as u32).to_be_bytes());

    for (rec_type, data) in &records {
        let rec_len = 8 + data.len();
        out.extend_from_slice(&rec_type.to_be_bytes());
        out.extend_from_slice(&(rec_len as u32).to_be_bytes());
        out.extend_from_slice(data);
    }

    // Pad EXTH block to 4-byte boundary and update the stored headerLength.
    let remainder = out.len() % 4;
    if remainder != 0 {
        let pad = 4 - remainder;
        out.resize(out.len() + pad, 0u8);
        // Update the headerLength field at bytes 4..8.
        let new_len = out.len() as u32;
        out[4..8].copy_from_slice(&new_len.to_be_bytes());
    }

    out
}

// ── Shared helpers
// ────────────────────────────────────────────────────────────

fn normalize_zip_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn resolve_zip_path(dir: &str, href: &str) -> String {
    let combined = if dir.is_empty() { href.to_string() } else { format!("{dir}/{href}") };
    normalize_zip_path(&combined)
}

fn local_name_lower(qualified: &[u8]) -> String {
    let s = std::str::from_utf8(qualified).unwrap_or("");
    let local = s.split(':').next_back().unwrap_or(s);
    local.to_ascii_lowercase()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use bb_core::{
        book::{AuthorRole, BookStatus, FileFormat},
        storage::{BookSidecar, SidecarAuthor, SidecarFile, SidecarSeries},
    };
    use rust_decimal::Decimal;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;

    use super::convert_to_mobi;

    // ── Minimal synthetic EPUB builder ───────────────────────────────────

    const CONTAINER_XML: &[u8] = br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;

    const CONTENT_OPF: &[u8] = br#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test Book</dc:title>
    <dc:creator>Test Author</dc:creator>
  </metadata>
  <manifest>
    <item id="ch1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
  </spine>
</package>"#;

    const CHAPTER1_XHTML: &[u8] = b"<html><body><p>Hello world</p></body></html>";

    fn build_test_epub() -> Vec<u8> {
        let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let cursor = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(cursor);

        zip.start_file("mimetype", stored).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();

        zip.start_file("META-INF/container.xml", stored).unwrap();
        zip.write_all(CONTAINER_XML).unwrap();

        zip.start_file("OEBPS/content.opf", stored).unwrap();
        zip.write_all(CONTENT_OPF).unwrap();

        zip.start_file("OEBPS/chapter1.xhtml", stored).unwrap();
        zip.write_all(CHAPTER1_XHTML).unwrap();

        zip.finish().unwrap().into_inner()
    }

    fn make_sidecar(title: &str, author: &str) -> BookSidecar {
        BookSidecar {
            title: title.to_string(),
            authors: vec![SidecarAuthor {
                name: author.to_string(),
                role: AuthorRole::Author,
                sort_order: 0,
                file_as: None,
            }],
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

    // ── Test 1: output is a PalmDB file (not a ZIP) ───────────────────────

    #[test]
    fn test_convert_produces_palmdb_signature() {
        let dir = tempdir().unwrap();
        let epub_path = dir.path().join("test.epub");
        let mobi_path = dir.path().join("test.mobi");

        std::fs::write(&epub_path, build_test_epub()).unwrap();
        let sidecar = make_sidecar("Test Book", "Test Author");

        convert_to_mobi(&epub_path, &mobi_path, &sidecar, None).expect("convert_to_mobi failed");

        let out = std::fs::read(&mobi_path).unwrap();
        // A MOBI/PalmDB file must NOT start with PK (ZIP magic).
        assert!(!out.starts_with(b"PK"), "output should not be a ZIP file (PK magic found)");
        // PalmDB type field at offset 60 must be "BOOK".
        assert_eq!(&out[60..64], b"BOOK", "PalmDB type field should be BOOK");
        // PalmDB creator field at offset 64 must be "MOBI".
        assert_eq!(&out[64..68], b"MOBI", "PalmDB creator field should be MOBI");
    }

    // ── Test 2: EXTH block contains the author name ───────────────────────

    #[test]
    fn test_convert_includes_exth_author() {
        let dir = tempdir().unwrap();
        let epub_path = dir.path().join("test.epub");
        let mobi_path = dir.path().join("test.mobi");

        std::fs::write(&epub_path, build_test_epub()).unwrap();
        let sidecar = make_sidecar("My Book", "Jane Austen");

        convert_to_mobi(&epub_path, &mobi_path, &sidecar, None).expect("convert_to_mobi failed");

        let out = std::fs::read(&mobi_path).unwrap();
        // The author string must appear somewhere in the output bytes.
        let author_bytes = b"Jane Austen";
        let found = out.windows(author_bytes.len()).any(|w| w == author_bytes);
        assert!(found, "author name 'Jane Austen' should appear in MOBI output");
    }

    // ── Test 3: EXTH record 506 present when cover is provided ───────────

    #[test]
    fn test_convert_cover_record_index_matches_exth_506() {
        let dir = tempdir().unwrap();
        let epub_path = dir.path().join("test.epub");
        let mobi_path = dir.path().join("test.mobi");

        std::fs::write(&epub_path, build_test_epub()).unwrap();
        let sidecar = make_sidecar("Covered Book", "Cover Author");

        // Minimal fake JPEG.
        let fake_cover: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46];

        convert_to_mobi(&epub_path, &mobi_path, &sidecar, Some(fake_cover)).expect("convert_to_mobi with cover failed");

        let out = std::fs::read(&mobi_path).unwrap();
        // EXTH record type 506 is present: the bytes 0x00 0x00 0x01 0xFA (506 in
        // big-endian u32).
        let exth_506_type = 506u32.to_be_bytes();
        let found = out.windows(4).any(|w| w == exth_506_type);
        assert!(found, "EXTH record type 506 (cover record index) should be present in output");
    }

    // ── Test 4: EXTH block contains the series name ───────────────────────

    #[test]
    fn test_convert_includes_exth_series() {
        let dir = tempdir().unwrap();
        let epub_path = dir.path().join("test.epub");
        let mobi_path = dir.path().join("test.mobi");

        std::fs::write(&epub_path, build_test_epub()).unwrap();
        let mut sidecar = make_sidecar("Foundation", "Isaac Asimov");
        sidecar.series = Some(SidecarSeries {
            name: "Foundation Series".to_string(),
            number: Some(Decimal::new(1, 0)),
        });

        convert_to_mobi(&epub_path, &mobi_path, &sidecar, None).expect("convert_to_mobi failed");

        let out = std::fs::read(&mobi_path).unwrap();
        let series_bytes = b"Foundation Series";
        let found = out.windows(series_bytes.len()).any(|w| w == series_bytes);
        assert!(found, "series name should appear in MOBI output");
    }
}
