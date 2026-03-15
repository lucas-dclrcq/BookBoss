//! `GET /kobo/{sync_token}/v1/image/{book_token}/{width}/{height}/{quality}/
//! {isGreyScale}/image.jpg` `GET /kobo/{sync_token}/v1/image/{book_token}/
//! {width}/{height}/{isGreyScale}/image.jpg`
//!
//! Serves the book cover as a JPEG image. Width, height, quality, and
//! greyscale params are accepted by the route but ignored — we serve the
//! stored cover at full resolution without resizing or conversion, matching
//! the behaviour of both Komga and Calibre-Web for locally-available covers.
//!
//! Only JPEG covers (`cover.jpg`) are served. Books whose cover was stored as
//! PNG, WebP, or GIF return 404 — on-the-fly conversion can be added later if
//! needed. The `/image.jpg` suffix in the route path implies JPEG is expected.

use std::{collections::HashMap, str::FromStr, sync::Arc};

use axum::{
    extract::Path,
    http::{StatusCode, header},
    response::IntoResponse,
};
use bb_core::{CoreServices, book::BookToken};

use super::KoboDevice;

// ── Handler
// ─────────────────────────────────────────────────────────────────

pub async fn handle(_kobo: KoboDevice, Path(params): Path<HashMap<String, String>>, core_services: Arc<CoreServices>) -> impl IntoResponse {
    let Some(book_token_str) = params.get("book_token") else {
        return StatusCode::BAD_REQUEST.into_response();
    };

    // Reconstruct the full BookToken by prepending the `BK_` prefix.
    let full_token = format!("BK_{book_token_str}");
    let token = match BookToken::from_str(&full_token) {
        Ok(t) => t,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };

    // Look up the book.
    let book = match core_services.book_service.find_book_by_token(&token).await {
        Ok(Some(b)) => b,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::error!(error = ?e, "find_book_by_token failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // Only serve JPEG covers. Non-JPEG formats (cover.png, cover.webp,
    // cover.gif) return 404 — the route implies JPEG via the /image.jpg suffix.
    let cover_filename = match book.cover_path.as_deref() {
        Some("cover.jpg") => "cover.jpg",
        _ => return StatusCode::NOT_FOUND.into_response(),
    };

    // Resolve and read the cover file.
    let cover_path = core_services.library_store.cover_path(&token, cover_filename);
    let bytes = match tokio::fs::read(&cover_path).await {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return StatusCode::NOT_FOUND.into_response();
        }
        Err(e) => {
            tracing::error!(error = ?e, "failed to read cover file");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "no-cache")],
        bytes,
    )
        .into_response()
}
