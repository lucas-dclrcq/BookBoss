use dioxus::prelude::*;
// ── Server-only imports
// ───────────────────────────────────────────────────────
#[cfg(feature = "server")]
use {
    crate::server::{AuthSession, AuthUser, BackendSessionPool},
    axum::http::Method,
    axum_session_auth::{Auth, Rights},
    base64::{Engine, engine::general_purpose::STANDARD as B64},
    bb_core::{
        CoreServices,
        book::{AuthorToken, BookToken, IdentifierType, PublisherToken, SeriesToken},
        import::ImportJobToken,
        pipeline::{BookEdit, ProviderBook},
        types::Capability,
        user::UserId,
    },
    rust_decimal::Decimal,
    std::{str::FromStr, sync::Arc},
};

use super::types::{BookEditFields, BookReviewData, IdentifierMap, PicklistData, ProviderResult, SeriesOption};

// ── Helpers (server only)
// ─────────────────────────────────────────────────────

#[cfg(feature = "server")]
fn cover_to_base64(bytes: &[u8]) -> String {
    let mime = if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        "image/png"
    } else if bytes.starts_with(&[0x47, 0x49, 0x46]) {
        "image/gif"
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "image/jpeg"
    };
    format!("data:{};base64,{}", mime, B64.encode(bytes))
}

#[cfg(feature = "server")]
pub(crate) fn image_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) && data.len() >= 24 {
        let w = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        let h = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
        return Some((w, h));
    }
    if (data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a")) && data.len() >= 10 {
        let w = u32::from(u16::from_le_bytes([data[6], data[7]]));
        let h = u32::from(u16::from_le_bytes([data[8], data[9]]));
        return Some((w, h));
    }
    if data.len() >= 30 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        match &data[12..16] {
            b"VP8 " => {
                let w = u32::from(u16::from_le_bytes([data[26], data[27]]) & 0x3FFF);
                let h = u32::from(u16::from_le_bytes([data[28], data[29]]) & 0x3FFF);
                return Some((w, h));
            }
            b"VP8L" if data.len() >= 25 => {
                let bits = u32::from_le_bytes([data[21], data[22], data[23], data[24]]);
                return Some(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1));
            }
            b"VP8X" => {
                let w = u32::from_le_bytes([data[24], data[25], data[26], 0]) + 1;
                let h = u32::from_le_bytes([data[27], data[28], data[29], 0]) + 1;
                return Some((w, h));
            }
            _ => {}
        }
    }
    if data.starts_with(&[0xFF, 0xD8]) {
        let mut i = 2usize;
        while i + 3 < data.len() {
            if data[i] != 0xFF {
                break;
            }
            let marker = data[i + 1];
            if matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) && i + 8 < data.len() {
                let h = u32::from(u16::from_be_bytes([data[i + 5], data[i + 6]]));
                let w = u32::from(u16::from_be_bytes([data[i + 7], data[i + 8]]));
                return Some((w, h));
            }
            let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
            if len < 2 {
                break;
            }
            i += 2 + len;
        }
    }
    None
}

#[cfg(feature = "server")]
fn provider_book_to_result(pb: &ProviderBook) -> ProviderResult {
    let meta = &pb.metadata;
    let title = meta.title.clone().unwrap_or_default();
    let description = meta.description.clone().unwrap_or_default();
    let published_date = meta.published_date.map(|y| y.to_string()).unwrap_or_default();
    let language = meta.language.clone().unwrap_or_default();
    let series_name = meta.series_name.clone().unwrap_or_default();
    let series_number = meta.series_number.as_ref().map(std::string::ToString::to_string).unwrap_or_default();
    let publisher_name = meta.publisher.clone().unwrap_or_default();
    let authors = meta.authors.as_deref().unwrap_or(&[]).iter().map(|a| a.name.clone()).collect::<Vec<_>>();
    let identifiers = meta
        .identifiers
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|i| (i.identifier_type.form_key().to_string(), i.value.clone()))
        .collect();
    // Provider cover bytes take priority; fall back to embedded cover in metadata.
    let cover_bytes = pb.cover_bytes.as_deref().or(meta.cover_bytes.as_deref());
    let cover_dimensions = cover_bytes.and_then(image_dimensions);
    let cover_thumbnail = cover_bytes.map(cover_to_base64);
    ProviderResult {
        title,
        description,
        published_date,
        language,
        series_name,
        series_number,
        publisher_name,
        page_count: String::new(),
        authors,
        identifiers,
        cover_thumbnail,
        cover_dimensions,
    }
}

