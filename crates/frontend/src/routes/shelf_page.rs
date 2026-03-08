use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{Route, routes::books_page::BookSummary};

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

/// Full shelf metadata returned by the detail endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ShelfDetail {
    pub token: String,
    pub name: String,
    /// `"Private"` or `"Public"`
    pub visibility: String,
    /// `true` if the current user owns this shelf.
    pub is_own: bool,
}

// ---------------------------------------------------------------------------
// Server-only imports
// ---------------------------------------------------------------------------

#[cfg(feature = "server")]
use {
    crate::server::AuthSession,
    bb_core::{
        CoreServices,
        book::{AuthorToken, BookToken, SeriesToken},
        shelf::{ShelfToken, ShelfVisibility},
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

/// Returns metadata for a single shelf.
///
/// Owners can access private shelves; non-owners can only access public
/// shelves.
#[post(
    "/api/v1/shelves/detail",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn get_shelf(token: String) -> Result<ShelfDetail, ServerFnError> {
    let user = auth_session
        .current_user
        .as_ref()
        .filter(|u| !u.username.is_empty())
        .ok_or_else(|| ServerFnError::new("Not authenticated"))?;
    let user_id = user.id();

    let shelf_token: ShelfToken = token.parse().map_err(|_| ServerFnError::new("Invalid shelf token"))?;

    let shelf = core_services
        .shelf_service
        .get_shelf(&shelf_token, user_id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(ShelfDetail {
        token: shelf.token.to_string(),
        name: shelf.name.clone(),
        visibility: visibility_str(&shelf.visibility).to_string(),
        is_own: shelf.owner_id == user_id,
    })
}

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

/// Updates the name and visibility of a shelf in one call. Only the owner may
/// update.
#[put(
    "/api/v1/shelves/update",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn update_shelf(token: String, name: String, visibility: String) -> Result<(), ServerFnError> {
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
        .update_shelf(&shelf_token, name, vis, user_id)
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
#[post(
    "/api/v1/shelves/books/list",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn books_for_shelf(token: String, cursor: Option<u64>, page_size: Option<u64>) -> Result<Vec<BookSummary>, ServerFnError> {
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
        .map(|(book, author_links)| {
            let mut sorted = author_links.clone();
            sorted.sort_by_key(|ba| ba.sort_order);
            let author_names = sorted.iter().filter_map(|ba| author_map.get(&ba.author_id).cloned()).collect();

            BookSummary {
                token: book.token.to_string(),
                title: book.title.clone(),
                cover_path: book.cover_path.clone(),
                author_names,
                series_name: book.series_id.and_then(|sid| series_map.get(&sid).cloned()),
                series_number: book.series_number.as_ref().map(|n| n.to_string()),
            }
        })
        .collect();

    Ok(summaries)
}

// ---------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------

/// Book card for the shelf detail page. Renders identically to `BookCard` but
/// adds an "×" remove button (owner only) overlaid on the cover.
#[component]
fn ShelfBookCard(book: BookSummary, shelf_token: String, is_own: bool, on_removed: EventHandler<()>) -> Element {
    let navigator = use_navigator();
    let book_token_nav = book.token.clone();
    let book_token_remove = book.token.clone();
    let shelf_tok = shelf_token.clone();
    let author_str = book.author_names.join(", ");
    let series_line = match (&book.series_name, &book.series_number) {
        (Some(name), Some(num)) => Some(format!("{name} #{num}")),
        (Some(name), None) => Some(name.clone()),
        _ => None,
    };

    rsx! {
        div { class: "flex flex-col",
            // Cover with optional remove overlay
            div {
                class: "relative cursor-pointer",
                onclick: move |_| { navigator.push(Route::BookDetailPage { token: book_token_nav.clone() }); },
                img {
                    src: "/api/v1/covers/{book.token}",
                    alt: "{book.title}",
                    class: "w-full object-cover rounded shadow-sm",
                    style: "aspect-ratio: 2/3",
                }
                if is_own {
                    button {
                        class: "absolute top-1 right-1 w-5 h-5 flex items-center justify-center rounded-full bg-black/50 text-white text-xs hover:bg-red-600/80 leading-none",
                        title: "Remove from shelf",
                        // Stop click propagating to the cover (which navigates to book detail)
                        onclick: move |e| {
                            e.stop_propagation();
                            let s = shelf_tok.clone();
                            let b = book_token_remove.clone();
                            spawn(async move {
                                let _ = remove_book_from_shelf(s, b).await;
                                on_removed.call(());
                            });
                        },
                        "×"
                    }
                }
            }
            div { class: "mt-1 px-0.5",
                p { class: "text-xs font-semibold text-gray-900 leading-tight line-clamp-2",
                    "{book.title}"
                }
                p { class: "text-xs text-gray-500 leading-tight truncate mt-0.5",
                    "{author_str}"
                }
                if let Some(series) = series_line {
                    p { class: "text-xs text-gray-400 leading-tight truncate mt-0.5",
                        "{series}"
                    }
                }
            }
        }
    }
}

/// Shelf detail page — shows shelf metadata and its books.
#[component]
pub(crate) fn ShelfPage(token: String) -> Element {
    let nav = use_navigator();

    // --- State ---
    let mut editing_name = use_signal(|| false);
    let mut draft_name = use_signal(String::new);
    let mut show_delete_confirm = use_signal(|| false);
    let mut deleting = use_signal(|| false);

    // --- Data loading ---
    let token_for_shelf = token.clone();
    let mut shelf_resource = use_server_future(move || get_shelf(token_for_shelf.clone()))?;

    let token_for_books = token.clone();
    let mut books_resource = use_server_future(move || books_for_shelf(token_for_books.clone(), None, None))?;

    rsx! {
        div { class: "flex-1 overflow-auto p-6",
            match shelf_resource() {
                None => rsx! {
                    div { class: "flex items-center justify-center h-full text-gray-400 text-sm",
                        "Loading…"
                    }
                },
                Some(Err(e)) => rsx! {
                    div { class: "text-red-600 text-sm", "Failed to load shelf: {e}" }
                },
                Some(Ok(shelf)) => rsx! {
                    // Delete confirmation modal
                    if show_delete_confirm() {
                        div { class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
                            div { class: "bg-white rounded-lg shadow-xl p-6 w-full max-w-sm mx-4",
                                h2 { class: "text-lg font-semibold text-gray-900 mb-2", "Delete Shelf?" }
                                p { class: "text-sm text-gray-600 mb-6",
                                    "This will permanently delete \"{shelf.name}\". Books will not be affected."
                                }
                                div { class: "flex gap-3 justify-end",
                                    button {
                                        class: "px-4 py-2 text-sm font-medium rounded border border-gray-300 text-gray-700 hover:bg-gray-50",
                                        autofocus: true,
                                        onclick: move |_| show_delete_confirm.set(false),
                                        "Cancel"
                                    }
                                    button {
                                        class: "px-4 py-2 text-sm font-medium rounded bg-red-600 text-white hover:bg-red-700 disabled:opacity-50",
                                        disabled: deleting(),
                                        onclick: {
                                            let tok = shelf.token.clone();
                                            move |_| {
                                                let tok = tok.clone();
                                                deleting.set(true);
                                                spawn(async move {
                                                    match delete_shelf(tok).await {
                                                        Ok(()) => {
                                                            let _ = nav.push(Route::BooksPage {});
                                                        }
                                                        Err(_) => {
                                                            deleting.set(false);
                                                            show_delete_confirm.set(false);
                                                        }
                                                    }
                                                });
                                            }
                                        },
                                        if deleting() { "Deleting…" } else { "Yes, Delete" }
                                    }
                                }
                            }
                        }
                    }

                    // Back link
                    Link {
                        to: Route::BooksPage {},
                        class: "inline-flex items-center gap-1 text-sm text-indigo-600 hover:text-indigo-800 mb-6",
                        "← Library"
                    }

                    // Shelf header
                    div { class: "flex items-center gap-3 mb-6 flex-wrap",
                        // Inline-editable title (owner only)
                        if shelf.is_own && editing_name() {
                            input {
                                class: "text-2xl font-bold text-gray-900 border-b-2 border-indigo-500 outline-none bg-transparent",
                                value: draft_name(),
                                autofocus: true,
                                oninput: move |e| draft_name.set(e.value()),
                                onblur: {
                                    let tok = shelf.token.clone();
                                    move |_| {
                                        let tok = tok.clone();
                                        let name = draft_name();
                                        editing_name.set(false);
                                        spawn(async move {
                                            let _ = rename_shelf(tok, name).await;
                                            shelf_resource.restart();
                                        });
                                    }
                                },
                                onkeydown: {
                                    let tok = shelf.token.clone();
                                    move |e: KeyboardEvent| {
                                        match e.key() {
                                            Key::Enter => {
                                                let tok = tok.clone();
                                                let name = draft_name();
                                                editing_name.set(false);
                                                spawn(async move {
                                                    let _ = rename_shelf(tok, name).await;
                                                    shelf_resource.restart();
                                                });
                                            }
                                            Key::Escape => {
                                                editing_name.set(false);
                                            }
                                            _ => {}
                                        }
                                    }
                                },
                            }
                        } else {
                            h1 { class: "text-2xl font-bold text-gray-900",
                                "{shelf.name}"
                            }
                            if shelf.is_own {
                                button {
                                    class: "text-gray-400 hover:text-indigo-600 text-sm",
                                    title: "Rename shelf",
                                    onclick: {
                                        let name = shelf.name.clone();
                                        move |_| {
                                            draft_name.set(name.clone());
                                            editing_name.set(true);
                                        }
                                    },
                                    "✎"
                                }
                            }
                        }

                        // Visibility badge / toggle
                        if shelf.is_own {
                            button {
                                class: "px-2 py-0.5 rounded text-xs font-medium border hover:opacity-80 transition-opacity",
                                class: if shelf.visibility == "Public" {
                                    "border-green-300 bg-green-50 text-green-700"
                                } else {
                                    "border-gray-300 bg-gray-50 text-gray-600"
                                },
                                title: if shelf.visibility == "Public" { "Click to make Private" } else { "Click to make Public" },
                                onclick: {
                                    let tok = shelf.token.clone();
                                    let new_vis = if shelf.visibility == "Public" { "Private" } else { "Public" };
                                    move |_| {
                                        let tok = tok.clone();
                                        let vis = new_vis.to_string();
                                        spawn(async move {
                                            let _ = set_shelf_visibility(tok, vis).await;
                                            shelf_resource.restart();
                                        });
                                    }
                                },
                                "{shelf.visibility}"
                            }
                        } else {
                            span {
                                class: "px-2 py-0.5 rounded text-xs font-medium border border-green-300 bg-green-50 text-green-700",
                                "Public"
                            }
                        }

                        // Delete button (owner only) — pushed to the right
                        if shelf.is_own {
                            div { class: "ml-auto",
                                button {
                                    class: "text-sm text-red-500 hover:text-red-700",
                                    onclick: move |_| show_delete_confirm.set(true),
                                    "Delete Shelf"
                                }
                            }
                        }
                    }

                    // Book list
                    match books_resource() {
                        None => rsx! {
                            div { class: "text-gray-400 text-sm", "Loading books…" }
                        },
                        Some(Err(e)) => rsx! {
                            div { class: "text-red-600 text-sm", "Failed to load books: {e}" }
                        },
                        Some(Ok(books)) => rsx! {
                            if books.is_empty() {
                                div { class: "flex flex-col items-center justify-center py-20 text-center",
                                    p { class: "text-gray-400 text-sm", "No books on this shelf yet." }
                                    if shelf.is_own {
                                        p { class: "text-gray-300 text-xs mt-1",
                                            "Open any book and use \"Add to Shelf\" to populate this shelf."
                                        }
                                    }
                                }
                            } else {
                                div { class: "grid gap-x-8 gap-y-4",
                                    style: "grid-template-columns: repeat(auto-fill, minmax(120px, 1fr))",
                                    for book in &books {
                                        ShelfBookCard {
                                            book: book.clone(),
                                            shelf_token: shelf.token.clone(),
                                            is_own: shelf.is_own,
                                            on_removed: move |_| books_resource.restart(),
                                        }
                                    }
                                }
                            }
                        },
                    }
                },
            }
        }
    }
}
