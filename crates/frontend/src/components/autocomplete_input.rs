use dioxus::prelude::*;

use super::chip_input::word_match;

/// Single-value text input with a live pick-list dropdown for series names.
///
/// - When a value is set, it renders as a removable pill. Chips whose name is
///   not found in `options` display a **new** badge in green.
/// - When empty, focuses immediately show up to 8 options from the pick-list.
/// - Typing 1+ characters filters `options` by word-order-independent matching.
/// - Clicking a suggestion sets the value, closes the dropdown, and fires
///   `on_series_selected` with `(series_name, suggested_next_number)`.
/// - The × on the pill clears the value and fires `on_cleared`.
/// - `on_blur` (fired on focus-out with the current value) lets the parent set
///   a default book number when the user types a brand-new series name.
#[component]
pub(crate) fn AutocompleteInput(
    mut value: Signal<String>,
    options: Vec<(String, u32)>,
    on_series_selected: EventHandler<(String, u32)>,
    on_cleared: EventHandler<()>,
    on_blur: EventHandler<String>,
) -> Element {
    let mut show_dropdown = use_signal(|| false);

    let query = value.read().clone();
    let show = *show_dropdown.read();

    let filtered: Vec<(String, u32)> = if query.is_empty() {
        if show { options.iter().take(8).cloned().collect() } else { vec![] }
    } else {
        options.iter().filter(|(name, _)| word_match(name, &query)).take(8).cloned().collect()
    };

    let is_set = !query.is_empty();
    let is_new = is_set && !options.iter().any(|(name, _)| name.eq_ignore_ascii_case(&query));

    rsx! {
        div { class: "relative flex-1",
            if is_set {
                // ── Pill display ──────────────────────────────────────────────
                div { class: "flex flex-wrap gap-1 items-center border border-gray-300 rounded px-2 py-1 min-h-[34px]",
                    {
                        let label = query.clone();
                        let chip_class = if is_new {
                            "inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs bg-green-100 text-green-800 border border-green-300"
                        } else {
                            "inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs bg-gray-100 text-gray-700 border border-gray-300"
                        };
                        rsx! {
                            span { class: "{chip_class}",
                                "{label}"
                                if is_new {
                                    span { class: "font-semibold ml-0.5 text-green-700", "new" }
                                }
                                button {
                                    r#type: "button",
                                    class: "ml-0.5 text-gray-400 hover:text-gray-700 cursor-pointer leading-none",
                                    onclick: move |_| {
                                        value.write().clear();
                                        on_cleared.call(());
                                    },
                                    "×"
                                }
                            }
                        }
                    }
                }
            } else {
                // ── Text input ────────────────────────────────────────────────
                input {
                    class: "w-full border border-gray-300 rounded px-2 py-1 text-sm focus:outline-none focus:ring-1 focus:ring-indigo-400",
                    value: "{value}",
                    placeholder: "Series name",
                    oninput: move |e| {
                        value.set(e.value());
                        show_dropdown.set(true);
                    },
                    onfocus: move |_| {
                        show_dropdown.set(true);
                    },
                    onfocusout: move |_| {
                        show_dropdown.set(false);
                        on_blur.call(value.read().clone());
                    },
                    onkeydown: move |e| {
                        if e.key() == Key::Escape {
                            show_dropdown.set(false);
                        }
                    },
                }
            }
            // ── Dropdown ──────────────────────────────────────────────────────
            if show && !filtered.is_empty() {
                div { class: "absolute left-0 right-0 top-full mt-1 bg-white border border-gray-200 rounded shadow-lg z-50 max-h-48 overflow-y-auto",
                    for (name, next_num) in filtered {
                        {
                            let label = name.clone();
                            let click_name = name;
                            rsx! {
                                div {
                                    key: "{label}",
                                    class: "px-3 py-1.5 text-sm text-gray-700 hover:bg-indigo-50 cursor-pointer",
                                    onmousedown: move |e| e.prevent_default(),
                                    onclick: move |_| {
                                        value.set(click_name.clone());
                                        show_dropdown.set(false);
                                        on_series_selected.call((click_name.clone(), next_num));
                                    },
                                    "{label}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
