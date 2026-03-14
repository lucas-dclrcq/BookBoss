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

/// Paginated book result for a shelf.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ShelfBooksPage {
    pub books: Vec<BookSummary>,
    /// Last `book_id` seen; pass as `cursor` to fetch the next page.
    /// `None` when there are no more results.
    pub next_cursor: Option<u64>,
}

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
    /// `true` if this shelf is managed by a device (delete is disabled).
    pub is_device_shelf: bool,
    /// Serialized `BookFilter` JSON — present only for smart shelves owned by
    /// the current user.
    pub filter_json: Option<String>,
    /// Matching book count — populated for own smart shelves only.
    pub count: Option<u64>,
}

// ---------------------------------------------------------------------------
// Server-only imports
// ---------------------------------------------------------------------------

#[cfg(feature = "server")]
use {
    crate::server::AuthSession,
    bb_core::{
        CoreServices,
        book::{AuthorToken, Book, BookToken, SeriesToken},
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
                is_device_shelf: s.device_id.is_some(),
                filter_json,
                count: None,
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

    let own_shelves = shelf_service
        .list_shelves_for_user(user_id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    let mut own = Vec::with_capacity(own_shelves.len());
    for s in own_shelves {
        let is_smart = s.shelf_type == ShelfType::Smart;
        let filter_json = if is_smart {
            s.filter_criteria.as_ref().and_then(|f| serde_json::to_string(f).ok())
        } else {
            None
        };
        let count = if is_smart {
            shelf_service.count_for_filter(&s.token, user_id).await.ok()
        } else {
            None
        };
        own.push(ShelfSummary {
            token: s.token.to_string(),
            name: s.name.clone(),
            visibility: visibility_str(&s.visibility).to_string(),
            is_own: true,
            is_smart,
            is_device_shelf: s.device_id.is_some(),
            filter_json,
            count,
        });
    }

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
            is_device_shelf: false,
            filter_json: None,
            count: None,
        });

    own.extend(others);
    Ok(own)
}

