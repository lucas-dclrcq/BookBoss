use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

/// Shelf detail page — renders the books on a single shelf.
#[component]
pub(crate) fn ShelfPage(token: String) -> Element {
    rsx! {
        div { class: "flex-1 flex items-center justify-center text-gray-400 text-sm",
            "Shelf {token} — coming soon"
        }
    }
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

/// Lightweight shelf descriptor returned by list and create operations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ShelfSummary {
    pub token: String,
    pub name: String,
    /// `"Private"` or `"Public"`
    pub visibility: String,
    /// `true` if the current user owns this shelf.
    pub is_own: bool,
}

/// Full book entry for a shelf, including hydrated author and series data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ShelfBookSummary {
    pub token: String,
    pub title: String,
    pub cover_path: Option<String>,
    pub author_names: Vec<String>,
    pub series_name: Option<String>,
    pub series_number: Option<String>,
    pub added_at: String,
}

// ---------------------------------------------------------------------------
// Server-only imports
// ---------------------------------------------------------------------------

#[cfg(feature = "server")]
use {
    crate::server::{AuthSession, AuthUser, BackendSessionPool},
    axum::http::Method,
    axum_session_auth::{Auth, Rights},
    bb_core::{
        CoreServices,
        book::{AuthorToken, BookToken, SeriesToken},
        shelf::{ShelfToken, ShelfVisibility},
        user::UserId,
    },
    std::sync::Arc,
};

// ---------------------------------------------------------------------------
// Helpers (server only)
// ---------------------------------------------------------------------------

#[cfg(feature = "server")]
fn parse_visibility(s: &str) -> Result<ShelfVisibility, ServerFnError> {
    match s {
        "Public" => Ok(ShelfVisibility::Public),
        "Private" => Ok(ShelfVisibility::Private),
        other => Err(ServerFnError::new(format!("invalid visibility: {other}"))),
    }
}

#[cfg(feature = "server")]
fn visibility_str(v: &ShelfVisibility) -> &'static str {
    match v {
        ShelfVisibility::Public => "Public",
        ShelfVisibility::Private => "Private",
    }
}

// ---------------------------------------------------------------------------
// Server functions
// ---------------------------------------------------------------------------

