use std::sync::Arc;

use axum::{
    Extension,
    body::Body,
    extract::{Path, Query},
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use bb_core::{CoreServices, book::BookToken};

use super::AuthSession;

static BLANK_COVER: &[u8] = include_bytes!("../../assets/BlankCover.png");

#[derive(serde::Deserialize)]
pub(crate) struct CoverQuery {
    /// When true, skip thumbnail and serve the full-size cover directly.
    #[serde(default)]
    pub full: Option<bool>,
}

/// Serves a cover image for a given book token.
///
/// Route: `GET /api/v1/covers/:book_token`
///
/// Requires authentication. Looks up the book's cover filename from the
/// database, reads the file from the library store, and returns it with the
/// appropriate `Content-Type` header. If the book has no cover, serves the
/// built-in blank cover PNG.
///
/// Query parameters:
/// - `full=true` — serve the full-size cover instead of the thumbnail
pub(crate) async fn serve_cover(
    Path(book_token_str): Path<String>,
    Query(query): Query<CoverQuery>,
    auth_session: AuthSession,
    Extension(core_services): Extension<Arc<CoreServices>>,
) -> Response {
    let authenticated = auth_session.current_user.as_ref().is_some_and(|u| !u.username.is_empty());
    if !authenticated {
        return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
    }
    let token: BookToken = match book_token_str.parse() {
        Ok(t) => t,
        Err(_) => return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(),
    };

    let book = match core_services.book_service.find_book_by_token(token).await {
        Ok(Some(b)) => b,
        Ok(None) => return Response::builder().status(StatusCode::NOT_FOUND).body(Body::empty()).unwrap(),
        Err(e) if e.is_transient() => {
            return Response::builder().status(StatusCode::SERVICE_UNAVAILABLE).body(Body::empty()).unwrap();
        }
        Err(_) => return Response::builder().status(StatusCode::INTERNAL_SERVER_ERROR).body(Body::empty()).unwrap(),
    };

    if !book.has_cover {
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, HeaderValue::from_static("image/png"))
            .header(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"))
            .body(Body::from(BLANK_COVER))
            .unwrap();
    }

    let (data, content_type) = if query.full == Some(true) {
        // Full-size cover requested — skip thumbnail.
        let cover_path = core_services.file_store.cover_path(token);
        match tokio::fs::read(&cover_path).await {
            Ok(d) => (d, "image/jpeg"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, HeaderValue::from_static("image/png"))
                    .header(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"))
                    .body(Body::from(BLANK_COVER))
                    .unwrap();
            }
            Err(_) => return Response::builder().status(StatusCode::INTERNAL_SERVER_ERROR).body(Body::empty()).unwrap(),
        }
    } else {
        // Default: serve thumbnail if available, fall back to full-size cover.
        let thumb_path = core_services.file_store.thumbnail_path(token);
        match tokio::fs::read(&thumb_path).await {
            Ok(d) => (d, "image/jpeg"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let cover_path = core_services.file_store.cover_path(token);
                match tokio::fs::read(&cover_path).await {
                    Ok(d) => (d, "image/jpeg"),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        return Response::builder()
                            .status(StatusCode::OK)
                            .header(header::CONTENT_TYPE, HeaderValue::from_static("image/png"))
                            .header(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"))
                            .body(Body::from(BLANK_COVER))
                            .unwrap();
                    }
                    Err(_) => {
                        return Response::builder().status(StatusCode::INTERNAL_SERVER_ERROR).body(Body::empty()).unwrap();
                    }
                }
            }
            Err(_) => {
                return Response::builder().status(StatusCode::INTERNAL_SERVER_ERROR).body(Body::empty()).unwrap();
            }
        }
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static(content_type))
        .header(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"))
        .body(Body::from(data))
        .unwrap()
}