// ── Review server functions
// ────────────────────────────────────────────────────

#[post(
    "/api/v1/incoming/review",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(super) async fn get_review_data(job_token: String) -> Result<BookReviewData, ServerFnError> {
    let current_user = auth_session.current_user.clone().unwrap_or_default();
    if !Auth::<AuthUser, UserId, BackendSessionPool>::build([Method::POST], true)
        .requires(Rights::any([Rights::permission(Capability::ApproveImports.as_str())]))
        .validate(&current_user, &Method::POST, None)
        .await
    {
        return Err(ServerFnError::new("Forbidden"));
    }

    let token: ImportJobToken = job_token.parse().map_err(|_| ServerFnError::new("Invalid token"))?;
    let import_service = &core_services.import_job_service;
    let book_service = &core_services.book_service;
    let pipeline_service = &core_services.pipeline_service;

    let job = import_service
        .find_by_token(token)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .ok_or_else(|| ServerFnError::new("Job not found"))?;

    let book_id = job.candidate_book_id.ok_or_else(|| ServerFnError::new("No candidate book"))?;
    let book = book_service
        .find_book_by_token(BookToken::new(book_id))
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .ok_or_else(|| ServerFnError::new("Book not found"))?;

    // Authors sorted by sort_order
    let book_author_links = {
        let mut links = book_service.authors_for_book(book.id).await.map_err(|e| ServerFnError::new(e.to_string()))?;
        links.sort_by_key(|a| a.sort_order);
        links
    };
    let mut author_names = Vec::with_capacity(book_author_links.len());
    for ba in &book_author_links {
        if let Some(author) = book_service
            .find_author_by_token(AuthorToken::new(ba.author_id))
            .await
            .map_err(|e| ServerFnError::new(e.to_string()))?
        {
            author_names.push(author.name);
        }
    }

    // Series name
    let series_name = if let Some(sid) = book.series_id {
        book_service
            .find_series_by_token(SeriesToken::new(sid))
            .await
            .map_err(|e| ServerFnError::new(e.to_string()))?
            .map(|s| s.name)
            .unwrap_or_default()
    } else {
        String::new()
    };

    // Publisher name
    let publisher_name = if let Some(pid) = book.publisher_id {
        book_service
            .find_publisher_by_token(PublisherToken::new(pid))
            .await
            .map_err(|e| ServerFnError::new(e.to_string()))?
            .map(|p| p.name)
            .unwrap_or_default()
    } else {
        String::new()
    };

    // Identifiers
    let raw_identifiers = book_service
        .identifiers_for_book(book.id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    let identifiers: IdentifierMap = raw_identifiers
        .iter()
        .map(|i| (i.identifier_type.form_key().to_string(), i.value.clone()))
        .collect();

    let provider_names = pipeline_service
        .list_provider_names()
        .into_iter()
        .map(std::string::ToString::to_string)
        .collect();

    // Read cover file to determine dimensions.
    let cover_dimensions = if let Some(filename) = &book.cover_path {
        let path = core_services.file_store.cover_path(book.token, filename);
        tokio::fs::read(&path).await.ok().and_then(|b| image_dimensions(&b))
    } else {
        None
    };

    let genres = book_service
        .genres_for_book(book.id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .into_iter()
        .map(|g| g.name)
        .collect();
    let tags = book_service
        .tags_for_book(book.id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .into_iter()
        .map(|t| t.name)
        .collect();

    Ok(BookReviewData {
        job_token: job.token.to_string(),
        book_token: book.token.to_string(),
        title: book.title,
        description: book.description.unwrap_or_default(),
        published_date: book.published_date.map(|y| y.to_string()).unwrap_or_default(),
        language: book.language.unwrap_or_default(),
        series_name,
        series_number: book.series_number.as_ref().map(std::string::ToString::to_string).unwrap_or_default(),
        publisher_name,
        page_count: book.page_count.map(|p| p.to_string()).unwrap_or_default(),
        authors: author_names,
        genres,
        tags,
        identifiers,
        provider_names,
        cover_dimensions,
    })
}

#[post(
    "/api/v1/incoming/review/fetch",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(super) async fn fetch_provider_metadata(
    job_token: String,
    provider_name: String,
    title: String,
    authors: Vec<String>,
    identifiers: IdentifierMap,
) -> Result<Option<ProviderResult>, ServerFnError> {
    let current_user = auth_session.current_user.clone().unwrap_or_default();
    if !Auth::<AuthUser, UserId, BackendSessionPool>::build([Method::POST], true)
        .requires(Rights::any([Rights::permission(Capability::ApproveImports.as_str())]))
        .validate(&current_user, &Method::POST, None)
        .await
    {
        return Err(ServerFnError::new("Forbidden"));
    }

    let token: ImportJobToken = job_token.parse().map_err(|_| ServerFnError::new("Invalid token"))?;
    let temp_dir = std::env::temp_dir();

    let parsed_identifiers: Vec<(IdentifierType, String)> = identifiers
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .filter_map(|(k, v)| IdentifierType::from_form_key(&k).map(|t| (t, v)))
        .collect();

    let title = if title.is_empty() { None } else { Some(title) };
    let result = core_services
        .pipeline_service
        .fetch_from_provider(&provider_name, title, authors, parsed_identifiers, &token.to_string(), &temp_dir)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(result.as_ref().map(provider_book_to_result))
}

#[put(
    "/api/v1/incoming/review/approve",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(super) async fn approve_book(fields: BookEditFields) -> Result<(), ServerFnError> {
    let current_user = auth_session.current_user.clone().unwrap_or_default();
    if !Auth::<AuthUser, UserId, BackendSessionPool>::build([Method::PUT], true)
        .requires(Rights::any([Rights::permission(Capability::ApproveImports.as_str())]))
        .validate(&current_user, &Method::PUT, None)
        .await
    {
        return Err(ServerFnError::new("Forbidden"));
    }

    let token: ImportJobToken = fields.job_token.parse().map_err(|_| ServerFnError::new("Invalid token"))?;

    let authors = fields.authors;

    let identifiers: Vec<(IdentifierType, String)> = fields
        .identifiers
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .filter_map(|(k, v)| IdentifierType::from_form_key(&k).map(|t| (t, v)))
        .collect();

    let edit = BookEdit {
        title: fields.title,
        description: if fields.description.is_empty() { None } else { Some(fields.description) },
        published_date: fields.published_date.parse::<i32>().ok(),
        language: if fields.language.is_empty() { None } else { Some(fields.language) },
        series_name: if fields.series_name.is_empty() { None } else { Some(fields.series_name) },
        series_number: Decimal::from_str(&fields.series_number).ok(),
        publisher_name: if fields.publisher_name.is_empty() {
            None
        } else {
            Some(fields.publisher_name)
        },
        page_count: fields.page_count.parse::<i32>().ok(),
        authors,
        identifiers,
        use_fetched_cover: fields.use_fetched_cover,
        genres: fields.genres,
        tags: fields.tags,
    };

    let temp_dir = std::env::temp_dir();
    core_services
        .pipeline_service
        .approve_job(token, edit, &temp_dir)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(())
}

#[put(
    "/api/v1/incoming/review/reject",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(super) async fn reject_review_book(job_token: String) -> Result<(), ServerFnError> {
    let current_user = auth_session.current_user.clone().unwrap_or_default();
    if !Auth::<AuthUser, UserId, BackendSessionPool>::build([Method::PUT], true)
        .requires(Rights::any([Rights::permission(Capability::ApproveImports.as_str())]))
        .validate(&current_user, &Method::PUT, None)
        .await
    {
        return Err(ServerFnError::new("Forbidden"));
    }

    let token: ImportJobToken = job_token.parse().map_err(|_| ServerFnError::new("Invalid token"))?;
    let temp_dir = std::env::temp_dir();

    // Remove any temp cover that may have been fetched
    let cover_path = temp_dir.join("bookboss-covers").join(job_token);
    let _ = tokio::fs::remove_file(&cover_path).await;

    core_services
        .pipeline_service
        .reject_job(token)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(())
}

// ── Edit-metadata server functions
// ────────────────────────────────────────────

#[post(
    "/api/v1/books/edit/data",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn get_book_for_edit(book_token: String) -> Result<BookReviewData, ServerFnError> {
    let current_user = auth_session.current_user.clone().unwrap_or_default();
    if !Auth::<AuthUser, UserId, BackendSessionPool>::build([Method::POST], true)
        .requires(Rights::any([Rights::permission(Capability::EditBook.as_str())]))
        .validate(&current_user, &Method::POST, None)
        .await
    {
        return Err(ServerFnError::new("Forbidden"));
    }

    let book_service = &core_services.book_service;
    let pipeline_service = &core_services.pipeline_service;

    let token = BookToken::from_str(&book_token).map_err(|_| ServerFnError::new("Invalid book token"))?;
    let book = book_service
        .find_book_by_token(token)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .ok_or_else(|| ServerFnError::new("Book not found"))?;

    // Authors sorted by sort_order
    let book_author_links = {
        let mut links = book_service.authors_for_book(book.id).await.map_err(|e| ServerFnError::new(e.to_string()))?;
        links.sort_by_key(|a| a.sort_order);
        links
    };
    let mut author_names = Vec::with_capacity(book_author_links.len());
    for ba in &book_author_links {
        if let Some(author) = book_service
            .find_author_by_token(AuthorToken::new(ba.author_id))
            .await
            .map_err(|e| ServerFnError::new(e.to_string()))?
        {
            author_names.push(author.name);
        }
    }

    // Series name
    let series_name = if let Some(sid) = book.series_id {
        book_service
            .find_series_by_token(SeriesToken::new(sid))
            .await
            .map_err(|e| ServerFnError::new(e.to_string()))?
            .map(|s| s.name)
            .unwrap_or_default()
    } else {
        String::new()
    };

    // Publisher name
    let publisher_name = if let Some(pid) = book.publisher_id {
        book_service
            .find_publisher_by_token(PublisherToken::new(pid))
            .await
            .map_err(|e| ServerFnError::new(e.to_string()))?
            .map(|p| p.name)
            .unwrap_or_default()
    } else {
        String::new()
    };

    // Identifiers
    let raw_identifiers = book_service
        .identifiers_for_book(book.id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    let identifiers: IdentifierMap = raw_identifiers
        .iter()
        .map(|i| (i.identifier_type.form_key().to_string(), i.value.clone()))
        .collect();

    let provider_names = pipeline_service
        .list_provider_names()
        .into_iter()
        .map(std::string::ToString::to_string)
        .collect();

    // Cover dimensions
    let cover_dimensions = if let Some(filename) = &book.cover_path {
        let path = core_services.file_store.cover_path(book.token, filename);
        tokio::fs::read(&path).await.ok().and_then(|b| image_dimensions(&b))
    } else {
        None
    };

    let genres = book_service
        .genres_for_book(book.id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .into_iter()
        .map(|g| g.name)
        .collect();
    let tags = book_service
        .tags_for_book(book.id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .into_iter()
        .map(|t| t.name)
        .collect();

    Ok(BookReviewData {
        job_token: String::new(),
        book_token: book.token.to_string(),
        title: book.title,
        description: book.description.unwrap_or_default(),
        published_date: book.published_date.map(|y| y.to_string()).unwrap_or_default(),
        language: book.language.unwrap_or_default(),
        series_name,
        series_number: book.series_number.as_ref().map(std::string::ToString::to_string).unwrap_or_default(),
        publisher_name,
        page_count: book.page_count.map(|p| p.to_string()).unwrap_or_default(),
        authors: author_names,
        genres,
        tags,
        identifiers,
        provider_names,
        cover_dimensions,
    })
}

#[post(
    "/api/v1/books/edit/fetch",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(super) async fn fetch_provider_for_edit(
    book_token: String,
    provider_name: String,
    title: String,
    authors: Vec<String>,
    identifiers: IdentifierMap,
) -> Result<Option<ProviderResult>, ServerFnError> {
    let current_user = auth_session.current_user.clone().unwrap_or_default();
    if !Auth::<AuthUser, UserId, BackendSessionPool>::build([Method::POST], true)
        .requires(Rights::any([Rights::permission(Capability::EditBook.as_str())]))
        .validate(&current_user, &Method::POST, None)
        .await
    {
        return Err(ServerFnError::new("Forbidden"));
    }

    let temp_dir = std::env::temp_dir();

    let parsed_identifiers: Vec<(IdentifierType, String)> = identifiers
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .filter_map(|(k, v)| IdentifierType::from_form_key(&k).map(|t| (t, v)))
        .collect();

    let title = if title.is_empty() { None } else { Some(title) };
    let result = core_services
        .pipeline_service
        .fetch_from_provider(&provider_name, title, authors, parsed_identifiers, &book_token, &temp_dir)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(result.as_ref().map(provider_book_to_result))
}

#[put(
    "/api/v1/books/edit",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(super) async fn save_library_book(book_token: String, fields: BookEditFields) -> Result<(), ServerFnError> {
    let current_user = auth_session.current_user.clone().unwrap_or_default();
    if !Auth::<AuthUser, UserId, BackendSessionPool>::build([Method::PUT], true)
        .requires(Rights::any([Rights::permission(Capability::EditBook.as_str())]))
        .validate(&current_user, &Method::PUT, None)
        .await
    {
        return Err(ServerFnError::new("Forbidden"));
    }

    let token = BookToken::from_str(&book_token).map_err(|_| ServerFnError::new("Invalid book token"))?;

    let authors = fields.authors;

    let identifiers: Vec<(IdentifierType, String)> = fields
        .identifiers
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .filter_map(|(k, v)| IdentifierType::from_form_key(&k).map(|t| (t, v)))
        .collect();

    let edit = BookEdit {
        title: fields.title,
        description: if fields.description.is_empty() { None } else { Some(fields.description) },
        published_date: fields.published_date.parse::<i32>().ok(),
        language: if fields.language.is_empty() { None } else { Some(fields.language) },
        series_name: if fields.series_name.is_empty() { None } else { Some(fields.series_name) },
        series_number: Decimal::from_str(&fields.series_number).ok(),
        publisher_name: if fields.publisher_name.is_empty() {
            None
        } else {
            Some(fields.publisher_name)
        },
        page_count: fields.page_count.parse::<i32>().ok(),
        authors,
        identifiers,
        use_fetched_cover: fields.use_fetched_cover,
        genres: fields.genres,
        tags: fields.tags,
    };

    let temp_dir = std::env::temp_dir();
    core_services
        .pipeline_service
        .edit_book(token, edit, &book_token, &temp_dir)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(())
}

// ── Picklist server function
// ──────────────────────────────────────────────────

#[post(
    "/api/v1/books/picklist",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn get_picklist_data((): ()) -> Result<PicklistData, ServerFnError> {
    let current_user = auth_session.current_user.clone().unwrap_or_default();
    if !Auth::<AuthUser, UserId, BackendSessionPool>::build([Method::POST], true)
        .requires(Rights::any([
            Rights::permission(Capability::ApproveImports.as_str()),
            Rights::permission(Capability::EditBook.as_str()),
        ]))
        .validate(&current_user, &Method::POST, None)
        .await
    {
        return Err(ServerFnError::new("Forbidden"));
    }
    let book_service = &core_services.book_service;

    let authors = book_service
        .list_all_authors()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .into_iter()
        .map(|a| a.name)
        .collect();

    let genres = book_service
        .list_all_genres()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .into_iter()
        .map(|g| g.name)
        .collect();

    let tags = book_service
        .list_all_tags()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .into_iter()
        .map(|t| t.name)
        .collect();

    let all_series = book_service.list_all_series().await.map_err(|e| ServerFnError::new(e.to_string()))?;

    let mut series_options = Vec::with_capacity(all_series.len());
    for s in all_series {
        let next = book_service.series_next_number(&s.name).await.map_err(|e| ServerFnError::new(e.to_string()))?;
        series_options.push(SeriesOption {
            name: s.name,
            next_number: next,
        });
    }

    let publishers = book_service
        .list_all_publishers()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .into_iter()
        .map(|p| p.name)
        .collect();

    Ok(PicklistData {
        authors,
        genres,
        tags,
        series: series_options,
        publishers,
    })
}
