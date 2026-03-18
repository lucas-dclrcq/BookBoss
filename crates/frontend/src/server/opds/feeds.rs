//! OPDS feed handlers for root catalog, all books, and shelves.

use std::sync::Arc;

use axum::{
    Extension,
    extract::Query,
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use bb_core::{
    CoreServices,
    book::{AuthorToken, BookQuery, BookStatus},
};
use chrono::Utc;
use serde::Deserialize;

use super::{
    extractor::OpdsUser,
    xml::{AtomEntry, AtomFeed, AtomLink, mime, rel},
};

const PAGE_SIZE: u64 = 50;

#[derive(Deserialize)]
pub struct PaginationParams {
    pub start: Option<u64>,
}

fn xml_response(xml: String) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static(mime::ATOM_XML))
        .header(header::CACHE_CONTROL, HeaderValue::from_static("private, no-cache"))
        .body(axum::body::Body::from(xml))
        .unwrap()
}

/// `GET /opds/` — Root catalog (navigation feed).
pub async fn root(opds_user: OpdsUser) -> Response {
    let now = Utc::now();
    let _ = &opds_user;

    let feed = AtomFeed::new("urn:bookboss:opds:root", "BookBoss Catalog", now)
        .with_link(AtomLink::new(rel::SELF, "/opds/").with_type(mime::NAVIGATION))
        .with_link(AtomLink::new(rel::START, "/opds/").with_type(mime::NAVIGATION))
        .with_entry(
            AtomEntry::new("urn:bookboss:opds:all", "All Books", now)
                .with_content("Browse all books in the library")
                .with_link(AtomLink::new(rel::SUBSECTION, "/opds/all").with_type(mime::ACQUISITION)),
        )
        .with_entry(
            AtomEntry::new("urn:bookboss:opds:shelves", "Shelves", now)
                .with_content("Browse books by shelf")
                .with_link(AtomLink::new(rel::SUBSECTION, "/opds/shelves").with_type(mime::NAVIGATION)),
        )
        .with_entry(
            AtomEntry::new("urn:bookboss:opds:authors", "Authors", now)
                .with_content("Browse books by author")
                .with_link(AtomLink::new(rel::SUBSECTION, "/opds/authors").with_type(mime::NAVIGATION)),
        )
        .with_entry(
            AtomEntry::new("urn:bookboss:opds:series", "Series", now)
                .with_content("Browse books by series")
                .with_link(AtomLink::new(rel::SUBSECTION, "/opds/series").with_type(mime::NAVIGATION)),
        );

    match feed.to_xml() {
        Ok(xml) => xml_response(xml),
        Err(_) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(axum::body::Body::empty())
            .unwrap(),
    }
}

/// `GET /opds/all` — All available books (acquisition feed, paginated).
pub async fn all_books(opds_user: OpdsUser, Query(params): Query<PaginationParams>, Extension(core_services): Extension<Arc<CoreServices>>) -> Response {
    let _ = &opds_user;
    let now = Utc::now();

    let filter = BookQuery {
        status: Some(BookStatus::Available),
        ..Default::default()
    };

    let books = match core_services.book_service.list_books(&filter, params.start, Some(PAGE_SIZE + 1)).await {
        Ok(b) => b,
        Err(_) => {
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(axum::body::Body::empty())
                .unwrap();
        }
    };

    let has_next = books.len() as u64 > PAGE_SIZE;
    let page_books = if has_next { &books[..PAGE_SIZE as usize] } else { &books };

    let mut feed = AtomFeed::new("urn:bookboss:opds:all", "All Books", now)
        .with_link(AtomLink::new(rel::SELF, format_all_url(params.start)).with_type(mime::ACQUISITION))
        .with_link(AtomLink::new(rel::START, "/opds/").with_type(mime::NAVIGATION));

    if has_next {
        if let Some(last) = page_books.last() {
            feed = feed.with_link(AtomLink::new(rel::NEXT, format_all_url(Some(last.id + 1))).with_type(mime::ACQUISITION));
        }
    }

    for book in page_books {
        let mut book_authors = core_services.book_service.authors_for_book(book.id).await.unwrap_or_default();
        book_authors.sort_by_key(|a| a.sort_order);
        let files = core_services.book_service.files_for_book(book.id).await.unwrap_or_default();

        let mut entry = AtomEntry::new(format!("urn:bookboss:book:{}", book.token), &book.title, book.updated_at);

        if let Some(ref desc) = book.description {
            entry = entry.with_content(desc);
        }

        for ba in &book_authors {
            if let Ok(Some(author)) = core_services.book_service.find_author_by_token(&AuthorToken::new(ba.author_id)).await {
                entry = entry.with_author(&author.name);
            }
        }

        entry = add_file_links(entry, &book.token.to_string(), &files);
        entry = add_cover_link(entry, &book.token.to_string(), book.cover_path.as_deref());

        feed = feed.with_entry(entry);
    }

    match feed.to_xml() {
        Ok(xml) => xml_response(xml),
        Err(_) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(axum::body::Body::empty())
            .unwrap(),
    }
}

