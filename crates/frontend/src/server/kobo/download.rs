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
    book::{AuthorToken, BookToken, FileFormat, FileRole, book_slug},
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
    let token = match BookToken::from_str(&full_token) {
        Ok(t) => t,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };

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

    // 5. Compute slug — same logic as the pipeline: first author (by sort_order) +
    //    title.
    let mut author_links = match core_services.book_service.authors_for_book(book.id).await {
        Ok(a) => a,
        Err(e) => {
            tracing::error!(error = ?e, "authors_for_book failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    author_links.sort_by_key(|a| a.sort_order);

    let first_author_name = if let Some(ba) = author_links.first() {
        match core_services.book_service.find_author_by_token(&AuthorToken::new(ba.author_id)).await {
            Ok(Some(a)) => Some(a.name),
            _ => None,
        }
    } else {
        None
    };

    let slug = book_slug(&book.title, first_author_name.as_deref());

    // 6. Resolve the file path; if enriched isn't on disk yet fall back to the
    //    original in the flat Originals/ directory.
    let (file_size, fs_file) = if enriched.is_some() {
        let enriched_path = core_services.library_store.book_file_path(&token, &slug, format.clone());
        match File::open(&enriched_path).await {
            Ok(f) => {
                let size = enriched.map(|e| e.file_size).unwrap_or(0);
                (size, f)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Enriched record exists but file not on disk — fall back to original.
                match original.and_then(|f| f.original_filename.as_deref()) {
                    Some(orig_filename) => {
                        let orig_path = core_services.library_store.original_file_path(orig_filename);
                        match File::open(&orig_path).await {
                            Ok(f) => {
                                let size = original.map(|o| o.file_size).unwrap_or(0);
                                (size, f)
                            }
                            Err(_) => return StatusCode::NOT_FOUND.into_response(),
                        }
                    }
                    None => return StatusCode::NOT_FOUND.into_response(),
                }
            }
            Err(e) => {
                tracing::error!(error = ?e, "failed to open enriched file");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
    } else {
        // No enriched record — serve the original directly.
        match original.and_then(|f| f.original_filename.as_deref()) {
            Some(orig_filename) => {
                let orig_path = core_services.library_store.original_file_path(orig_filename);
                match File::open(&orig_path).await {
                    Ok(f) => {
                        let size = original.map(|o| o.file_size).unwrap_or(0);
                        (size, f)
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        return StatusCode::NOT_FOUND.into_response();
                    }
                    Err(e) => {
                        tracing::error!(error = ?e, "failed to open original file");
                        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                    }
                }
            }
            None => return StatusCode::NOT_FOUND.into_response(),
        }
    };

    tracing::debug!(
        device_id = kobo.device.id,
        book_id = book.id,
        format = ?format,
        "kobo download"
    );

    // 7. Build Content-Disposition filename. Kepub must have .kepub.epub extension
    //    so the Kobo recognises it.
    let ext = match format {
        FileFormat::Epub => "epub",
        FileFormat::Kepub => "kepub.epub",
        _ => "epub",
    };
    let filename = format!("{slug}.{ext}");
    let content_disposition = format!("attachment; filename=\"{filename}\"");

    // 8. Stream the file.
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