/// Returns all shelves belonging to the authenticated user.
#[get(
    "/api/v1/shelves",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn list_my_shelves() -> Result<Vec<ShelfSummary>, ServerFnError> {
    let user = auth_session
        .current_user
        .as_ref()
        .filter(|u| !u.username.is_empty())
        .ok_or_else(|| ServerFnError::new("Not authenticated"))?;
    let user_id = user.id();

    let shelves = core_services
        .shelf_service
        .list_shelves_for_user(user_id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(shelves
        .iter()
        .map(|s| ShelfSummary {
            token: s.token.to_string(),
            name: s.name.clone(),
            visibility: visibility_str(&s.visibility).to_string(),
            is_own: true,
        })
        .collect())
}

/// Creates a new manual shelf and returns its token.
#[post(
    "/api/v1/shelves",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn create_shelf(name: String, visibility: String) -> Result<String, ServerFnError> {
    let user = auth_session
        .current_user
        .as_ref()
        .filter(|u| !u.username.is_empty())
        .ok_or_else(|| ServerFnError::new("Not authenticated"))?;
    let user_id = user.id();

    let vis = parse_visibility(&visibility)?;

    let token = core_services
        .shelf_service
        .create_manual_shelf(user_id, name, vis)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(token.to_string())
}

/// Renames a shelf. Only the owner may rename.
#[put(
    "/api/v1/shelves/rename",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn rename_shelf(token: String, new_name: String) -> Result<(), ServerFnError> {
    let user = auth_session
        .current_user
        .as_ref()
        .filter(|u| !u.username.is_empty())
        .ok_or_else(|| ServerFnError::new("Not authenticated"))?;
    let user_id = user.id();

    let shelf_token: ShelfToken = token.parse().map_err(|_| ServerFnError::new("Invalid shelf token"))?;

    core_services
        .shelf_service
        .rename_shelf(&shelf_token, new_name, user_id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Deletes a shelf. Only the owner may delete.
#[delete(
    "/api/v1/shelves",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn delete_shelf(token: String) -> Result<(), ServerFnError> {
    let user = auth_session
        .current_user
        .as_ref()
        .filter(|u| !u.username.is_empty())
        .ok_or_else(|| ServerFnError::new("Not authenticated"))?;
    let user_id = user.id();

    let shelf_token: ShelfToken = token.parse().map_err(|_| ServerFnError::new("Invalid shelf token"))?;

    core_services
        .shelf_service
        .delete_shelf(&shelf_token, user_id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Adds a book to a shelf. Only the owner may add books.
#[post(
    "/api/v1/shelves/books",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn add_book_to_shelf(shelf_token: String, book_token: String) -> Result<(), ServerFnError> {
    let user = auth_session
        .current_user
        .as_ref()
        .filter(|u| !u.username.is_empty())
        .ok_or_else(|| ServerFnError::new("Not authenticated"))?;
    let user_id = user.id();

    let shelf: ShelfToken = shelf_token.parse().map_err(|_| ServerFnError::new("Invalid shelf token"))?;
    let book: BookToken = book_token.parse().map_err(|_| ServerFnError::new("Invalid book token"))?;

    core_services
        .shelf_service
        .add_book_to_shelf(&shelf, &book, user_id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Removes a book from a shelf. Only the owner may remove books.
#[delete(
    "/api/v1/shelves/books",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn remove_book_from_shelf(shelf_token: String, book_token: String) -> Result<(), ServerFnError> {
    let user = auth_session
        .current_user
        .as_ref()
        .filter(|u| !u.username.is_empty())
        .ok_or_else(|| ServerFnError::new("Not authenticated"))?;
    let user_id = user.id();

    let shelf: ShelfToken = shelf_token.parse().map_err(|_| ServerFnError::new("Invalid shelf token"))?;
    let book: BookToken = book_token.parse().map_err(|_| ServerFnError::new("Invalid book token"))?;

    core_services
        .shelf_service
        .remove_book_from_shelf(&shelf, &book, user_id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Updates the visibility of a shelf. Only the owner may change visibility.
#[put(
    "/api/v1/shelves/visibility",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn set_shelf_visibility(token: String, visibility: String) -> Result<(), ServerFnError> {
    let user = auth_session
        .current_user
        .as_ref()
        .filter(|u| !u.username.is_empty())
        .ok_or_else(|| ServerFnError::new("Not authenticated"))?;
    let user_id = user.id();

    let shelf_token: ShelfToken = token.parse().map_err(|_| ServerFnError::new("Invalid shelf token"))?;
    let vis = parse_visibility(&visibility)?;

    core_services
        .shelf_service
        .set_visibility(&shelf_token, vis, user_id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Returns the current user's own shelves plus all public shelves from other
/// users.
///
/// Own shelves come first (in creation order), then others' public shelves
/// sorted by name. Each entry carries `is_own` so the UI can split them into
/// two groups.
#[get(
    "/api/v1/shelves/all",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn list_all_accessible_shelves() -> Result<Vec<ShelfSummary>, ServerFnError> {
    let user = auth_session
        .current_user
        .as_ref()
        .filter(|u| !u.username.is_empty())
        .ok_or_else(|| ServerFnError::new("Not authenticated"))?;
    let user_id = user.id();

    let shelf_service = &core_services.shelf_service;

    let mut own = shelf_service
        .list_shelves_for_user(user_id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .into_iter()
        .map(|s| ShelfSummary {
            token: s.token.to_string(),
            name: s.name.clone(),
            visibility: visibility_str(&s.visibility).to_string(),
            is_own: true,
        })
        .collect::<Vec<_>>();

    let others = shelf_service
        .list_public_shelves(user_id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .into_iter()
        .map(|s| ShelfSummary {
            token: s.token.to_string(),
            name: s.name.clone(),
            visibility: visibility_str(&s.visibility).to_string(),
            is_own: false,
        });

    own.extend(others);
    Ok(own)
}

/// Returns a paginated list of books on a shelf, with author and series data
/// hydrated.
///
/// `cursor` is the last `book_id` seen (exclusive lower bound for the next
/// page). `page_size` defaults to the server's configured page size when
/// `None`.
#[get(
    "/api/v1/shelves/books/list",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn books_for_shelf(token: String, cursor: Option<u64>, page_size: Option<u64>) -> Result<Vec<ShelfBookSummary>, ServerFnError> {
    use std::collections::{HashMap, HashSet};

    let user = auth_session
        .current_user
        .as_ref()
        .filter(|u| !u.username.is_empty())
        .ok_or_else(|| ServerFnError::new("Not authenticated"))?;
    let user_id = user.id();

    let shelf_token: ShelfToken = token.parse().map_err(|_| ServerFnError::new("Invalid shelf token"))?;

    let shelf_service = &core_services.shelf_service;
    let book_service = &core_services.book_service;

    // 1. Load BookShelf entries (cursor-paginated, ordered by book_id).
    let shelf_entries = shelf_service
        .books_for_shelf(&shelf_token, user_id, cursor, page_size)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    if shelf_entries.is_empty() {
        return Ok(vec![]);
    }

    // 2. Resolve each book_id → Book, preserving shelf order.
    let mut books = Vec::with_capacity(shelf_entries.len());
    let mut all_author_ids: HashSet<u64> = HashSet::new();
    let mut added_ats: Vec<String> = Vec::with_capacity(shelf_entries.len());

    for entry in &shelf_entries {
        let book = book_service
            .find_book_by_token(&BookToken::new(entry.book_id))
            .await
            .map_err(|e| ServerFnError::new(e.to_string()))?
            .ok_or_else(|| ServerFnError::new("Book not found"))?;

        let authors = book_service.authors_for_book(book.id).await.map_err(|e| ServerFnError::new(e.to_string()))?;

        for ba in &authors {
            all_author_ids.insert(ba.author_id);
        }

        added_ats.push(entry.added_at.to_rfc3339());
        books.push((book, authors));
    }

    // 3. Batch-load unique authors.
    let mut author_map: HashMap<u64, String> = HashMap::new();
    for author_id in all_author_ids {
        if let Some(author) = book_service
            .find_author_by_token(&AuthorToken::new(author_id))
            .await
            .map_err(|e| ServerFnError::new(e.to_string()))?
        {
            author_map.insert(author_id, author.name);
        }
    }

    // 4. Batch-load unique series.
    let unique_series: HashSet<u64> = books.iter().filter_map(|(b, _)| b.series_id).collect();
    let mut series_map: HashMap<u64, String> = HashMap::new();
    for series_id in unique_series {
        if let Some(series) = book_service
            .find_series_by_token(&SeriesToken::new(series_id))
            .await
            .map_err(|e| ServerFnError::new(e.to_string()))?
        {
            series_map.insert(series_id, series.name);
        }
    }

    // 5. Assemble summaries.
    let summaries = books
        .iter()
        .zip(added_ats.iter())
        .map(|((book, author_links), added_at)| {
            let mut sorted = author_links.clone();
            sorted.sort_by_key(|ba| ba.sort_order);
            let author_names = sorted.iter().filter_map(|ba| author_map.get(&ba.author_id).cloned()).collect();

            ShelfBookSummary {
                token: book.token.to_string(),
                title: book.title.clone(),
                cover_path: book.cover_path.clone(),
                author_names,
                series_name: book.series_id.and_then(|sid| series_map.get(&sid).cloned()),
                series_number: book.series_number.as_ref().map(|n| n.to_string()),
                added_at: added_at.clone(),
            }
        })
        .collect();

    Ok(summaries)
}
