use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    Route,
    components::{BookFilter, BookGrid, BookGridContext, FilterBuilder, FilterEntityOptions, ShelfBar, default_book_filter},
    routes::books_page::BookSummary,
};

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
    /// `true` if this is a smart (filter-based) shelf.
    pub is_smart: bool,
    /// Serialized `BookFilter` JSON — present only for smart shelves owned by
    /// the current user.
    pub filter_json: Option<String>,
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
        filter::BookFilter as CoreBookFilter,
        shelf::{ShelfToken, ShelfType, ShelfVisibility},
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
        .map(|s| {
            let is_smart = s.shelf_type == ShelfType::Smart;
            let filter_json = if is_smart {
                s.filter_criteria.as_ref().and_then(|f| serde_json::to_string(f).ok())
            } else {
                None
            };
            ShelfSummary {
                token: s.token.to_string(),
                name: s.name.clone(),
                visibility: visibility_str(&s.visibility).to_string(),
                is_own: true,
                is_smart,
                filter_json,
            }
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

/// Creates a new smart shelf from a serialized `BookFilter` JSON.
///
/// `filter_json` must be a valid JSON-encoded `BookFilter`.  Returns the new
/// shelf token.
#[post(
    "/api/v1/shelves/smart",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn create_smart_shelf(name: String, visibility: String, filter_json: String) -> Result<String, ServerFnError> {
    let user = auth_session
        .current_user
        .as_ref()
        .filter(|u| !u.username.is_empty())
        .ok_or_else(|| ServerFnError::new("Not authenticated"))?;
    let user_id = user.id();

    let vis = parse_visibility(&visibility)?;
    let filter: CoreBookFilter = serde_json::from_str(&filter_json).map_err(|e| ServerFnError::new(format!("Invalid filter: {e}")))?;

    let token = core_services
        .shelf_service
        .create_smart_shelf(user_id, name, vis, filter)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(token.to_string())
}

/// Updates the filter on an existing smart shelf. Only the owner may update.
#[put(
    "/api/v1/shelves/smart/filter",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn update_smart_shelf_filter(token: String, filter_json: String) -> Result<(), ServerFnError> {
    let user = auth_session
        .current_user
        .as_ref()
        .filter(|u| !u.username.is_empty())
        .ok_or_else(|| ServerFnError::new("Not authenticated"))?;
    let user_id = user.id();

    let shelf_token: ShelfToken = token.parse().map_err(|_| ServerFnError::new("Invalid shelf token"))?;
    let filter: CoreBookFilter = serde_json::from_str(&filter_json).map_err(|e| ServerFnError::new(format!("Invalid filter: {e}")))?;

    core_services
        .shelf_service
        .update_shelf_filter(&shelf_token, filter, user_id)
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

/// Returns all entity options needed by the filter builder (authors, series,
/// genres, tags, publishers), each as `(id, name)` pairs for use as `EntityRef`
/// values.
#[get(
    "/api/v1/shelves/filter-entities",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn get_filter_entity_options() -> Result<FilterEntityOptions, ServerFnError> {
    let _user = auth_session
        .current_user
        .as_ref()
        .filter(|u| !u.username.is_empty())
        .ok_or_else(|| ServerFnError::new("Not authenticated"))?;

    let book_service = &core_services.book_service;

    let authors = book_service
        .list_all_authors()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .into_iter()
        .map(|a| (a.id as i64, a.name))
        .collect();

    let series = book_service
        .list_all_series()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .into_iter()
        .map(|s| (s.id as i64, s.name))
        .collect();

    let genres = book_service
        .list_all_genres()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .into_iter()
        .map(|g| (g.id as i64, g.name))
        .collect();

    let tags = book_service
        .list_all_tags()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .into_iter()
        .map(|t| (t.id as i64, t.name))
        .collect();

    let publishers = book_service
        .list_all_publishers()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .into_iter()
        .map(|p| (p.id as i64, p.name))
        .collect();

    Ok(FilterEntityOptions {
        authors,
        series,
        genres,
        tags,
        publishers,
    })
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
        .map(|s| {
            let is_smart = s.shelf_type == ShelfType::Smart;
            let filter_json = if is_smart {
                s.filter_criteria.as_ref().and_then(|f| serde_json::to_string(f).ok())
            } else {
                None
            };
            ShelfSummary {
                token: s.token.to_string(),
                name: s.name.clone(),
                visibility: visibility_str(&s.visibility).to_string(),
                is_own: true,
                is_smart,
                filter_json,
            }
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
            is_smart: s.shelf_type == ShelfType::Smart,
            filter_json: None, // don't expose others' filter JSON
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
                series_number: book.series_number.as_ref().map(std::string::ToString::to_string),
                reading_state: None,
            }
        })
        .collect();

    Ok(summaries)
}

// ---------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------

/// Shelf detail page — shows the `ShelfBar` and book grid, matching `BooksPage`
/// layout.
#[component]
pub(crate) fn ShelfPage(token: String) -> Element {
    let nav = use_navigator();

    use_context_provider(|| Signal::new(None::<String>)); // DraggedBookToken

    // Edit shelf modal state
    let mut show_edit = use_signal(|| false);
    let mut edit_name = use_signal(String::new);
    let mut edit_private = use_signal(|| true);
    let mut edit_filter = use_signal(default_book_filter);
    let mut saving = use_signal(|| false);
    let mut edit_error: Signal<Option<String>> = use_signal(|| None);

    // Entity options for the smart shelf filter editor
    let entity_options_resource = use_resource(get_filter_entity_options);

    // Delete shelf modal state
    let mut show_delete = use_signal(|| false);
    let mut deleting = use_signal(|| false);

    // Data loading
    let mut shelves_resource = use_server_future(list_all_accessible_shelves)?;

    // Sync `token` prop into a signal so `use_server_future` restarts reactively
    // when navigating between shelves (Dioxus re-renders in-place; plain captured
    // values won't trigger the future to re-run).
    let mut token_sig = use_signal(|| token.clone());
    if *token_sig.peek() != token {
        token_sig.set(token.clone());
    }
    let mut books_resource = use_server_future(move || {
        let tok = token_sig();
        books_for_shelf(tok, None, None)
    })?;

    // Derive current shelf info from the shelves list (avoids a separate get_shelf
    // call).
    let shelves: Vec<ShelfSummary> = shelves_resource().and_then(std::result::Result::ok).unwrap_or_default();
    let current_shelf = shelves.iter().find(|s| s.token == token).cloned();
    let is_own = current_shelf.as_ref().is_some_and(|s| s.is_own);
    let current_name = current_shelf.as_ref().map(|s| s.name.clone()).unwrap_or_default();
    let current_vis = current_shelf.as_ref().map(|s| s.visibility.clone()).unwrap_or_default();
    let current_is_smart = current_shelf.as_ref().is_some_and(|s| s.is_smart);
    let current_filter_json = current_shelf.as_ref().and_then(|s| s.filter_json.clone());

    let entity_options: FilterEntityOptions = entity_options_resource().and_then(std::result::Result::ok).unwrap_or_default();

    let context = if is_own {
        BookGridContext::OwnShelf { shelf_token: token.clone() }
    } else {
        BookGridContext::ReadOnly
    };

    rsx! {
        div { class: "flex-1 flex flex-col overflow-hidden",
            ShelfBar {
                shelves: shelves.clone(),
                current_shelf_token: Some(token.clone()),
                on_edit_shelf: {
                    let name_for_edit = current_name.clone();
                    let vis_for_edit = current_vis.clone();
                    let filter_json_for_edit = current_filter_json.clone();
                    move |()| {
                        edit_name.set(name_for_edit.clone());
                        edit_private.set(vis_for_edit == "Private");
                        let parsed = filter_json_for_edit
                            .as_deref()
                            .and_then(|j| serde_json::from_str::<BookFilter>(j).ok())
                            .unwrap_or_else(default_book_filter);
                        edit_filter.set(parsed);
                        edit_error.set(None);
                        show_edit.set(true);
                    }
                },
                on_delete_shelf: move |()| show_delete.set(true),
            }

            match books_resource() {
                None => rsx! {
                    div { class: "flex-1 flex items-center justify-center text-gray-400 text-sm",
                        "Loading…"
                    }
                },
                Some(Err(e)) => rsx! {
                    div { class: "flex-1 flex items-center justify-center text-red-600 text-sm",
                        "Failed to load books: {e}"
                    }
                },
                Some(Ok(books)) => {
                    if books.is_empty() {
                        rsx! {
                            div { class: "flex-1 flex flex-col items-center justify-center py-20 text-center",
                                p { class: "text-gray-400 text-sm", "No books on this shelf yet." }
                                if is_own {
                                    p { class: "text-gray-300 text-xs mt-1",
                                        "Drag a book here or open any book and use \"Add to Shelf\"."
                                    }
                                }
                            }
                        }
                    } else {
                        rsx! {
                            BookGrid {
                                books,
                                context: context.clone(),
                                on_action: move |()| books_resource.restart(),
                            }
                        }
                    }
                }
            }
        }

        // Edit shelf modal
        if show_edit() {
            div { class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
                div {
                    class: if current_is_smart {
                        "bg-white rounded-lg shadow-xl p-6 w-full max-w-2xl mx-4 max-h-[85vh] overflow-y-auto"
                    } else {
                        "bg-white rounded-lg shadow-xl p-6 w-full max-w-sm mx-4"
                    },
                    h2 { class: "text-lg font-semibold text-gray-900 mb-4", "Edit Shelf" }
                    form {
                        onsubmit: {
                            let tok = token.clone();
                            move |e: FormEvent| {
                                e.prevent_default();
                                let name = edit_name().trim().to_string();
                                if name.is_empty() {
                                    edit_error.set(Some("Shelf name is required.".into()));
                                    return;
                                }
                                let vis = if edit_private() { "Private" } else { "Public" }.to_string();
                                let tok = tok.clone();
                                saving.set(true);
                                edit_error.set(None);
                                if current_is_smart {
                                    let filter_json = match serde_json::to_string(&edit_filter()) {
                                        Ok(j) => j,
                                        Err(err) => {
                                            saving.set(false);
                                            edit_error.set(Some(format!("Filter error: {err}")));
                                            return;
                                        }
                                    };
                                    spawn(async move {
                                        let r1 = update_shelf(tok.clone(), name, vis).await;
                                        let r2 = update_smart_shelf_filter(tok, filter_json).await;
                                        match (r1, r2) {
                                            (Ok(()), Ok(())) => {
                                                show_edit.set(false);
                                                saving.set(false);
                                                shelves_resource.restart();
                                            }
                                            (Err(e), _) | (_, Err(e)) => {
                                                saving.set(false);
                                                edit_error.set(Some(e.to_string()));
                                            }
                                        }
                                    });
                                } else {
                                    spawn(async move {
                                        match update_shelf(tok, name, vis).await {
                                            Ok(()) => {
                                                show_edit.set(false);
                                                saving.set(false);
                                                shelves_resource.restart();
                                            }
                                            Err(e) => {
                                                saving.set(false);
                                                edit_error.set(Some(e.to_string()));
                                            }
                                        }
                                    });
                                }
                            }
                        },

                        div { class: "mb-4",
                            label { class: "block text-sm font-medium text-gray-700 mb-1",
                                r#for: "edit-shelf-name",
                                "Shelf name"
                            }
                            input {
                                id: "edit-shelf-name",
                                class: "w-full px-3 py-2 border rounded text-sm outline-none focus:ring-1",
                                class: if edit_error().is_some() {
                                    "border-red-400 focus:border-red-500 focus:ring-red-500"
                                } else {
                                    "border-gray-300 focus:border-indigo-500 focus:ring-indigo-500"
                                },
                                r#type: "text",
                                autofocus: true,
                                value: edit_name(),
                                oninput: move |e| {
                                    edit_name.set(e.value());
                                    edit_error.set(None);
                                },
                                onkeydown: move |e: KeyboardEvent| {
                                    if e.key() == Key::Escape {
                                        show_edit.set(false);
                                    }
                                },
                            }
                            if let Some(msg) = edit_error() {
                                p { class: "mt-1 text-xs text-red-600", "{msg}" }
                            }
                        }

                        div { class: "mb-4 flex items-center gap-2",
                            input {
                                id: "edit-shelf-private",
                                class: "h-4 w-4 rounded border-gray-300 text-indigo-600 focus:ring-indigo-500",
                                r#type: "checkbox",
                                checked: edit_private(),
                                onchange: move |e| edit_private.set(e.checked()),
                            }
                            label { class: "text-sm text-gray-700 cursor-pointer", r#for: "edit-shelf-private",
                                "Private"
                            }
                        }

                        if current_is_smart {
                            div { class: "mb-6",
                                p { class: "text-sm font-medium text-gray-700 mb-2", "Filter rules" }
                                FilterBuilder { filter: edit_filter, entity_options: entity_options.clone() }
                            }
                        }

                        div { class: "flex gap-3 justify-end",
                            button {
                                r#type: "button",
                                class: "px-4 py-2 text-sm font-medium rounded border border-gray-300 text-gray-700 hover:bg-gray-50",
                                onclick: move |_| show_edit.set(false),
                                "Cancel"
                            }
                            button {
                                r#type: "submit",
                                class: "px-4 py-2 text-sm font-medium rounded bg-indigo-600 text-white hover:bg-indigo-700 disabled:opacity-50",
                                disabled: saving(),
                                if saving() { "Saving…" } else { "Save" }
                            }
                        }
                    }
                }
            }
        }

        // Delete shelf modal
        if show_delete() {
            div { class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
                div { class: "bg-white rounded-lg shadow-xl p-6 w-full max-w-sm mx-4",
                    h2 { class: "text-lg font-semibold text-gray-900 mb-2", "Delete Shelf?" }
                    p { class: "text-sm text-gray-600 mb-6",
                        "This will permanently delete \"{current_name}\". Books will not be affected."
                    }
                    div { class: "flex gap-3 justify-end",
                        button {
                            class: "px-4 py-2 text-sm font-medium rounded border border-gray-300 text-gray-700 hover:bg-gray-50",
                            autofocus: true,
                            onclick: move |_| show_delete.set(false),
                            "Cancel"
                        }
                        button {
                            class: "px-4 py-2 text-sm font-medium rounded bg-red-600 text-white hover:bg-red-700 disabled:opacity-50",
                            disabled: deleting(),
                            onclick: {
                                let tok = token.clone();
                                move |_| {
                                    let tok = tok.clone();
                                    deleting.set(true);
                                    spawn(async move {
                                        if let Ok(()) = delete_shelf(tok).await { nav.push(Route::BooksPage {}); } else {
                                            deleting.set(false);
                                            show_delete.set(false);
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
    }
}
