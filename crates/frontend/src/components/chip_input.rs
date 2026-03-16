use dioxus::prelude::*;

/// Returns true if every whitespace-separated word in `query` appears
/// (case-insensitive) anywhere in `candidate`.
pub(crate) fn word_match(candidate: &str, query: &str) -> bool {
    let lower = candidate.to_lowercase();
    query.split_whitespace().all(|w| lower.contains(&w.to_lowercase()))
}

/// Multi-value chip input with a live pick-list dropdown.
///
/// - Each selected value renders as a removable chip with an ✕ button.
/// - Typing 1+ characters filters `options` and shows a dropdown (up to 8
///   items). The filter is word-order-independent.
/// - Press **Enter** or click a dropdown item to add a chip.
/// - Press **Backspace** on an empty input to remove the last chip.
/// - Chips whose value is not found in `options` (case-insensitive) display a
///   **new** badge in green, giving the user a visual cue that the entry will
///   be created in the database on save.
/// - Focusing an empty input immediately shows up to 8 options, signalling that
///   a pick-list is available even before any typing.
/// - `max_chips`: when set, the text input is hidden once the limit is reached,
///   preventing additional entries (useful for single-value fields like
///   Publisher).
#[component]
pub(crate) fn ChipInput(mut values: Signal<Vec<String>>, options: Vec<String>, placeholder: String, #[props(default)] max_chips: Option<usize>) -> Element {
    let mut input_text = use_signal(String::new);
    let mut show_dropdown = use_signal(|| false);

    let query = input_text.read().clone();
    let current = values.read().clone();
    let at_limit = max_chips.is_some_and(|max| current.len() >= max);

    let show = *show_dropdown.read();
    let filtered: Vec<String> = if query.is_empty() {
        if show {
            options
                .iter()
                .filter(|opt| !current.iter().any(|v| v.eq_ignore_ascii_case(opt)))
                .take(8)
                .cloned()
                .collect()
        } else {
            vec![]
        }
    } else {
        options
            .iter()
            .filter(|opt| word_match(opt, &query) && !current.iter().any(|v| v.eq_ignore_ascii_case(opt)))
            .take(8)
            .cloned()
            .collect()
    };

    let ph = if current.is_empty() { placeholder.as_str() } else { "" };

    rsx! {
        div { class: "relative",
            // ── Chip container ─────────────────────────────────────────────────
            div { class: "flex flex-wrap gap-1 items-center border border-gray-300 rounded px-2 py-1 min-h-[34px] focus-within:ring-1 focus-within:ring-indigo-400",
                for (i, chip) in current.iter().enumerate() {
                    {
                        let is_new = !options.iter().any(|o| o.eq_ignore_ascii_case(chip));
                        let label = chip.clone();
                        let chip_class = if is_new {
                            "inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs bg-green-100 text-green-800 border border-green-300"
                        } else {
                            "inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs bg-gray-100 text-gray-700 border border-gray-300"
                        };
                        rsx! {
                            span { key: "{i}", class: "{chip_class}",
                                "{label}"
                                if is_new {
                                    span { class: "font-semibold ml-0.5 text-green-700", "new" }
                                }
                                button {
                                    r#type: "button",
                                    class: "ml-0.5 text-gray-400 hover:text-gray-700 cursor-pointer leading-none",
                                    onclick: move |_| {
                                        values.write().remove(i);
                                    },
                                    "×"
                                }
                            }
                        }
                    }
                }
                if !at_limit {
                input {
                    class: "flex-1 min-w-[120px] text-sm outline-none bg-transparent py-0.5",
                    value: "{input_text}",
                    placeholder: "{ph}",
                    oninput: move |e| {
                        let v = e.value();
                        let nonempty = !v.is_empty();
                        input_text.set(v);
                        show_dropdown.set(nonempty);
                    },
                    onkeydown: move |e| {
                        match e.key() {
                            Key::Enter => {
                                e.prevent_default();
                                let text = input_text.read().trim().to_string();
                                if !text.is_empty() {
                                    let mut v = values.write();
                                    if !v.iter().any(|x| x.eq_ignore_ascii_case(&text)) {
                                        v.push(text);
                                    }
                                    drop(v);
                                    input_text.set(String::new());
                                    show_dropdown.set(false);
                                }
                            }
                            Key::Backspace if input_text.read().is_empty() => {
                                let mut v = values.write();
                                if !v.is_empty() {
                                    v.pop();
                                }
                            }
                            Key::Escape => {
                                input_text.set(String::new());
                                show_dropdown.set(false);
                            }
                            _ => {}
                        }
                    },
                    onfocus: move |_| {
                        show_dropdown.set(true);
                    },
                    onfocusout: move |_| {
                        show_dropdown.set(false);
                    },
                }
                } // end if !at_limit
            }
            // ── Dropdown ───────────────────────────────────────────────────────
            if *show_dropdown.read() && !filtered.is_empty() {
                div { class: "absolute left-0 right-0 top-full mt-1 bg-white border border-gray-200 rounded shadow-lg z-50 max-h-48 overflow-y-auto",
                    for option in filtered {
                        {
                            let label = option.clone();
                            let click_val = option;
                            rsx! {
                                div {
                                    key: "{label}",
                                    class: "px-3 py-1.5 text-sm text-gray-700 hover:bg-indigo-50 cursor-pointer",
                                    onmousedown: move |e| e.prevent_default(),
                                    onclick: move |_| {
                                        let name = click_val.trim().to_string();
                                        if !name.is_empty() {
                                            let mut v = values.write();
                                            if !v.iter().any(|x| x.eq_ignore_ascii_case(&name)) {
                                                v.push(name);
                                            }
                                        }
                                        input_text.set(String::new());
                                        show_dropdown.set(false);
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
