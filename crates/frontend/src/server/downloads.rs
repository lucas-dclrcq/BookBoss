use std::sync::Arc;

use axum::{
    Extension,
    body::Body,
    extract::Path,
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use bb_core::{
    CoreServices,
    book::{AuthorToken, BookToken, FileFormat},
};

use super::AuthSession;

/// Serves a book file for download.
///
/// Route: `GET /api/v1/books/:book_token/download/:format`
///
/// Requires authentication. Resolves the book file path using the same slug
/// logic as the pipeline service, then streams the file with a
/// `Content-Disposition: attachment` header.
pub(crate) async fn serve_book_file(
    Path((book_token_str, format_str)): Path<(String, String)>,
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

    let format = match format_str.to_lowercase().as_str() {
        "epub" => FileFormat::Epub,
        "mobi" => FileFormat::Mobi,
        "azw3" => FileFormat::Azw3,
        "pdf" => FileFormat::Pdf,
        "cbz" => FileFormat::Cbz,
        _ => return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(),
    };

    let book = match core_services.book_service.find_book_by_token(&token).await {
        Ok(Some(b)) => b,
        Ok(None) => return Response::builder().status(StatusCode::NOT_FOUND).body(Body::empty()).unwrap(),
        Err(_) => return Response::builder().status(StatusCode::INTERNAL_SERVER_ERROR).body(Body::empty()).unwrap(),
    };

    // Verify the requested format exists for this book.
    let Ok(files) = core_services.book_service.files_for_book(book.id).await else {
        return Response::builder().status(StatusCode::INTERNAL_SERVER_ERROR).body(Body::empty()).unwrap();
    };
    if !files.iter().any(|f| f.format == format) {
        return Response::builder().status(StatusCode::NOT_FOUND).body(Body::empty()).unwrap();
    }

    // Compute slug: same logic as pipeline service — first author (by sort_order) +
    // title.
    let Ok(mut authors) = core_services.book_service.authors_for_book(book.id).await else {
        return Response::builder().status(StatusCode::INTERNAL_SERVER_ERROR).body(Body::empty()).unwrap();
    };
    authors.sort_by_key(|a| a.sort_order);

    let first_author_name = if let Some(ba) = authors.first() {
        match core_services.book_service.find_author_by_token(&AuthorToken::new(ba.author_id)).await {
            Ok(Some(a)) => Some(a.name),
            _ => None,
        }
    } else {
        None
    };

    let slug = match &first_author_name {
        Some(name) => format!("{}-{}", slugify(name), slugify(&book.title)),
        None => slugify(&book.title),
    };

    let ext = format_ext(&format);
    let path = core_services.library_store.book_file_path(&token, &slug, format.clone());

    let data = match tokio::fs::read(&path).await {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Response::builder().status(StatusCode::NOT_FOUND).body(Body::empty()).unwrap();
        }
        Err(_) => return Response::builder().status(StatusCode::INTERNAL_SERVER_ERROR).body(Body::empty()).unwrap(),
    };

    // Use the book title as the suggested download filename.
    let download_name = format!("{}.{ext}", sanitize_filename(&book.title));
    let content_disposition = format!("attachment; filename=\"{download_name}\"");

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static(content_type_for_format(&format)))
        .header(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&content_disposition).unwrap_or(HeaderValue::from_static("attachment")),
        )
        .header(header::CACHE_CONTROL, HeaderValue::from_static("private, no-cache"))
        .body(Body::from(data))
        .unwrap()
}

/// Mirrors the pipeline service slugify — filesystem-safe, lowercase,
/// hyphenated.
fn slugify(s: &str) -> String {
    let raw: String = s.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    raw.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-")
}

/// Produces a safe filename fragment from a book title (keeps alphanumerics,
/// spaces, and hyphens; replaces everything else with underscores).
fn sanitize_filename(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect()
}

fn format_ext(format: &FileFormat) -> &'static str {
    match format {
        FileFormat::Epub => "epub",
        FileFormat::Kepub => "kepub.epub",
        FileFormat::Mobi => "mobi",
        FileFormat::Azw3 => "azw3",
        FileFormat::Pdf => "pdf",
        FileFormat::Cbz => "cbz",
    }
}

fn content_type_for_format(format: &FileFormat) -> &'static str {
    match format {
        FileFormat::Epub => "application/epub+zip",
        FileFormat::Kepub => "application/epub+zip",
        FileFormat::Mobi => "application/x-mobipocket-ebook",
        FileFormat::Azw3 => "application/vnd.amazon.mobi8-ebook",
        FileFormat::Pdf => "application/pdf",
        FileFormat::Cbz => "application/vnd.comicbook+zip",
    }
}
