//! `GET /kobo/{sync_token}/v1/library/sync`
//!
//! Incremental and full-sync endpoint. The Kobo echoes back the cursor we
//! returned in the previous response's `x-kobo-synctoken` header; we decode
//! it to determine `since` and the keyset bookmark (`after_book_id`).
//!
//! # Response headers
//!
//! | Header              | Value                                              |
//! |---------------------|----------------------------------------------------|
//! | `x-kobo-synctoken`  | encoded cursor for the next request                |
//! | `x-kobo-sync`       | `"continue"` when more pages remain (absent when done) |
//!
//! Sources consulted: Komga (KoboController.kt, BookEntitlementDto.kt,
//! KoboBookMetadataDto.kt) and Calibre-Web (kobo.py).

use std::{collections::HashMap, sync::Arc};

use axum::{
    Json,
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::IntoResponse,
};
use bb_core::{
    CoreServices,
    book::{BookId, BookToken, FileFormat},
    device::BookSyncEntry,
};
use chrono::Utc;
use serde::Serialize;

use super::{KoboDevice, cursor};

// ── Kobo dummy constant (matches Komga / Calibre-Web convention)
// ──────────────────────────────────────────────────────────────────

/// Placeholder UUID used for `categories` and `genre` when no real value
/// exists. Both Komga and Calibre-Web use the same sentinel.
const DUMMY_ID: &str = "00000000-0000-0000-0000-000000000001";

// ── Response types
// ──────────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct KoboActivePeriod {
    from: String,
}

/// Per-book entitlement. `IsRemoved: true` signals deletion to the Kobo.
///
/// Field names and values validated against Komga `BookEntitlementDto.kt` and
/// Calibre-Web `kobo.py :: create_book_entitlement()`.
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct KoboBookEntitlement {
    /// Always `"Full"` — string, not integer.
    accessibility: &'static str,
    active_period: KoboActivePeriod,
    created: String,
    cross_revision_id: String,
    id: String,
    is_hidden_from_archive: bool,
    is_locked: bool,
    is_removed: bool,
    last_modified: String,
    origin_category: &'static str,
    revision_id: String,
    /// Always `"Active"` — string, not integer.
    status: &'static str,
}

