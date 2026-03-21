//! OPDS feed handlers for root catalog, all books, shelves, and file serving.

use std::sync::Arc;

use axum::{
    Extension,
    body::Body,
    extract::{Path, Query},
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use bb_core::{
    CoreServices,
    book::{AuthorToken, Book, BookQuery, BookToken, FileFormat, FileRole, SeriesToken},
    filter::{BookFilter, FilterRule, TextOp},
    shelf::ShelfType,
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

#[derive(Deserialize)]
pub struct SearchParams {
    pub q: Option<String>,
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
        .with_link(
            AtomLink::new(rel::SEARCH, "/opds/search/description.xml")
                .with_type(mime::OPENSEARCH)
                .with_title("Search BookBoss"),
        )
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

    let filter = BookQuery::default();

    let offset = params.start;
    let Ok(books) = core_services.book_service.list_books(&filter, offset, Some(PAGE_SIZE + 1)).await else {
        return Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(axum::body::Body::empty())
            .unwrap();
    };

    let has_next = books.len() as u64 > PAGE_SIZE;
    let page_books = if has_next { &books[..PAGE_SIZE as usize] } else { &books };

    let mut feed = AtomFeed::new("urn:bookboss:opds:all", "All Books", now)
        .with_link(AtomLink::new(rel::SELF, format_all_url(offset)).with_type(mime::ACQUISITION))
        .with_link(AtomLink::new(rel::START, "/opds/").with_type(mime::NAVIGATION));

    if has_next {
        let next_offset = offset.unwrap_or(0) + PAGE_SIZE;
        feed = feed.with_link(AtomLink::new(rel::NEXT, format_all_url(Some(next_offset))).with_type(mime::ACQUISITION));
    }

    for book in page_books {
        let entry = book_to_entry(book, &core_services).await;
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

    let Ok(shelf_list) = core_services.shelf_service.list_shelves_for_user(opds_user.user.id).await else {
        return Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(axum::body::Body::empty())
            .unwrap();
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
    format_paginated_url("/opds/all", start)
}

/// Adds acquisition links for available book files.
pub(crate) fn add_file_links(mut entry: AtomEntry, book_token: &str, files: &[bb_core::book::BookFile]) -> AtomEntry {
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

/// `GET /opds/shelves/{shelf_token}` — Books on a shelf (acquisition feed).
pub async fn shelf_books(
    opds_user: OpdsUser,
    axum::extract::Path(shelf_token_str): axum::extract::Path<String>,
    Query(params): Query<PaginationParams>,
    Extension(core_services): Extension<Arc<CoreServices>>,
) -> Response {
    let now = Utc::now();
    let user_id = opds_user.user.id;

    let shelf_token: bb_core::shelf::ShelfToken = match shelf_token_str.parse() {
        Ok(t) => t,
        Err(_) => return error_response(StatusCode::BAD_REQUEST),
    };

    let Ok(shelf) = core_services.shelf_service.get_shelf(&shelf_token, user_id).await else {
        return error_response(StatusCode::NOT_FOUND);
    };

    let offset = params.start;
    let books: Vec<Book> = if shelf.shelf_type == ShelfType::Smart {
        match core_services
            .shelf_service
            .books_for_filter(&shelf_token, user_id, offset, Some(PAGE_SIZE + 1), None)
            .await
        {
            Ok(b) => b,
            Err(_) => return error_response(StatusCode::INTERNAL_SERVER_ERROR),
        }
    } else {
        let Ok(entries) = core_services
            .shelf_service
            .books_for_shelf(&shelf_token, user_id, offset, Some(PAGE_SIZE + 1))
            .await
        else {
            return error_response(StatusCode::INTERNAL_SERVER_ERROR);
        };
        let mut result = Vec::with_capacity(entries.len());
        for entry in &entries {
            if let Ok(Some(book)) = core_services.book_service.find_book_by_token(&BookToken::new(entry.book_id)).await {
                result.push(book);
            }
        }
        result
    };

    let has_next = books.len() as u64 > PAGE_SIZE;
    let page_books = if has_next { &books[..PAGE_SIZE as usize] } else { &books };

    let self_url = format_shelf_url(&shelf_token_str, offset);
    let mut feed = AtomFeed::new(format!("urn:bookboss:shelf:{}", shelf.token), &shelf.name, now)
        .with_link(AtomLink::new(rel::SELF, &self_url).with_type(mime::ACQUISITION))
        .with_link(AtomLink::new(rel::START, "/opds/").with_type(mime::NAVIGATION));

    if has_next {
        let next_offset = offset.unwrap_or(0) + PAGE_SIZE;
        feed = feed.with_link(AtomLink::new(rel::NEXT, format_shelf_url(&shelf_token_str, Some(next_offset))).with_type(mime::ACQUISITION));
    }

    for book in page_books {
        let entry = book_to_entry(book, &core_services).await;
        feed = feed.with_entry(entry);
    }

    match feed.to_xml() {
        Ok(xml) => xml_response(xml),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// `GET /opds/authors` — Authors (navigation feed, paginated).
pub async fn authors(opds_user: OpdsUser, Query(params): Query<PaginationParams>, Extension(core_services): Extension<Arc<CoreServices>>) -> Response {
    let _ = &opds_user;
    let now = Utc::now();

    let Ok(author_list) = core_services.book_service.list_authors(params.start, Some(PAGE_SIZE + 1)).await else {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR);
    };

    let has_next = author_list.len() as u64 > PAGE_SIZE;
    let page_authors = if has_next { &author_list[..PAGE_SIZE as usize] } else { &author_list };

    let mut feed = AtomFeed::new("urn:bookboss:opds:authors", "Authors", now)
        .with_link(AtomLink::new(rel::SELF, format_paginated_url("/opds/authors", params.start)).with_type(mime::NAVIGATION))
        .with_link(AtomLink::new(rel::START, "/opds/").with_type(mime::NAVIGATION));

    if has_next {
        if let Some(last) = page_authors.last() {
            feed = feed.with_link(AtomLink::new(rel::NEXT, format_paginated_url("/opds/authors", Some(last.id + 1))).with_type(mime::NAVIGATION));
        }
    }

    for author in page_authors {
        let entry = AtomEntry::new(format!("urn:bookboss:author:{}", author.token), &author.name, author.updated_at)
            .with_link(AtomLink::new(rel::SUBSECTION, format!("/opds/authors/{}", author.id)).with_type(mime::ACQUISITION));
        feed = feed.with_entry(entry);
    }

    match feed.to_xml() {
        Ok(xml) => xml_response(xml),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// `GET /opds/authors/{id}` — Books by author (acquisition feed, paginated).
pub async fn author_books(
    opds_user: OpdsUser,
    Path(author_id): Path<u64>,
    Query(params): Query<PaginationParams>,
    Extension(core_services): Extension<Arc<CoreServices>>,
) -> Response {
    let _ = &opds_user;
    let now = Utc::now();

    let author = match core_services.book_service.find_author_by_token(&AuthorToken::new(author_id)).await {
        Ok(Some(a)) => a,
        Ok(None) => return error_response(StatusCode::NOT_FOUND),
        Err(_) => return error_response(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let filter = BookQuery {
        author_id: Some(author_id),
        ..Default::default()
    };

    let offset = params.start;
    let Ok(books) = core_services.book_service.list_books(&filter, offset, Some(PAGE_SIZE + 1)).await else {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR);
    };

    let has_next = books.len() as u64 > PAGE_SIZE;
    let page_books = if has_next { &books[..PAGE_SIZE as usize] } else { &books };
    let base_url = format!("/opds/authors/{author_id}");

    let mut feed = AtomFeed::new(format!("urn:bookboss:author:{}", author.token), &author.name, now)
        .with_link(AtomLink::new(rel::SELF, format_paginated_url(&base_url, offset)).with_type(mime::ACQUISITION))
        .with_link(AtomLink::new(rel::START, "/opds/").with_type(mime::NAVIGATION));

    if has_next {
        let next_offset = offset.unwrap_or(0) + PAGE_SIZE;
        feed = feed.with_link(AtomLink::new(rel::NEXT, format_paginated_url(&base_url, Some(next_offset))).with_type(mime::ACQUISITION));
    }

    for book in page_books {
        let entry = book_to_entry(book, &core_services).await;
        feed = feed.with_entry(entry);
    }

    match feed.to_xml() {
        Ok(xml) => xml_response(xml),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// `GET /opds/series` — Series (navigation feed, paginated).
pub async fn series_list(opds_user: OpdsUser, Query(params): Query<PaginationParams>, Extension(core_services): Extension<Arc<CoreServices>>) -> Response {
    let _ = &opds_user;
    let now = Utc::now();

    let Ok(all_series) = core_services.book_service.list_series(params.start, Some(PAGE_SIZE + 1)).await else {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR);
    };

    let has_next = all_series.len() as u64 > PAGE_SIZE;
    let page_series = if has_next { &all_series[..PAGE_SIZE as usize] } else { &all_series };

    let mut feed = AtomFeed::new("urn:bookboss:opds:series", "Series", now)
        .with_link(AtomLink::new(rel::SELF, format_paginated_url("/opds/series", params.start)).with_type(mime::NAVIGATION))
        .with_link(AtomLink::new(rel::START, "/opds/").with_type(mime::NAVIGATION));

    if has_next {
        if let Some(last) = page_series.last() {
            feed = feed.with_link(AtomLink::new(rel::NEXT, format_paginated_url("/opds/series", Some(last.id + 1))).with_type(mime::NAVIGATION));
        }
    }

    for series in page_series {
        let entry = AtomEntry::new(format!("urn:bookboss:series:{}", series.token), &series.name, series.updated_at)
            .with_link(AtomLink::new(rel::SUBSECTION, format!("/opds/series/{}", series.id)).with_type(mime::ACQUISITION));
        feed = feed.with_entry(entry);
    }

    match feed.to_xml() {
        Ok(xml) => xml_response(xml),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// `GET /opds/series/{id}` — Books in a series (acquisition feed, paginated).
pub async fn series_books(
    opds_user: OpdsUser,
    Path(series_id): Path<u64>,
    Query(params): Query<PaginationParams>,
    Extension(core_services): Extension<Arc<CoreServices>>,
) -> Response {
    let _ = &opds_user;
    let now = Utc::now();

    let series = match core_services.book_service.find_series_by_token(&SeriesToken::new(series_id)).await {
        Ok(Some(s)) => s,
        Ok(None) => return error_response(StatusCode::NOT_FOUND),
        Err(_) => return error_response(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let filter = BookQuery {
        series_id: Some(series_id),
        ..Default::default()
    };

    let offset = params.start;
    let Ok(books) = core_services.book_service.list_books(&filter, offset, Some(PAGE_SIZE + 1)).await else {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR);
    };

    let has_next = books.len() as u64 > PAGE_SIZE;
    let page_books = if has_next { &books[..PAGE_SIZE as usize] } else { &books };
    let base_url = format!("/opds/series/{series_id}");

    let mut feed = AtomFeed::new(format!("urn:bookboss:series:{}", series.token), &series.name, now)
        .with_link(AtomLink::new(rel::SELF, format_paginated_url(&base_url, offset)).with_type(mime::ACQUISITION))
        .with_link(AtomLink::new(rel::START, "/opds/").with_type(mime::NAVIGATION));

    if has_next {
        let next_offset = offset.unwrap_or(0) + PAGE_SIZE;
        feed = feed.with_link(AtomLink::new(rel::NEXT, format_paginated_url(&base_url, Some(next_offset))).with_type(mime::ACQUISITION));
    }

    for book in page_books {
        let entry = book_to_entry(book, &core_services).await;
        feed = feed.with_entry(entry);
    }

    match feed.to_xml() {
        Ok(xml) => xml_response(xml),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// `GET /opds/search/description.xml` — OpenSearch description document.
pub async fn search_description(_opds_user: OpdsUser) -> Response {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<OpenSearchDescription xmlns="http://a9.com/-/spec/opensearch/1.1/">
  <ShortName>BookBoss</ShortName>
  <Description>Search the BookBoss library by title or author</Description>
  <Url type="application/atom+xml;profile=opds-catalog;kind=acquisition"
       template="/opds/search?q={searchTerms}&amp;start={startIndex?}"/>
</OpenSearchDescription>"#;

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static(mime::OPENSEARCH))
        .header(header::CACHE_CONTROL, HeaderValue::from_static("private, no-cache"))
        .body(Body::from(xml))
        .unwrap()
}

/// `GET /opds/search?q=...` — Search acquisition feed.
pub async fn search(opds_user: OpdsUser, Query(params): Query<SearchParams>, Extension(core_services): Extension<Arc<CoreServices>>) -> Response {
    let _ = &opds_user;
    let now = Utc::now();

    let q = match params.q.as_deref().filter(|s| !s.is_empty()) {
        Some(q) => q.to_string(),
        None => {
            return xml_response(
                AtomFeed::new("urn:bookboss:opds:search", "Search Results", now)
                    .with_link(AtomLink::new(rel::SELF, "/opds/search").with_type(mime::ACQUISITION))
                    .with_link(AtomLink::new(rel::START, "/opds/").with_type(mime::NAVIGATION))
                    .to_xml()
                    .unwrap_or_default(),
            );
        }
    };

    let filter = BookFilter::Rule(FilterRule::TitleText {
        op: TextOp::Contains,
        value: q.clone(),
    })
    .or(BookFilter::Rule(FilterRule::AuthorText {
        op: TextOp::Contains,
        value: q.clone(),
    }));

    let offset = params.start;
    let Ok(books) = core_services.library_service.search_books(&filter, offset, Some(PAGE_SIZE + 1)).await else {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR);
    };

    let has_next = books.len() as u64 > PAGE_SIZE;
    let page_books = if has_next { &books[..PAGE_SIZE as usize] } else { &books };

    let self_url = format_search_url(&q, offset);
    let mut feed = AtomFeed::new("urn:bookboss:opds:search", format!("Search: {q}"), now)
        .with_link(AtomLink::new(rel::SELF, &self_url).with_type(mime::ACQUISITION))
        .with_link(AtomLink::new(rel::START, "/opds/").with_type(mime::NAVIGATION));

    if has_next {
        let next_offset = offset.unwrap_or(0) + PAGE_SIZE;
        feed = feed.with_link(AtomLink::new(rel::NEXT, format_search_url(&q, Some(next_offset))).with_type(mime::ACQUISITION));
    }

    for book in page_books {
        let entry = book_to_entry(book, &core_services).await;
        feed = feed.with_entry(entry);
    }

    match feed.to_xml() {
        Ok(xml) => xml_response(xml),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

fn format_search_url(q: &str, start: Option<u64>) -> String {
    let encoded_q = q.replace('&', "%26").replace(' ', "+");
    match start {
        Some(s) => format!("/opds/search?q={encoded_q}&start={s}"),
        None => format!("/opds/search?q={encoded_q}"),
    }
}

fn format_paginated_url(base: &str, start: Option<u64>) -> String {
    match start {
        Some(s) => format!("{base}?start={s}"),
        None => base.to_string(),
    }
}

fn format_shelf_url(token: &str, start: Option<u64>) -> String {
    format_paginated_url(&format!("/opds/shelves/{token}"), start)
}

fn error_response(status: StatusCode) -> Response {
    Response::builder().status(status).body(axum::body::Body::empty()).unwrap()
}

/// Builds an OPDS acquisition entry from a Book, resolving authors, files, and
/// cover.
async fn book_to_entry(book: &Book, core_services: &Arc<CoreServices>) -> AtomEntry {
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

    let token_str = book.token.to_string();
    entry = add_file_links(entry, &token_str, &files);
    entry = add_cover_link(entry, &token_str, book.cover_path.as_deref());

    entry
}

static BLANK_COVER: &[u8] = include_bytes!("../../../assets/BlankCover.png");

/// `GET /opds/covers/{book_token}` — Serve a book's cover image.
pub async fn serve_cover(Path(book_token_str): Path<String>, _opds_user: OpdsUser, Extension(core_services): Extension<Arc<CoreServices>>) -> Response {
    let token: BookToken = match book_token_str.parse() {
        Ok(t) => t,
        Err(_) => return error_response(StatusCode::BAD_REQUEST),
    };

    let book = match core_services.book_service.find_book_by_token(&token).await {
        Ok(Some(b)) => b,
        Ok(None) => return error_response(StatusCode::NOT_FOUND),
        Err(_) => return error_response(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let Some(filename) = book.cover_path else {
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, HeaderValue::from_static("image/png"))
            .header(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"))
            .body(Body::from(BLANK_COVER))
            .unwrap();
    };

    let path = core_services.library_store.cover_path(&token, &filename);

    match tokio::fs::read(&path).await {
        Ok(data) => {
            let content_type = cover_content_type(&filename);
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, HeaderValue::from_static(content_type))
                .header(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"))
                .body(Body::from(data))
                .unwrap()
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, HeaderValue::from_static("image/png"))
            .header(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"))
            .body(Body::from(BLANK_COVER))
            .unwrap(),
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// `GET /opds/download/{book_token}/{format}` — Download a book file.
pub async fn serve_download(
    Path((book_token_str, format_str)): Path<(String, String)>,
    _opds_user: OpdsUser,
    Extension(core_services): Extension<Arc<CoreServices>>,
) -> Response {
    let token: BookToken = match book_token_str.parse() {
        Ok(t) => t,
        Err(_) => return error_response(StatusCode::BAD_REQUEST),
    };

    let format: FileFormat = match format_str.to_lowercase().parse() {
        Ok(f) => f,
        Err(_) => return error_response(StatusCode::BAD_REQUEST),
    };

    let book = match core_services.book_service.find_book_by_token(&token).await {
        Ok(Some(b)) => b,
        Ok(None) => return error_response(StatusCode::NOT_FOUND),
        Err(_) => return error_response(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let Ok(files) = core_services.book_service.files_for_book(book.id).await else {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR);
    };

    let enriched_file = files.iter().find(|f| f.format == format && f.file_role == FileRole::Enriched);
    let original_file = files.iter().find(|f| f.format == format && f.file_role == FileRole::Original);

    if enriched_file.is_none() && original_file.is_none() {
        return error_response(StatusCode::NOT_FOUND);
    }

    let ext = format.extension();

    // Try the enriched file first; fall back to the original if not yet on disk.
    let data = if let Some(enriched) = enriched_file {
        let enriched_path = core_services.library_store.resolve(&enriched.path);
        match tokio::fs::read(&enriched_path).await {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let Some(original) = original_file else {
                    return error_response(StatusCode::NOT_FOUND);
                };
                let orig_path = core_services.library_store.resolve(&original.path);
                match tokio::fs::read(&orig_path).await {
                    Ok(d) => d,
                    Err(_) => return error_response(StatusCode::NOT_FOUND),
                }
            }
            Err(_) => return error_response(StatusCode::INTERNAL_SERVER_ERROR),
        }
    } else {
        let Some(original) = original_file else {
            return error_response(StatusCode::NOT_FOUND);
        };
        let orig_path = core_services.library_store.resolve(&original.path);
        match tokio::fs::read(&orig_path).await {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return error_response(StatusCode::NOT_FOUND),
            Err(_) => return error_response(StatusCode::INTERNAL_SERVER_ERROR),
        }
    };

    let download_name = format!("{}.{ext}", sanitize_filename(&book.title));
    let content_disposition = format!("attachment; filename=\"{download_name}\"");

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static(format.content_type()))
        .header(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&content_disposition).unwrap_or(HeaderValue::from_static("attachment")),
        )
        .header(header::CACHE_CONTROL, HeaderValue::from_static("private, no-cache"))
        .body(Body::from(data))
        .unwrap()
}

fn cover_content_type(filename: &str) -> &'static str {
    let ext = filename.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/jpeg",
    }
}

fn sanitize_filename(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect()
}