/// `GET /opds/shelves` — User's shelves (navigation feed).
pub async fn shelves(opds_user: OpdsUser, Extension(core_services): Extension<Arc<CoreServices>>) -> Response {
    let now = Utc::now();

    let shelf_list = match core_services.shelf_service.list_shelves_for_user(opds_user.user.id).await {
        Ok(s) => s,
        Err(_) => {
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(axum::body::Body::empty())
                .unwrap();
        }
    };

    let mut feed = AtomFeed::new("urn:bookboss:opds:shelves", "Shelves", now)
        .with_link(AtomLink::new(rel::SELF, "/opds/shelves").with_type(mime::NAVIGATION))
        .with_link(AtomLink::new(rel::START, "/opds/").with_type(mime::NAVIGATION));

    for shelf in &shelf_list {
        let entry = AtomEntry::new(format!("urn:bookboss:shelf:{}", shelf.token), &shelf.name, shelf.updated_at)
            .with_link(AtomLink::new(rel::SUBSECTION, format!("/opds/shelves/{}", shelf.token)).with_type(mime::ACQUISITION));
        feed = feed.with_entry(entry);
    }

    match feed.to_xml() {
        Ok(xml) => xml_response(xml),
        Err(_) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(axum::body::Body::empty())
            .unwrap(),
    }
}

fn format_all_url(start: Option<u64>) -> String {
    match start {
        Some(s) => format!("/opds/all?start={s}"),
        None => "/opds/all".to_string(),
    }
}

/// Adds acquisition links for available book files.
pub(crate) fn add_file_links(mut entry: AtomEntry, book_token: &str, files: &[bb_core::book::BookFile]) -> AtomEntry {
    use bb_core::book::FileRole;

    // Group by format, prefer enriched over original.
    let mut seen_formats: Vec<String> = Vec::new();
    // Enriched files first.
    for file in files.iter().filter(|f| f.file_role == FileRole::Enriched) {
        let ext = file.format.extension().to_string();
        if !seen_formats.contains(&ext) {
            seen_formats.push(ext.clone());
            entry = entry.with_link(AtomLink::new(rel::ACQUISITION, format!("/opds/download/{book_token}/{ext}")).with_type(file.format.content_type()));
        }
    }
    // Then originals for formats not yet covered.
    for file in files.iter().filter(|f| f.file_role == FileRole::Original) {
        let ext = file.format.extension().to_string();
        if !seen_formats.contains(&ext) {
            seen_formats.push(ext.clone());
            entry = entry.with_link(AtomLink::new(rel::ACQUISITION, format!("/opds/download/{book_token}/{ext}")).with_type(file.format.content_type()));
        }
    }
    entry
}

/// Adds cover image link if the book has a cover.
pub(crate) fn add_cover_link(mut entry: AtomEntry, book_token: &str, cover_path: Option<&str>) -> AtomEntry {
    if cover_path.is_some() {
        let cover_url = format!("/opds/covers/{book_token}");
        entry = entry
            .with_link(AtomLink::new(rel::IMAGE, &cover_url).with_type("image/jpeg"))
            .with_link(AtomLink::new(rel::THUMBNAIL, &cover_url).with_type("image/jpeg"));
    }
    entry
}
