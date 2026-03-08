use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

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
        book::BookToken,
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
