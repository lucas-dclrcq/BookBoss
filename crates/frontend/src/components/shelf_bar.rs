use dioxus::prelude::*;

use crate::{
    Route,
    routes::shelf_page::{ShelfSummary, create_shelf},
};

/// Horizontal pill-button row listing the user's shelves and a "New Shelf"
/// button. Shown above the book grid in grid-view mode.
#[component]
pub(crate) fn ShelfBar(shelves: Vec<ShelfSummary>) -> Element {
    let navigator = use_navigator();

    rsx! {
        div { class: "flex items-center gap-2 px-4 py-2 bg-white border-b border-gray-200 overflow-x-auto shrink-0",
            // "All Books" pill — always active on this page
            Link {
                to: Route::BooksPage {},
                class: "px-3 py-1 rounded-full text-sm bg-indigo-600 text-white font-medium whitespace-nowrap shrink-0",
                "All Books"
            }

            // Own shelves
            for shelf in shelves.iter().filter(|s| s.is_own) {
                Link {
                    to: Route::ShelfPage { token: shelf.token.clone() },
                    class: "px-3 py-1 rounded-full text-sm bg-gray-100 text-gray-700 hover:bg-indigo-50 hover:text-indigo-600 whitespace-nowrap shrink-0",
                    "{shelf.name}"
                }
            }

            // Divider + public shelves from others
            if shelves.iter().any(|s| !s.is_own) {
                span { class: "text-gray-300 select-none shrink-0", "|" }
                for shelf in shelves.iter().filter(|s| !s.is_own) {
                    Link {
                        to: Route::ShelfPage { token: shelf.token.clone() },
                        class: "px-3 py-1 rounded-full text-sm bg-gray-100 text-gray-500 hover:bg-indigo-50 hover:text-indigo-600 whitespace-nowrap shrink-0 italic",
                        "{shelf.name}"
                    }
                }
            }

            // New Shelf button
            button {
                class: "ml-auto px-3 py-1 rounded-full text-sm border border-dashed border-gray-300 text-gray-500 hover:border-indigo-400 hover:text-indigo-600 whitespace-nowrap shrink-0",
                onclick: move |_| {
                    let nav = navigator;
                    async move {
                        if let Ok(token) = create_shelf("New Shelf".to_string(), "Private".to_string()).await {
                            nav.push(Route::ShelfPage { token });
                        }
                    }
                },
                "+ New Shelf"
            }
        }
    }
}