/// Returns a paginated list of books on a shelf, with author and series data
/// hydrated.
///
/// `cursor` is the last `book_id` seen (exclusive lower bound for the next
/// page). `page_size` defaults to 48 when `None`.
///
/// Smart shelves use the shelf's filter; manual shelves use the junction table.
#[post(
    "/api/v1/shelves/books/list",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn books_for_shelf(token: String, cursor: Option<u64>, page_size: Option<u64>) -> Result<ShelfBooksPage, ServerFnError> {
    use std::collections::{HashMap, HashSet};

    const PAGE_SIZE: u64 = 48;

    let user = auth_session
        .current_user
        .as_ref()
        .filter(|u| !u.username.is_empty())
        .ok_or_else(|| ServerFnError::new("Not authenticated"))?;
    let user_id = user.id();

    let shelf_token: ShelfToken = token.parse().map_err(|_| ServerFnError::new("Invalid shelf token"))?;

    let shelf_service = &core_services.shelf_service;
    let book_service = &core_services.book_service;

    let effective_page_size = page_size.unwrap_or(PAGE_SIZE);

    // Load the shelf to determine if it's smart or manual.
    let shelf = shelf_service
        .get_shelf(&shelf_token, user_id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Fetch books: smart → filter query; manual → junction table + book lookup.
    let books: Vec<Book> = if shelf.shelf_type == ShelfType::Smart {
        shelf_service
            .books_for_filter(&shelf_token, user_id, cursor, Some(effective_page_size))
            .await
            .map_err(|e| ServerFnError::new(e.to_string()))?
    } else {
        let shelf_entries = shelf_service
            .books_for_shelf(&shelf_token, user_id, cursor, Some(effective_page_size))
            .await
            .map_err(|e| ServerFnError::new(e.to_string()))?;

        let mut books = Vec::with_capacity(shelf_entries.len());
        for entry in &shelf_entries {
            let book = book_service
                .find_book_by_token(&BookToken::new(entry.book_id))
                .await
                .map_err(|e| ServerFnError::new(e.to_string()))?
                .ok_or_else(|| ServerFnError::new("Book not found"))?;
            books.push(book);
        }
        books
    };

    // Determine cursor for the next page.
    let next_cursor = if books.len() as u64 >= effective_page_size {
        books.last().map(|b| b.id)
    } else {
        None
    };

    if books.is_empty() {
        return Ok(ShelfBooksPage {
            books: vec![],
            next_cursor: None,
        });
    }

    // Hydrate authors.
    let mut all_author_ids: HashSet<u64> = HashSet::new();
    let mut books_with_authors = Vec::with_capacity(books.len());

    for book in &books {
        let authors = book_service.authors_for_book(book.id).await.map_err(|e| ServerFnError::new(e.to_string()))?;
        for ba in &authors {
            all_author_ids.insert(ba.author_id);
        }
        books_with_authors.push((book.clone(), authors));
    }

    // Batch-load unique authors.
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

    // Batch-load unique series.
    let unique_series: HashSet<u64> = books_with_authors.iter().filter_map(|(b, _)| b.series_id).collect();
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

    // Assemble summaries.
    let book_summaries = books_with_authors
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

    Ok(ShelfBooksPage {
        books: book_summaries,
        next_cursor,
    })
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

    // Accumulated load-more books (first page comes from books_resource).
    let mut extra_books: Signal<Vec<BookSummary>> = use_signal(Vec::new);
    let mut next_cursor: Signal<Option<u64>> = use_signal(|| None);
    let mut loading_more = use_signal(|| false);
    let mut load_more_error: Signal<Option<String>> = use_signal(|| None);

    // Sync cursor from first page; reset accumulated state when token/data changes.
    use_effect(move || {
        let _ = token_sig(); // subscribe to token changes
        extra_books.set(vec![]);
        load_more_error.set(None);
        match books_resource() {
            Some(Ok(ref page)) => next_cursor.set(page.next_cursor),
            Some(Err(_)) | None => next_cursor.set(None),
        }
    });

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

    // Smart shelves are read-only (no drag-drop, no remove button).
    let context = if is_own && !current_is_smart {
        BookGridContext::OwnShelf { shelf_token: token.clone() }
    } else {
        BookGridContext::ReadOnly
    };

    // Merged book list: first page + any load-more pages.
    let first_books = books_resource().and_then(Result::ok).map(|p| p.books).unwrap_or_default();
    let all_books: Vec<BookSummary> = first_books.into_iter().chain(extra_books()).collect();
    let has_more = next_cursor().is_some();

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
                is_device_shelf: current_shelf.as_ref().is_some_and(|s| s.is_device_shelf),
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
                Some(Ok(_)) => {
                    if all_books.is_empty() {
                        rsx! {
                            div { class: "flex-1 flex flex-col items-center justify-center py-20 text-center",
                                p { class: "text-gray-400 text-sm",
                                    if current_is_smart { "No books match this filter." } else { "No books on this shelf yet." }
                                }
                                if is_own && !current_is_smart {
                                    p { class: "text-gray-300 text-xs mt-1",
                                        "Drag a book here or open any book and use \"Add to Shelf\"."
                                    }
                                }
                            }
                        }
                    } else {
                        rsx! {
                            div { class: "flex-1 flex flex-col overflow-hidden",
                                BookGrid {
                                    books: all_books.clone(),
                                    context: context.clone(),
                                    on_action: move |()| books_resource.restart(),
                                }
                                if has_more {
                                    div { class: "flex flex-col items-center gap-2 py-4 shrink-0",
                                        if let Some(err) = load_more_error() {
                                            p { class: "text-red-600 text-xs", "{err}" }
                                        }
                                        button {
                                            class: "px-6 py-2 text-sm font-medium rounded bg-indigo-600 text-white hover:bg-indigo-700 disabled:opacity-50",
                                            disabled: loading_more(),
                                            onclick: {
                                                let tok = token.clone();
                                                move |_| {
                                                    let tok = tok.clone();
                                                    let cursor = next_cursor();
                                                    loading_more.set(true);
                                                    load_more_error.set(None);
                                                    spawn(async move {
                                                        match books_for_shelf(tok, cursor, None).await {
                                                            Ok(page) => {
                                                                next_cursor.set(page.next_cursor);
                                                                extra_books.write().extend(page.books);
                                                                loading_more.set(false);
                                                            }
                                                            Err(e) => {
                                                                load_more_error.set(Some(e.to_string()));
                                                                loading_more.set(false);
                                                            }
                                                        }
                                                    });
                                                }
                                            },
                                            if loading_more() { "Loading…" } else { "Load more" }
                                        }
                                    }
                                }
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
                                                books_resource.restart();
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
