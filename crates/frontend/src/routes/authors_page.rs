use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::Route;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct AuthorRow {
    pub token: String,
    pub name: String,
    pub book_count: u64,
}

#[cfg(feature = "server")]
use {crate::routes::server_helpers::authenticated_user, crate::server::AuthSession, bb_core::CoreServices, std::sync::Arc};

#[get("/api/v1/authors/list", auth_session: axum::Extension<AuthSession>, core_services: axum::Extension<Arc<CoreServices>>)]
async fn get_authors() -> Result<Vec<AuthorRow>, ServerFnError> {
    authenticated_user(&auth_session)?;

    let book_service = &core_services.book_service;

    let authors = book_service.list_all_authors().await.map_err(|e| ServerFnError::new(e.to_string()))?;

    let mut rows = Vec::with_capacity(authors.len());
    for author in &authors {
        let count = book_service
            .count_books_for_author(author.id)
            .await
            .map_err(|e| ServerFnError::new(e.to_string()))?;
        rows.push(AuthorRow {
            token: author.token.to_string(),
            name: author.name.clone(),
            book_count: count,
        });
    }

    Ok(rows)
}

#[component]
pub(crate) fn AuthorsPage() -> Element {
    let authors = use_server_future(get_authors)?;

    rsx! {
        div { class: "flex-1 overflow-auto p-6",
            match authors() {
                None => rsx! {
                    div { class: "flex items-center justify-center h-full text-gray-400 text-sm",
                        "Loading…"
                    }
                },
                Some(Err(e)) => rsx! {
                    div { class: "text-red-600 text-sm", "Failed to load authors: {e}" }
                },
                Some(Ok(authors)) => {
                    let query = crate::components::SEARCH_TEXT();
                    let query_lower = query.trim().to_lowercase();
                    let filtered: Vec<&AuthorRow> = if query_lower.is_empty() {
                        authors.iter().collect()
                    } else {
                        authors.iter().filter(|a| a.name.to_lowercase().contains(&query_lower)).collect()
                    };
                    let has_search = !query_lower.is_empty();

                    rsx! {
                        Link {
                            to: Route::BooksPage {},
                            class: "inline-flex items-center gap-1 text-sm text-indigo-600 hover:text-indigo-800 mb-6",
                            "← Library"
                        }

                        h1 { class: "text-2xl font-bold text-gray-900 mb-6", "Authors" }

                        if filtered.is_empty() && has_search {
                            p { class: "text-gray-400 text-sm mt-4", "No authors match your search." }
                        } else if filtered.is_empty() {
                            p { class: "text-gray-400 text-sm mt-4", "No authors in your library." }
                        } else {
                            table { class: "w-full max-w-2xl",
                                thead {
                                    tr { class: "border-b border-gray-200",
                                        th { class: "text-left text-xs font-semibold uppercase tracking-wider text-gray-500 pb-2 pr-4",
                                            "Name"
                                        }
                                        th { class: "text-center text-xs font-semibold uppercase tracking-wider text-gray-500 pb-2",
                                            "Books"
                                        }
                                    }
                                }
                                tbody {
                                    for author in &filtered {
                                        tr { class: "border-b border-gray-100 hover:bg-gray-50",
                                            key: "{author.token}",
                                            td { class: "py-2 pr-4",
                                                Link {
                                                    to: Route::AuthorDetailPage { token: author.token.clone() },
                                                    class: "text-sm text-indigo-600 hover:text-indigo-800",
                                                    "{author.name}"
                                                }
                                            }
                                            td { class: "py-2 text-center text-sm text-gray-600",
                                                "{author.book_count}"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
            }
        }
    }
}
