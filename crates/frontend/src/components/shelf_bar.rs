use dioxus::prelude::*;

use crate::{
    Route,
    routes::shelf_page::{ShelfSummary, create_shelf},
};

/// Horizontal pill-button row listing the user's shelves and a "New Shelf"
/// button. Shown above the book grid in grid-view mode.
#[component]
pub(crate) fn ShelfBar(shelves: Vec<ShelfSummary>, current_shelf_token: Option<String>) -> Element {
    let navigator = use_navigator();
    let mut show_modal = use_signal(|| false);
    let mut shelf_name = use_signal(String::new);
    let mut is_private = use_signal(|| true);
    let mut creating = use_signal(|| false);
    let mut error_msg: Signal<Option<String>> = use_signal(|| None);

    // Clone own shelf names for uniqueness check inside the closure.
    let own_names: Vec<String> = shelves.iter().filter(|s| s.is_own).map(|s| s.name.to_lowercase()).collect();

    // Validate name and submit.
    let mut do_create = move || {
        let name = shelf_name().trim().to_string();
        if name.is_empty() {
            error_msg.set(Some("Shelf name is required.".into()));
            return;
        }
        if own_names.contains(&name.to_lowercase()) {
            error_msg.set(Some("You already have a shelf with that name.".into()));
            return;
        }
        let visibility = if is_private() { "Private" } else { "Public" }.to_string();
        creating.set(true);
        error_msg.set(None);
        spawn(async move {
            match create_shelf(name, visibility).await {
                Ok(token) => {
                    show_modal.set(false);
                    creating.set(false);
                    navigator.push(Route::ShelfPage { token });
                }
                Err(e) => {
                    creating.set(false);
                    error_msg.set(Some(e.to_string()));
                }
            }
        });
    };

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
                    shelf_name.set(String::new());
                    is_private.set(true);
                    error_msg.set(None);
                    show_modal.set(true);
                },
                "+ New Shelf"
            }
        }

        // New Shelf modal
        if show_modal() {
            div { class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
                div { class: "bg-white rounded-lg shadow-xl p-6 w-full max-w-sm mx-4",
                    h2 { class: "text-lg font-semibold text-gray-900 mb-4", "New Shelf" }

                    form {
                        onsubmit: move |e| {
                            e.prevent_default();
                            do_create();
                        },

                        div { class: "mb-4",
                            label { class: "block text-sm font-medium text-gray-700 mb-1",
                                r#for: "shelf-name-input",
                                "Shelf name"
                            }
                            input {
                                id: "shelf-name-input",
                                class: "w-full px-3 py-2 border rounded text-sm outline-none focus:ring-1",
                                class: if error_msg().is_some() {
                                    "border-red-400 focus:border-red-500 focus:ring-red-500"
                                } else {
                                    "border-gray-300 focus:border-indigo-500 focus:ring-indigo-500"
                                },
                                r#type: "text",
                                placeholder: "e.g. To Read",
                                autofocus: true,
                                value: shelf_name(),
                                oninput: move |e| {
                                    shelf_name.set(e.value());
                                    error_msg.set(None);
                                },
                                onkeydown: move |e: KeyboardEvent| {
                                    if e.key() == Key::Escape {
                                        show_modal.set(false);
                                    }
                                },
                            }
                            if let Some(msg) = error_msg() {
                                p { class: "mt-1 text-xs text-red-600", "{msg}" }
                            }
                        }

                        div { class: "mb-6 flex items-center gap-2",
                            input {
                                id: "shelf-private-checkbox",
                                class: "h-4 w-4 rounded border-gray-300 text-indigo-600 focus:ring-indigo-500",
                                r#type: "checkbox",
                                checked: is_private(),
                                onchange: move |e| is_private.set(e.checked()),
                            }
                            label { class: "text-sm text-gray-700 cursor-pointer", r#for: "shelf-private-checkbox",
                                "Private"
                            }
                        }

                        div { class: "flex gap-3 justify-end",
                            button {
                                r#type: "button",
                                class: "px-4 py-2 text-sm font-medium rounded border border-gray-300 text-gray-700 hover:bg-gray-50",
                                onclick: move |_| show_modal.set(false),
                                "Cancel"
                            }
                            button {
                                r#type: "submit",
                                class: "px-4 py-2 text-sm font-medium rounded bg-indigo-600 text-white hover:bg-indigo-700 disabled:opacity-50",
                                disabled: creating(),
                                if creating() { "Creating…" } else { "Create" }
                            }
                        }
                    }
                }
            }
        }
    }
}
