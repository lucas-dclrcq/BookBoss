use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    Route,
    components::{BookGrid, BookGridContext, SelectionActionBar, ShelfBar, filter_books_by_search},
    routes::{
        book_detail_page::ReadingStateDto,
        shelf_page::{ShelfSummary, list_all_accessible_shelves},
    },
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct AuthorLink {
    pub token: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct BookSummary {
    pub token: String,
    pub title: String,
    pub has_cover: bool,
    pub authors: Vec<AuthorLink>,
    pub series_token: Option<String>,
    pub series_name: Option<String>,
    pub series_number: Option<String>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub reading_state: Option<ReadingStateDto>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ListBooksResponse {
    pub books: Vec<BookSummary>,
    pub can_delete_books: bool,
    /// Books with `Reading` status, sorted by `last_progress_at` desc.
    pub currently_reading: Vec<BookSummary>,
}

#[cfg(feature = "server")]
use {
    crate::components::to_core_sort,
    crate::routes::{
        book_detail_page::to_reading_state_dto,
        server_helpers::{authenticated_user, to_server_err},
    },
    crate::server::{AuthSession, AuthUser, BackendSessionPool},
    axum::http::Method,
    axum_session_auth::{Auth, Rights},
    bb_core::{
        CoreServices,
        book::{Author, AuthorId, Book, BookAuthor, BookHydrationData, BookId, BookQuery, Series, SeriesId},
        library::{ALL_BOOKS_LIBRARY_TOKEN, LibraryId, LibraryToken},
        reading::ReadStatus,
        types::Capability,
        user::UserId,
    },
    std::sync::Arc,
};

/// Hydrates a slice of `Book`s into `BookSummary` view-models.
///
/// Issues O(1) DB queries via a single `fetch_hydration_data` call
/// regardless of library size.
/// `reading_map` is an optional pre-built map of `book_id → ReadingStateDto`
/// used to attach per-user reading state; pass `None` for read-only contexts.
#[cfg(feature = "server")]
pub(crate) async fn hydrate_books(
    books: &[Book],
    core_services: &CoreServices,
    reading_map: Option<&std::collections::HashMap<u64, ReadingStateDto>>,
) -> Result<Vec<BookSummary>, ServerFnError> {
    use std::collections::HashMap;

    if books.is_empty() {
        return Ok(vec![]);
    }

    let book_ids: Vec<BookId> = books.iter().map(|b| b.id).collect();
    let series_ids: Vec<SeriesId> = {
        let mut ids: Vec<SeriesId> = books.iter().filter_map(|b| b.series_id).collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    };

    let data: BookHydrationData = core_services
        .book_service
        .fetch_hydration_data(&book_ids, &series_ids)
        .await
        .map_err(to_server_err)?;

    // Build lookup maps for O(1) assembly.
    let author_map: HashMap<AuthorId, &Author> = data.authors.iter().map(|a| (a.id, a)).collect();
    let series_map: HashMap<SeriesId, &Series> = data.series.iter().map(|s| (s.id, s)).collect();

    let mut book_authors_map: HashMap<BookId, Vec<&BookAuthor>> = HashMap::new();
    for ba in &data.book_authors {
        book_authors_map.entry(ba.book_id).or_default().push(ba);
    }

    let mut genres_map: HashMap<BookId, Vec<String>> = HashMap::new();
    for (book_id, genre) in &data.genres {
        genres_map.entry(*book_id).or_default().push(genre.name.clone());
    }

    let mut tags_map: HashMap<BookId, Vec<String>> = HashMap::new();
    for (book_id, tag) in &data.tags {
        tags_map.entry(*book_id).or_default().push(tag.name.clone());
    }

    let summaries = books
        .iter()
        .map(|book| {
            let mut author_links = book_authors_map.get(&book.id).cloned().unwrap_or_default();
            author_links.sort_by_key(|ba| ba.sort_order);
            let authors = author_links
                .iter()
                .filter_map(|ba| {
                    author_map.get(&ba.author_id).map(|a| AuthorLink {
                        token: a.token.to_string(),
                        name: a.name.clone(),
                    })
                })
                .collect();

            let (series_token, series_name) = book
                .series_id
                .and_then(|sid| series_map.get(&sid))
                .map_or((None, None), |s| (Some(s.token.to_string()), Some(s.name.clone())));

            BookSummary {
                token: book.token.to_string(),
                title: book.title.clone(),
                has_cover: book.has_cover,
                authors,
                series_token,
                series_name,
                series_number: book.series_number.as_ref().map(std::string::ToString::to_string),
                genres: genres_map.get(&book.id).cloned().unwrap_or_default(),
                tags: tags_map.get(&book.id).cloned().unwrap_or_default(),
                reading_state: reading_map.and_then(|m| m.get(&book.id).cloned()),
                created_at: book.created_at.to_rfc3339(),
                updated_at: book.updated_at.timestamp().to_string(),
            }
        })
        .collect();

    Ok(summaries)
}

/// Resolves an optional library token string to a `LibraryId`, validating user
/// access.
///
/// Returns `Ok(None)` when:
/// - `library_token` is `None` (no filter → all books)
/// - The token refers to the "All Books" system library (no filter needed)
///
/// Returns `Ok(Some(id))` for any other library the user has access to.
/// Returns `Err` if the token is malformed, the library does not exist, or the
/// user does not have access.
#[cfg(feature = "server")]
async fn resolve_library_id(core_services: &CoreServices, user_id: UserId, library_token: Option<String>) -> Result<Option<LibraryId>, ServerFnError> {
    let Some(token_str) = library_token else {
        return Ok(None);
    };
    // Fast path: All Books token → no library_id filter.
    if token_str == ALL_BOOKS_LIBRARY_TOKEN {
        return Ok(None);
    }
    let token = token_str.parse::<LibraryToken>().map_err(|_| ServerFnError::new("Invalid library token"))?;
    // Decode the library ID directly from the token (token encodes the ID).
    let library_id: LibraryId = token.id();
    // Validate that the user has access to this library.
    core_services
        .library_service
        .validate_user_library_access(user_id, library_id)
        .await
        .map_err(|_| ServerFnError::new("Access denied"))?;
    Ok(Some(library_id))
}

#[post("/api/v1/books", auth_session: axum::Extension<AuthSession>, core_services: axum::Extension<Arc<CoreServices>>)]
async fn list_books(sort: crate::components::SortOrder, library_token: Option<String>) -> Result<ListBooksResponse, ServerFnError> {
    use std::collections::HashMap;

    let current_user = authenticated_user(&auth_session)?;
    let user_id = current_user.id();

    let can_delete_books = Auth::<AuthUser, _, BackendSessionPool>::build([Method::POST], true)
        .requires(Rights::any([Rights::permission(Capability::DeleteBook.as_str())]))
        .validate(&current_user, &Method::POST, None)
        .await;

    let library_id = resolve_library_id(&core_services, user_id, library_token).await?;

    let filter = BookQuery {
        sort: Some(to_core_sort(sort)),
        ..Default::default()
    };
    let books = core_services
        .book_service
        .list_books(&filter, library_id, None, None)
        .await
        .map_err(to_server_err)?;

    // Load per-user reading state scoped to the fetched books (avoids page limits).
    let book_ids: Vec<bb_core::book::BookId> = books.iter().map(|b| b.id).collect();
    let reading_metas = core_services
        .reading_service
        .list_for_user_and_books(user_id, &book_ids)
        .await
        .map_err(to_server_err)?;
    let reading_map: HashMap<u64, ReadingStateDto> = reading_metas
        .iter()
        .filter(|m| m.read_status != ReadStatus::Unread)
        .map(|m| (m.book_id, to_reading_state_dto(m)))
        .collect();

    let summaries = hydrate_books(&books, &core_services, Some(&reading_map)).await?;

    // Build the "Currently Reading" list: Reading books sorted by last_progress_at
    // desc.
    let book_id_to_idx: HashMap<u64, usize> = books.iter().enumerate().map(|(i, b)| (b.id, i)).collect();
    let mut reading_now: Vec<_> = reading_metas
        .iter()
        .filter(|m| matches!(m.read_status, ReadStatus::Reading | ReadStatus::Rereading | ReadStatus::Paused))
        .collect();
    reading_now.sort_by_key(|b| std::cmp::Reverse(b.last_progress_at));
    let currently_reading: Vec<BookSummary> = reading_now
        .iter()
        .filter_map(|m| book_id_to_idx.get(&m.book_id).map(|&idx| summaries[idx].clone()))
        .collect();

    Ok(ListBooksResponse {
        books: summaries,
        can_delete_books,
        currently_reading,
    })
}

#[component]
pub(crate) fn BooksPage() -> Element {
    use crate::components::ACTIVE_LIBRARY;
    use_context_provider(|| Signal::new(None::<String>)); // DraggedBookToken
    let mut books_refresh = use_signal(|| 0u32);
    let mut page_data = use_server_future(move || {
        let sort = crate::components::SORT_ORDER();
        let library_token = ACTIVE_LIBRARY();
        let _ = books_refresh();
        list_books(sort, library_token)
    })?;
    let mut shelves_resource = use_server_future(list_all_accessible_shelves)?;
    let shelves: Vec<ShelfSummary> = shelves_resource().and_then(std::result::Result::ok).unwrap_or_default();
    rsx! {
        match page_data() {
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
            Some(Ok(ListBooksResponse { books, can_delete_books, currently_reading })) => {
                let query = crate::components::SEARCH_TEXT();
                let filtered_books = filter_books_by_search(books, &query);
                let filtered_reading = filter_books_by_search(currently_reading, &query);
                let has_search = !query.trim().is_empty();
                let book_tokens: Vec<String> = filtered_books.iter().map(|b| b.token.clone()).collect();
                rsx! {
                    div { class: "flex-1 flex flex-col overflow-hidden",
                        ShelfBar {
                            shelves,
                            current_shelf_token: None,
                            on_edit_shelf: |()| {},
                            on_delete_shelf: |()| {},
                        }
                        div { class: "flex-1 overflow-auto",
                            CurrentlyReadingSection { books: filtered_reading }
                            if filtered_books.is_empty() && has_search {
                                div { class: "p-8 text-center text-gray-400 text-sm",
                                    "No books match your search."
                                }
                            } else {
                                BookGrid {
                                    books: filtered_books,
                                    context: BookGridContext::AllBooks { can_delete: can_delete_books },
                                    on_action: move |()| page_data.restart(),
                                }
                            }
                        }
                    }
                    SelectionActionBar {
                        all_book_tokens: book_tokens,
                        on_action: move |()| {
                            *books_refresh.write() += 1;
                            shelves_resource.restart();
                        },
                    }
                }
            },
        }
    }
}

// ---------------------------------------------------------------------------
// CurrentlyReadingSection
// ---------------------------------------------------------------------------

#[component]
fn CurrentlyReadingSection(books: Vec<BookSummary>) -> Element {
    if books.is_empty() {
        return rsx! {};
    }

    let navigator = use_navigator();

    rsx! {
        div { class: "shrink-0 border-b border-gray-100 bg-gray-50 px-4 pt-3 pb-2",
            h2 { class: "text-xs font-semibold uppercase tracking-wider text-gray-500 mb-2",
                "Currently Reading"
            }
            div { class: "flex gap-3 overflow-x-auto pb-1",
                for book in &books {
                    {
                        let tok = book.token.clone();
                        let pct = book.reading_state.as_ref().and_then(|s| s.progress_pct).unwrap_or(0);
                        rsx! {
                            div {
                                class: "relative flex-none w-20 cursor-pointer",
                                onclick: move |_| {
                                    navigator.push(Route::BookDetailPage { token: tok.clone() });
                                },
                                img {
                                    src: "/api/v1/covers/{book.token}?v={book.updated_at}",
                                    alt: "{book.title}",
                                    class: "w-full object-cover rounded shadow-sm",
                                    style: "aspect-ratio: 2/3",
                                }
                                if pct > 0 {
                                    div {
                                        class: "absolute bottom-0 left-0 right-0 h-1 bg-black/20 rounded-b overflow-hidden",
                                        div { class: "h-full bg-indigo-400", style: "width: {pct}%" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
