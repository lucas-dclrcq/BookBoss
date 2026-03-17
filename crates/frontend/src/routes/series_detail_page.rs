use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    Route,
    components::{BookGrid, BookGridContext},
    routes::books_page::BookSummary,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SeriesPageData {
    pub token: String,
    pub name: String,
    pub description: Option<String>,
    pub books: Vec<BookSummary>,
}

#[cfg(feature = "server")]
use {
    crate::routes::{books_page::hydrate_books, server_helpers::authenticated_user},
    crate::server::AuthSession,
    bb_core::CoreServices,
    bb_core::book::{BookQuery, BookStatus, SeriesToken},
    std::str::FromStr,
    std::sync::Arc,
};

#[post("/api/v1/series", auth_session: axum::Extension<AuthSession>, core_services: axum::Extension<Arc<CoreServices>>)]
async fn get_series(token: String) -> Result<SeriesPageData, ServerFnError> {
    authenticated_user(&auth_session)?;

    let book_service = &core_services.book_service;

    let series_token = SeriesToken::from_str(&token).map_err(|_| ServerFnError::new("Invalid series token"))?;

    let series = book_service
        .find_series_by_token(&series_token)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .ok_or_else(|| ServerFnError::new("Series not found"))?;

    let filter = BookQuery {
        series_id: Some(series.id),
        status: Some(BookStatus::Available),
        ..Default::default()
    };
    let mut books = book_service
        .list_books(&filter, None, None)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Sort books by series_number ascending (None sorts last)
    books.sort_by(|a, b| match (&a.series_number, &b.series_number) {
        (Some(a), Some(b)) => a.cmp(b),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });

    let book_summaries = hydrate_books(&books, &core_services, None).await?;

    Ok(SeriesPageData {
        token: series.token.to_string(),
        name: series.name,
        description: series.description,
        books: book_summaries,
    })
}

#[component]
pub(crate) fn SeriesDetailPage(token: String) -> Element {
    use_context_provider(|| Signal::new(None::<String>)); // DraggedBookToken
    let series = use_server_future(move || get_series(token.clone()))?;

    rsx! {
        div { class: "flex-1 overflow-auto p-6",
            match series() {
                None => rsx! {
                    div { class: "flex items-center justify-center h-full text-gray-400 text-sm",
                        "Loading…"
                    }
                },
                Some(Err(e)) => rsx! {
                    div { class: "text-red-600 text-sm", "Failed to load series: {e}" }
                },
                Some(Ok(series)) => rsx! {
                    Link {
                        to: Route::BooksPage {},
                        class: "inline-flex items-center gap-1 text-sm text-indigo-600 hover:text-indigo-800 mb-6",
                        "← Library"
                    }

                    h1 { class: "text-2xl font-bold text-gray-900 mb-2", "{series.name}" }

                    if let Some(ref desc) = series.description {
                        p { class: "text-sm text-gray-600 leading-relaxed mb-6 max-w-prose", "{desc}" }
                    }

                    if !series.books.is_empty() {
                        h2 { class: "text-xs font-semibold uppercase tracking-wider text-gray-500 mb-4",
                            "Books"
                        }
                        BookGrid {
                            books: series.books,
                            context: BookGridContext::ReadOnly,
                            on_action: |()| {},
                        }
                    }
                },
            }
        }
    }
}
