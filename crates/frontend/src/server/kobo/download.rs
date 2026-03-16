//! `GET /kobo/{sync_token}/v1/download/{book_token}/{format}`
//!
//! Streams the best available file for the requested format from LibraryStore.
//! The `book_token` is the Kobo-facing ID from the `DownloadUrls` array set
//! during library sync (the `BK_`-stripped token). `format` is `"epub"` or
//! `"kepub"`, matching what we encoded in that URL.
//!
//! File selection: prefer `Enriched` role; fall back to `Original` (including
//! from the `Originals/` flat directory) when the enriched file is absent.
//! No on-the-fly conversion — both formats are pre-stored by M7.

use std::{collections::HashMap, str::FromStr, sync::Arc};

use axum::{
    body::Body,
    extract::Path,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use bb_core::{
    CoreServices,
    book::{BookToken, FileFormat, FileRole},
};
use tokio::fs::File;
use tokio_util::io::ReaderStream;

use super::KoboDevice;

// ── Handler
// ─────────────────────────────────────────────────────────────────

pub async fn handle(kobo: KoboDevice, Path(params): Path<HashMap<String, String>>, core_services: Arc<CoreServices>) -> Response {
    let Some(book_token_str) = params.get("book_token") else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(format_str) = params.get("format") else {
        return StatusCode::BAD_REQUEST.into_response();
    };

    // 1. Parse format from path param — only epub and kepub are served via Kobo.
    let format = match format_str.as_str() {
        "epub" => FileFormat::Epub,
        "kepub" => FileFormat::Kepub,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };

    // 2. Reconstruct the full BookToken by prepending the `BK_` prefix.
    let full_token = format!("BK_{book_token_str}");
    let Ok(token) = BookToken::from_str(&full_token) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    tracing::debug!(device_id = kobo.device.id, book_token = %full_token, format = ?format, "Download book requested");

    // 3. Look up the book.
    let book = match core_services.book_service.find_book_by_token(&token).await {
        Ok(Some(b)) => b,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::error!(error = ?e, "find_book_by_token failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // 4. Fetch files and split by role for the requested format.
    let files = match core_services.book_service.files_for_book(book.id).await {
        Ok(f) => f,
        Err(e) => {
            tracing::error!(error = ?e, "files_for_book failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let enriched = files.iter().find(|f| f.format == format && f.file_role == FileRole::Enriched);
    let original = files.iter().find(|f| f.format == format && f.file_role == FileRole::Original);

    if enriched.is_none() && original.is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }

    // 5. Resolve the file path; if enriched isn't on disk yet fall back to the
    //    original in the flat Originals/ directory.
    let (file_size, fs_file) = if let Some(enriched_file) = enriched {
        let enriched_path = core_services.library_store.resolve(&enriched_file.path);
        match File::open(&enriched_path).await {
            Ok(f) => (enriched_file.file_size, f),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Enriched record exists but file not on disk — fall back to original.
                let Some(orig_file) = original else {
                    return StatusCode::NOT_FOUND.into_response();
                };
                let orig_path = core_services.library_store.resolve(&orig_file.path);
                match File::open(&orig_path).await {
                    Ok(f) => (orig_file.file_size, f),
                    Err(_) => return StatusCode::NOT_FOUND.into_response(),
                }
            }
            Err(e) => {
                tracing::error!(error = ?e, "failed to open enriched file");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
    } else {
        // No enriched record — serve the original directly.
        let Some(orig_file) = original else {
            return StatusCode::NOT_FOUND.into_response();
        };
        let orig_path = core_services.library_store.resolve(&orig_file.path);
        match File::open(&orig_path).await {
            Ok(f) => (orig_file.file_size, f),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return StatusCode::NOT_FOUND.into_response();
            }
            Err(e) => {
                tracing::error!(error = ?e, "failed to open original file");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
    };

    tracing::debug!(
        device_id = kobo.device.id,
        book_id = book.id,
        format = ?format,
        "kobo download"
    );

    // 6. Build Content-Disposition filename. Kepub must have .kepub.epub extension
    //    so the Kobo recognises it.
    let ext = match format {
        FileFormat::Kepub => "kepub.epub",
        // FileFormat::Epub => "epub",
        _ => "epub",
    };
    let safe_title: String = book
        .title
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' })
        .collect();
    let filename = format!("{safe_title}.{ext}");
    let content_disposition = format!("attachment; filename=\"{filename}\"");

    // 7. Stream the file.
    let stream = ReaderStream::new(fs_file);
    let body = Body::from_stream(stream);

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/epub+zip")
        .header(header::CONTENT_DISPOSITION, content_disposition)
        .header(header::CONTENT_LENGTH, file_size as u64)
        .body(body)
        .unwrap()
}
