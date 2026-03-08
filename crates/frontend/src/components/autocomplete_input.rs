use dioxus::prelude::*;

use super::chip_input::word_match;

/// Single-value text input with a live pick-list dropdown for series names.
///
/// - Typing 1+ characters filters `options` by word-order-independent matching.
/// - Clicking a suggestion sets the input value, closes the dropdown, and fires
///   `on_series_selected` with `(series_name, suggested_next_number)`.
/// - The parent may use `on_blur` (fired on focus-out with the current value)
///   to set a default book number when the user types a brand-new series name.
#[component]
pub(crate) fn AutocompleteInput(
    mut value: Signal<String>,
    options: Vec<(String, u32)>,
    on_series_selected: EventHandler<(String, u32)>,
    on_blur: EventHandler<String>,
) -> Element {
    let mut show_dropdown = use_signal(|| false);

    let query = value.read().clone();
    let filtered: Vec<(String, u32)> = if query.is_empty() {
        vec![]
    } else {
        options.iter().filter(|(name, _)| word_match(name, &query)).take(8).cloned().collect()
    };

    rsx! {
        div { class: "relative flex-1",
            input {
                class: "w-full border border-gray-300 rounded px-2 py-1 text-sm focus:outline-none focus:ring-1 focus:ring-indigo-400",
                value: "{value}",
                placeholder: "Series name",
                oninput: move |e| {
                    value.set(e.value());
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
            if *show_dropdown.read() && !filtered.is_empty() {
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
