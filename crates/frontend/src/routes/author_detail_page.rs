use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    Route,
    components::{BookGrid, BookGridContext, filter_books_by_search},
    routes::books_page::BookSummary,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct AuthorPageData {
    pub token: String,
    pub name: String,
    pub bio: Option<String>,
    pub books: Vec<BookSummary>,
}

#[cfg(feature = "server")]
use {
    crate::routes::{books_page::hydrate_books, server_helpers::authenticated_user},
    crate::server::AuthSession,
    bb_core::CoreServices,
    bb_core::book::{AuthorToken, BookQuery, BookSortField, BookSortOrder, SortDirection},
    std::str::FromStr,
    std::sync::Arc,
};

#[post("/api/v1/author", auth_session: axum::Extension<AuthSession>, core_services: axum::Extension<Arc<CoreServices>>)]
async fn get_author(token: String) -> Result<AuthorPageData, ServerFnError> {
    authenticated_user(&auth_session)?;

    let book_service = &core_services.book_service;

    let author_token = AuthorToken::from_str(&token).map_err(|_| ServerFnError::new("Invalid author token"))?;

    let author = book_service
        .find_author_by_token(author_token)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .ok_or_else(|| ServerFnError::new("Author not found"))?;

    let filter = BookQuery {
        author_id: Some(author.id),
        sort: Some(BookSortOrder {
            field: BookSortField::Title,
            direction: SortDirection::Asc,
        }),
        ..Default::default()
    };
    let books = book_service
        .list_books(&filter, None, None)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    let book_summaries = hydrate_books(&books, &core_services, None).await?;

    Ok(AuthorPageData {
        token: author.token.to_string(),
        name: author.name,
        bio: author.bio,
        books: book_summaries,
    })
}

#[component]
pub(crate) fn AuthorDetailPage(token: String) -> Element {
    use_context_provider(|| Signal::new(None::<String>)); // DraggedBookToken
    let author = use_server_future(move || get_author(token.clone()))?;

    rsx! {
        div { class: "flex-1 overflow-auto p-6",
            match author() {
                None => rsx! {
                    div { class: "flex items-center justify-center h-full text-gray-400 text-sm",
                        "Loading…"
                    }
                },
                Some(Err(e)) => rsx! {
                    div { class: "text-red-600 text-sm", "Failed to load author: {e}" }
                },
                Some(Ok(author)) => {
                    let query = crate::components::SEARCH_TEXT();
                    let filtered_books = filter_books_by_search(author.books, &query);
                    let has_search = !query.trim().is_empty();
                    rsx! {
                        Link {
                            to: Route::BooksPage {},
                            class: "inline-flex items-center gap-1 text-sm text-indigo-600 hover:text-indigo-800 mb-6",
                            "← Library"
                        }

                        h1 { class: "text-2xl font-bold text-gray-900 mb-2", "{author.name}" }

                        if let Some(ref bio) = author.bio {
                            p { class: "text-sm text-gray-600 leading-relaxed mb-6 max-w-prose", "{bio}" }
                        }

                        if filtered_books.is_empty() && has_search {
                            p { class: "text-gray-400 text-sm mt-4", "No books match your search." }
                        } else if !filtered_books.is_empty() {
                            h2 { class: "text-xs font-semibold uppercase tracking-wider text-gray-500 mb-4",
                                "Books"
                            }
                            BookGrid {
                                books: filtered_books,
                                context: BookGridContext::ReadOnly {
                                    current_author_token: Some(author.token.clone()),
                                    current_series_token: None,
                                },
                                on_action: |()| {},
                            }
                        }
                    }
                },
            }
        }
    }
}