/// One entry in the `DownloadUrls` array.
///
/// The Kobo protocol requires an *array* of download URL objects, not a single
/// `DownloadUrl` string. Validated against Komga `DownloadUrlDto.kt` and
/// Calibre-Web `kobo.py`.
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct KoboDownloadUrl {
    drm_type: &'static str,
    format: &'static str,
    size: i64,
    platform: &'static str,
    url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct KoboPrice {
    currency_code: &'static str,
    total_amount: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct KoboPublisher {
    imprint: String,
    name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct KoboBookMetadata {
    categories: Vec<&'static str>,
    contributor_roles: Vec<String>,
    contributors: Vec<String>,
    cover_image_id: String,
    cross_revision_id: String,
    current_display_price: KoboPrice,
    current_love_display_price: KoboPrice,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    download_urls: Vec<KoboDownloadUrl>,
    entitlement_id: String,
    external_ids: Vec<String>,
    genre: &'static str,
    is_eligible_for_kobo_love: bool,
    is_internet_archive: bool,
    is_pre_order: bool,
    is_social_enabled: bool,
    language: String,
    phonetic_pronunciations: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    publication_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    publisher: Option<KoboPublisher>,
    revision_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    series: Option<()>,
    title: String,
    work_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct KoboEntitlementContainer {
    book_entitlement: KoboBookEntitlement,
    book_metadata: KoboBookMetadata,
}

/// Top-level item in the library sync response array.
///
/// Serde's default (externally tagged) representation emits
/// `{"NewEntitlement": {...}}` or `{"ChangedEntitlement": {...}}`.
///
/// Removed books use `ChangedEntitlement` with `IsRemoved: true` — **not** a
/// `DeletedEntitlement` key (which does not exist in the Kobo protocol).
#[derive(Serialize)]
enum KoboSyncItem {
    NewEntitlement(KoboEntitlementContainer),
    ChangedEntitlement(KoboEntitlementContainer),
}

// ── Helpers
// ─────────────────────────────────────────────────────────────────

/// Returns the Kobo-facing ID for a book: the token string with `BK_` stripped.
fn book_uuid_from_token(token: &BookToken) -> String {
    let s = token.to_string();
    s.strip_prefix("BK_").unwrap_or(&s).to_string()
}

fn book_uuid_from_id(id: BookId) -> String {
    book_uuid_from_token(&BookToken::new(id))
}

fn build_entitlement(uuid: &str, is_removed: bool, created: &str, last_modified: &str) -> KoboBookEntitlement {
    KoboBookEntitlement {
        accessibility: "Full",
        active_period: KoboActivePeriod { from: created.to_string() },
        created: created.to_string(),
        cross_revision_id: uuid.to_string(),
        id: uuid.to_string(),
        is_hidden_from_archive: false,
        is_locked: false,
        is_removed,
        last_modified: last_modified.to_string(),
        origin_category: "Imported",
        revision_id: uuid.to_string(),
        status: "Active",
    }
}

fn build_new_entitlement(entry: &BookSyncEntry, sync_token: &str, base: &str) -> KoboSyncItem {
    let book = &entry.book;
    let file = &entry.file;
    let uuid = book_uuid_from_token(&book.token);

    let (format_str, kobo_format) = match file.format {
        FileFormat::Kepub => ("kepub", "KEPUB"),
        _ => ("epub", "EPUB3"),
    };

    let download_url = format!("{base}/kobo/{sync_token}/v1/download/{uuid}/{format_str}");
    let created = book.created_at.to_rfc3339();
    let last_modified = book.updated_at.to_rfc3339();

    let entitlement = build_entitlement(&uuid, false, &created, &last_modified);

    let metadata = KoboBookMetadata {
        categories: vec![DUMMY_ID],
        contributor_roles: Vec::new(),
        contributors: Vec::new(),
        cover_image_id: uuid.clone(),
        cross_revision_id: uuid.clone(),
        current_display_price: KoboPrice {
            currency_code: "USD",
            total_amount: 0,
        },
        current_love_display_price: KoboPrice {
            currency_code: "USD",
            total_amount: 0,
        },
        description: book.description.clone().filter(|s| !s.is_empty()),
        download_urls: vec![KoboDownloadUrl {
            drm_type: "None",
            format: kobo_format,
            size: file.file_size,
            platform: "Generic",
            url: download_url,
        }],
        entitlement_id: uuid.clone(),
        external_ids: Vec::new(),
        genre: DUMMY_ID,
        is_eligible_for_kobo_love: false,
        is_internet_archive: false,
        is_pre_order: false,
        is_social_enabled: true,
        language: book.language.clone().unwrap_or_else(|| "en".to_string()),
        phonetic_pronunciations: HashMap::new(),
        publication_date: book.published_date.map(|y| format!("{y}-01-01T00:00:00Z")),
        publisher: None,
        revision_id: uuid.clone(),
        series: None,
        title: book.title.clone(),
        work_id: uuid,
    };

    KoboSyncItem::NewEntitlement(KoboEntitlementContainer {
        book_entitlement: entitlement,
        book_metadata: metadata,
    })
}

fn build_removed_entitlement(book_id: BookId) -> KoboSyncItem {
    let uuid = book_uuid_from_id(book_id);
    let now = Utc::now().to_rfc3339();

    let entitlement = build_entitlement(&uuid, true, &now, &now);

    // Minimal metadata — the Kobo only needs the ID fields to identify which
    // book to remove. Title is required by the schema so we use an empty string.
    let metadata = KoboBookMetadata {
        categories: vec![DUMMY_ID],
        contributor_roles: Vec::new(),
        contributors: Vec::new(),
        cover_image_id: uuid.clone(),
        cross_revision_id: uuid.clone(),
        current_display_price: KoboPrice {
            currency_code: "USD",
            total_amount: 0,
        },
        current_love_display_price: KoboPrice {
            currency_code: "USD",
            total_amount: 0,
        },
        description: None,
        download_urls: Vec::new(),
        entitlement_id: uuid.clone(),
        external_ids: Vec::new(),
        genre: DUMMY_ID,
        is_eligible_for_kobo_love: false,
        is_internet_archive: false,
        is_pre_order: false,
        is_social_enabled: true,
        language: "en".to_string(),
        phonetic_pronunciations: HashMap::new(),
        publication_date: None,
        publisher: None,
        revision_id: uuid.clone(),
        series: None,
        title: String::new(),
        work_id: uuid,
    };

    KoboSyncItem::ChangedEntitlement(KoboEntitlementContainer {
        book_entitlement: entitlement,
        book_metadata: metadata,
    })
}

// ── Handler
// ─────────────────────────────────────────────────────────────────

pub async fn handle(kobo: KoboDevice, req_headers: HeaderMap, core_services: Arc<CoreServices>, base_url: String) -> Result<impl IntoResponse, StatusCode> {
    // 1. Decode sync cursor from request header (absent = full sync from start).
    let raw_cursor = req_headers.get("x-kobo-synctoken").and_then(|v| v.to_str().ok()).unwrap_or("");
    let (since, after_book_id) = cursor::decode(raw_cursor);

    // 2. Compute which books to add / remove.
    let diff = core_services
        .device_service
        .compute_sync_diff(kobo.device.id, kobo.device.owner_id, since, after_book_id, 100)
        .await
        .map_err(|e| {
            tracing::error!(error = ?e, "compute_sync_diff failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    tracing::debug!(
        device_id = kobo.device.id,
        new_books = diff.new_books.len(),
        upgraded_books = diff.upgraded_books.len(),
        refreshed_books = diff.refreshed_books.len(),
        removed_books = diff.removed_book_ids.len(),
        has_more = diff.has_more,
        "kobo library sync"
    );

    // 3. Persist DeviceBook records for this page.
    core_services.device_service.apply_sync(kobo.device.id, &diff).await.map_err(|e| {
        tracing::error!(error = ?e, "apply_sync failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // 4. Build response body.
    let base = base_url.trim_end_matches('/');
    let t = &kobo.sync_token;
    let mut items: Vec<KoboSyncItem> =
        Vec::with_capacity(diff.removed_book_ids.len() + diff.new_books.len() + diff.upgraded_books.len() + diff.refreshed_books.len());

    // Removed books → ChangedEntitlement with IsRemoved: true.
    for &book_id in &diff.removed_book_ids {
        items.push(build_removed_entitlement(book_id));
    }

    // New, upgraded, and refreshed books → NewEntitlement.
    for entry in diff.new_books.iter().chain(diff.upgraded_books.iter()).chain(diff.refreshed_books.iter()) {
        items.push(build_new_entitlement(entry, t, base));
    }

    // 5. Compute cursor for next request.
    //    - Mid-pagination: preserve `since`, advance keyset bookmark.
    //    - Final page: advance `since` to now so next sync is incremental.
    let last_book_id = [diff.new_books.last(), diff.upgraded_books.last(), diff.refreshed_books.last()]
        .into_iter()
        .flatten()
        .map(|e| e.book.id)
        .max();

    let next_cursor = if diff.has_more {
        cursor::encode(since, last_book_id)
    } else {
        cursor::encode(Some(Utc::now()), None)
    };

    // 6. Build response headers. x-kobo-synctoken: always present (cursor for next
    //    call). x-kobo-sync:      "continue" only when more pages remain; absent
    //    when done.
    let next_cursor_hv = HeaderValue::try_from(next_cursor).expect("cursor contains only ASCII digits and colons");

    let mut resp_headers = HeaderMap::new();
    resp_headers.insert(HeaderName::from_static("x-kobo-synctoken"), next_cursor_hv);
    if diff.has_more {
        resp_headers.insert(HeaderName::from_static("x-kobo-sync"), HeaderValue::from_static("continue"));
    }

    Ok((resp_headers, Json(items)))
}
