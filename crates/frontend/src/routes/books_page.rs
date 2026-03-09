use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    Route,
    components::{BookGrid, BookGridContext, BookTable, ShelfBar, TreeExplorer},
    routes::{
        book_detail_page::ReadingStateDto,
        shelf_page::{ShelfSummary, list_all_accessible_shelves},
    },
    settings::BookDisplayView,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct BookSummary {
    pub token: String,
    pub title: String,
    pub cover_path: Option<String>,
    pub author_names: Vec<String>,
    pub series_name: Option<String>,
    pub series_number: Option<String>,
    pub reading_state: Option<ReadingStateDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ListBooksResponse {
    pub books: Vec<BookSummary>,
    pub can_delete_books: bool,
    /// Books with `Reading` status, sorted by `last_progress_at` desc.
    pub currently_reading: Vec<BookSummary>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TreeCategory {
    pub name: String,
    /// Each item is `(label, optional_route)`. Items without a route are
    /// rendered as plain text.
    pub items: Vec<(String, Option<Route>)>,
}

fn build_categories(shelves: &[ShelfSummary]) -> Vec<TreeCategory> {
    let mut cats = vec![
        TreeCategory {
            name: "Genres".into(),
            items: vec![("Fantasy".into(), None), ("Science Fiction".into(), None), ("Non-fiction".into(), None)],
        },
        TreeCategory {
            name: "Authors".into(),
            items: vec![],
        },
        TreeCategory {
            name: "Series".into(),
            items: vec![],
        },
    ];

    {
        let mut items = vec![("All Books".into(), Some(Route::BooksPage {}))];
        items.extend(shelves.iter().map(|s| (s.name.clone(), Some(Route::ShelfPage { token: s.token.clone() }))));
        cats.push(TreeCategory { name: "Shelves".into(), items });
    }

    cats
}

#[cfg(feature = "server")]
use {
    crate::server::{AuthSession, AuthUser, BackendSessionPool},
    axum::http::Method,
    axum_session_auth::{Auth, Rights},
    bb_core::{
        CoreServices,
        book::{AuthorToken, BookFilter, BookStatus, SeriesToken},
        reading::ReadStatus,
        types::Capability,
    },
    std::sync::Arc,
};

#[get("/api/v1/books", auth_session: axum::Extension<AuthSession>, core_services: axum::Extension<Arc<CoreServices>>)]
async fn list_books() -> Result<ListBooksResponse, ServerFnError> {
    use std::collections::{HashMap, HashSet};

    let current_user = auth_session
        .current_user
        .as_ref()
        .filter(|u| !u.username.is_empty())
        .ok_or_else(|| ServerFnError::new("Not authenticated"))?
        .clone();

    let user_id = current_user.id();

    let can_delete_books = Auth::<AuthUser, _, BackendSessionPool>::build([Method::POST], true)
        .requires(Rights::any([Rights::permission(Capability::DeleteBook.as_str())]))
        .validate(&current_user, &Method::POST, None)
        .await;

    let book_service = &core_services.book_service;

    let filter = BookFilter {
        status: Some(BookStatus::Available),
        ..Default::default()
    };
    let books = book_service
        .list_books(&filter, None, None)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Gather per-book author links and collect unique author IDs
    let mut book_authors: Vec<Vec<(i32, u64)>> = Vec::with_capacity(books.len());
    let mut all_author_ids: HashSet<u64> = HashSet::new();
    for book in &books {
        let authors = book_service.authors_for_book(book.id).await.map_err(|e| ServerFnError::new(e.to_string()))?;
        let pairs: Vec<(i32, u64)> = authors.iter().map(|ba| (ba.sort_order, ba.author_id)).collect();
        for &(_, aid) in &pairs {
            all_author_ids.insert(aid);
        }
        book_authors.push(pairs);
    }

    // Fetch each unique author once
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

    // Fetch each unique series once
    let unique_series: HashSet<u64> = books.iter().filter_map(|b| b.series_id).collect();
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

    // Load per-user reading state for all books in one query
    let reading_metas = core_services
        .reading_service
        .list_for_user(user_id, None)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    let reading_map: std::collections::HashMap<u64, ReadingStateDto> = reading_metas
        .iter()
        .filter(|m| m.read_status != ReadStatus::Unread)
        .map(|m| {
            let dto = ReadingStateDto {
                status: match m.read_status {
                    ReadStatus::Unread => "Unread",
                    ReadStatus::Reading => "Reading",
                    ReadStatus::Read => "Read",
                    ReadStatus::Dnf => "Dnf",
                }
                .to_string(),
                progress_pct: m.progress_percentage.map(|bps| (bps / 100) as u8),
                personal_rating: m.personal_rating,
                times_read: m.times_read,
                notes: m.notes.clone(),
            };
            (m.book_id, dto)
        })
        .collect();

    // Assemble view models
    let summaries: Vec<BookSummary> = books
        .iter()
        .zip(book_authors.iter())
        .map(|(book, author_pairs)| {
            let mut sorted = author_pairs.clone();
            sorted.sort_by_key(|&(order, _)| order);
            let author_names = sorted.iter().filter_map(|&(_, aid)| author_map.get(&aid).cloned()).collect();
            BookSummary {
                token: book.token.to_string(),
                title: book.title.clone(),
                cover_path: book.cover_path.clone(),
                author_names,
                series_name: book.series_id.and_then(|sid| series_map.get(&sid).cloned()),
                series_number: book.series_number.as_ref().map(|n| n.to_string()),
                reading_state: reading_map.get(&book.id).cloned(),
            }
        })
        .collect();

    // Build the "Currently Reading" list: Reading books sorted by last_progress_at
    // desc.
    let book_id_to_idx: HashMap<u64, usize> = books.iter().enumerate().map(|(i, b)| (b.id, i)).collect();
    let mut reading_now: Vec<_> = reading_metas.iter().filter(|m| m.read_status == ReadStatus::Reading).collect();
    reading_now.sort_by(|a, b| b.last_progress_at.cmp(&a.last_progress_at));
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
    use_context_provider(|| Signal::new(None::<String>)); // DraggedBookToken
    let view: Signal<BookDisplayView> = use_context();
    let mut page_data = use_server_future(list_books)?;
    let shelves_resource = use_server_future(list_all_accessible_shelves)?;
    let shelves: Vec<ShelfSummary> = shelves_resource().and_then(|r| r.ok()).unwrap_or_default();
    let categories = build_categories(&shelves);

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
            Some(Ok(ListBooksResponse { books, can_delete_books, currently_reading })) => rsx! {
                match *view.read() {
                    BookDisplayView::GridView => rsx! {
                        div { class: "flex-1 flex flex-col overflow-hidden",
                            ShelfBar {
                                shelves,
                                current_shelf_token: None,
                                on_edit_shelf: |_| {},
                                on_delete_shelf: |_| {},
                            }
                            CurrentlyReadingSection { books: currently_reading }
                            BookGrid {
                                books,
                                context: BookGridContext::AllBooks { can_delete: can_delete_books },
                                on_action: move |_| page_data.restart(),
                            }
                        }
                    },
                    BookDisplayView::TableView => rsx! {
                        TreeExplorer { categories }
                        BookTable { books }
                    },
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
        div { class: "shrink-0 border-b border-gray-100 bg-white px-4 pt-3 pb-2",
            h2 { class: "text-xs font-semibold uppercase tracking-wider text-gray-500 mb-2",
                "Currently Reading"
            }
            div { class: "flex gap-3 overflow-x-auto pb-1",
                for book in &books {
                    {
                        let tok = book.token.clone();
                        let pct = book.reading_state.as_ref().and_then(|s| s.progress_pct).unwrap_or(0);
                        let author_str = book.author_names.join(", ");
                        rsx! {
                            div {
                                class: "flex-none w-20 cursor-pointer",
                                onclick: move |_| {
                                    navigator.push(Route::BookDetailPage { token: tok.clone() });
                                },
                                div { class: "relative",
                                    img {
                                        src: "/api/v1/covers/{book.token}",
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
                                p { class: "text-xs font-semibold text-gray-900 leading-tight line-clamp-2 mt-1",
                                    "{book.title}"
                                }
                                p { class: "text-xs text-gray-500 leading-tight truncate mt-0.5",
                                    "{author_str}"
                                }
                                p { class: "text-xs text-indigo-600 font-medium mt-0.5", "{pct}%" }
                            }
                        }
                    }
                }
            }
        }
    }
}
