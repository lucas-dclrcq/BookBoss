use std::collections::HashSet;

use dioxus::prelude::*;
#[cfg(feature = "server")]
use {
    crate::routes::server_helpers::authenticated_user, crate::server::AuthSession, bb_core::CoreServices, bb_core::book::BookToken,
    bb_core::reading::ReadStatus, std::str::FromStr, std::sync::Arc,
};

// ---------------------------------------------------------------------------
// Global selection state
// ---------------------------------------------------------------------------

/// Whether selection mode is currently active.
pub(crate) static SELECTION_MODE: GlobalSignal<bool> = Signal::global(|| false);

/// Set of book tokens currently selected.
pub(crate) static SELECTED_BOOKS: GlobalSignal<HashSet<String>> = Signal::global(HashSet::new);

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

pub(crate) fn toggle_selection(token: &str) {
    let mut selected = SELECTED_BOOKS.write();
    if selected.contains(token) {
        selected.remove(token);
    } else {
        selected.insert(token.to_string());
    }
}

pub(crate) fn select_all(tokens: impl IntoIterator<Item = String>) {
    let mut selected = SELECTED_BOOKS.write();
    for token in tokens {
        selected.insert(token);
    }
}

pub(crate) fn deselect_all() {
    SELECTED_BOOKS.write().clear();
}

pub(crate) fn is_selected(token: &str) -> bool {
    SELECTED_BOOKS.read().contains(token)
}

pub(crate) fn selection_count() -> usize {
    SELECTED_BOOKS.read().len()
}

pub(crate) fn exit_selection_mode() {
    *SELECTION_MODE.write() = false;
    SELECTED_BOOKS.write().clear();
}

// ---------------------------------------------------------------------------
// Server function: bulk set reading status
// ---------------------------------------------------------------------------

#[post(
    "/api/v1/books/bulk/reading-status",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
async fn bulk_set_reading_status(tokens: Vec<String>, status: String) -> Result<u32, ServerFnError> {
    let user_id = authenticated_user(&auth_session)?.id();

    let new_status: ReadStatus = status.parse().map_err(|e: String| ServerFnError::new(e))?;

    let mut updated = 0u32;
    for token in &tokens {
        let book_token = BookToken::from_str(token).map_err(|_| ServerFnError::new(format!("Invalid book token: {token}")))?;
        let book = core_services
            .book_service
            .find_book_by_token(book_token)
            .await
            .map_err(|e| ServerFnError::new(e.to_string()))?
            .ok_or_else(|| ServerFnError::new(format!("Book not found: {token}")))?;

        core_services
            .reading_service
            .set_status(user_id, book.id, new_status)
            .await
            .map_err(|e| ServerFnError::new(e.to_string()))?;
        updated += 1;
    }

    Ok(updated)
}

// ---------------------------------------------------------------------------
// SelectionToggle — small icon button to enter/exit selection mode
// ---------------------------------------------------------------------------

#[component]
pub(crate) fn SelectionToggle() -> Element {
    let mode = SELECTION_MODE();

    rsx! {
        button {
            r#type: "button",
            class: if mode {
                "flex items-center justify-center px-2 py-1.5 rounded bg-indigo-600 text-white cursor-pointer"
            } else {
                "flex items-center justify-center px-2 py-1.5 rounded text-gray-500 hover:text-indigo-600 hover:bg-gray-100 cursor-pointer"
            },
            title: if mode { "Exit selection mode" } else { "Select books" },
            onclick: move |_| {
                if mode {
                    exit_selection_mode();
                } else {
                    *SELECTION_MODE.write() = true;
                }
            },
            // Heroicons mini: check in a rounded square
            svg {
                class: "w-5 h-5",
                xmlns: "http://www.w3.org/2000/svg",
                view_box: "0 0 20 20",
                fill: "currentColor",
                // Rounded rectangle
                path {
                    d: "M3 5a2 2 0 012-2h10a2 2 0 012 2v10a2 2 0 01-2 2H5a2 2 0 01-2-2V5zm1.5.5v9a.5.5 0 00.5.5h10a.5.5 0 00.5-.5v-9a.5.5 0 00-.5-.5H5a.5.5 0 00-.5.5z",
                    fill_rule: "evenodd",
                    clip_rule: "evenodd",
                }
                // Checkmark inside
                path {
                    d: "M13.854 7.146a.5.5 0 010 .708l-4.5 4.5a.5.5 0 01-.708 0l-2-2a.5.5 0 11.708-.708L9 11.293l4.146-4.147a.5.5 0 01.708 0z",
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// SelectionActionBar — fixed bottom bar with actions for selected books
// ---------------------------------------------------------------------------

#[component]
pub(crate) fn SelectionActionBar(
    /// Tokens of all books currently rendered in the grid (for Select All).
    all_book_tokens: Vec<String>,
    /// Called after a bulk operation completes so the parent can refresh data.
    #[props(default)]
    on_action: EventHandler<()>,
) -> Element {
    let mode = SELECTION_MODE();
    let count = selection_count();
    let mut show_status_dropdown = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut status_message = use_signal(|| None::<String>);

    if !mode {
        return rsx! {};
    }

    let label = match count {
        0 => "No books selected".to_string(),
        1 => "1 book selected".to_string(),
        _ => format!("{count} books selected"),
    };

    let has_selection = count > 0;

    let all_selected = {
        let selected = SELECTED_BOOKS.read();
        !all_book_tokens.is_empty() && all_book_tokens.iter().all(|t| selected.contains(t))
    };

    rsx! {
        div {
            class: "fixed bottom-0 left-0 right-0 z-40 bg-white border-t border-gray-200 shadow-lg px-6 py-3 flex items-center gap-4",

            // Selection count or status message
            if let Some(msg) = status_message() {
                span { class: "text-sm font-medium text-indigo-600", "{msg}" }
            } else {
                span { class: "text-sm font-medium text-gray-700", "{label}" }
            }

            // Select All / Deselect All toggle
            button {
                class: "text-sm text-indigo-600 hover:text-indigo-800 font-medium cursor-pointer disabled:opacity-40",
                disabled: busy(),
                onclick: {
                    let tokens = all_book_tokens.clone();
                    move |_| {
                        if all_selected {
                            deselect_all();
                        } else {
                            select_all(tokens.clone());
                        }
                    }
                },
                if all_selected { "Deselect All" } else { "Select All" }
            }

            // Select None — clear selections but stay in selection mode
            if has_selection {
                button {
                    class: "text-sm text-indigo-600 hover:text-indigo-800 font-medium cursor-pointer disabled:opacity-40",
                    disabled: busy(),
                    onclick: move |_| deselect_all(),
                    "Select None"
                }
            }

            // Spacer
            div { class: "flex-1" }

            // Edit Metadata (wired up in Phase 3)
            button {
                class: "px-4 py-2 text-sm font-medium rounded bg-indigo-600 text-white hover:bg-indigo-700 cursor-pointer disabled:opacity-40",
                disabled: busy() || !has_selection,
                "Edit Metadata"
            }

            // Set Status with dropdown
            div { class: "relative",
                button {
                    class: "px-4 py-2 text-sm font-medium rounded bg-indigo-600 text-white hover:bg-indigo-700 cursor-pointer disabled:opacity-40",
                    disabled: busy() || !has_selection,
                    onclick: move |_| show_status_dropdown.set(!show_status_dropdown()),
                    "Set Status"
                }
                if show_status_dropdown() {
                    div { class: "absolute bottom-full left-0 mb-1 z-50 bg-white rounded-lg shadow-lg border border-gray-200 py-1 min-w-max",
                        for s in ["Unread", "Reading", "Paused", "Rereading", "Read", "Abandoned"] {
                            {
                                let s_owned = s.to_string();
                                rsx! {
                                    button {
                                        class: "block w-full text-left px-4 py-2 text-sm hover:bg-gray-50 cursor-pointer",
                                        onclick: move |_| {
                                            let status_str = s_owned.clone();
                                            let tokens: Vec<String> = SELECTED_BOOKS.read().iter().cloned().collect();
                                            let count = tokens.len();
                                            show_status_dropdown.set(false);
                                            busy.set(true);
                                            status_message.set(Some(format!("Updating {count} books…")));
                                            spawn(async move {
                                                match bulk_set_reading_status(tokens, status_str).await {
                                                    Ok(_) => {
                                                        busy.set(false);
                                                        status_message.set(None);
                                                        exit_selection_mode();
                                                        on_action.call(());
                                                    }
                                                    Err(e) => {
                                                        status_message.set(Some(format!("Error: {e}")));
                                                        busy.set(false);
                                                    }
                                                }
                                            });
                                        },
                                        "{s}"
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Cancel
            button {
                class: "px-4 py-2 text-sm font-medium rounded border border-gray-300 text-gray-700 hover:bg-gray-50 cursor-pointer disabled:opacity-40",
                disabled: busy(),
                onclick: move |_| exit_selection_mode(),
                "Cancel"
            }
        }
    }
}
