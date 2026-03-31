use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::routes::books_page::BookSummary;

// ---------------------------------------------------------------------------
// Sort types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum SortField {
    DateAdded,
    Title,
    AuthorTitle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum SortDir {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SortOrder {
    pub field: SortField,
    pub dir: SortDir,
}

impl Default for SortOrder {
    fn default() -> Self {
        Self {
            field: SortField::DateAdded,
            dir: SortDir::Desc,
        }
    }
}

impl SortField {
    fn default_dir(self) -> SortDir {
        match self {
            Self::DateAdded => SortDir::Desc,
            Self::Title | Self::AuthorTitle => SortDir::Asc,
        }
    }
}

/// Global sort order — persists across route changes like `SEARCH_TEXT`.
pub(crate) static SORT_ORDER: GlobalSignal<SortOrder> = Signal::global(SortOrder::default);

// ---------------------------------------------------------------------------
// Conversion to core types
// ---------------------------------------------------------------------------

#[cfg(feature = "server")]
pub(crate) fn to_core_sort(sort: SortOrder) -> bb_core::book::BookSortOrder {
    bb_core::book::BookSortOrder {
        field: match sort.field {
            SortField::DateAdded => bb_core::book::BookSortField::DateAdded,
            SortField::Title => bb_core::book::BookSortField::Title,
            SortField::AuthorTitle => bb_core::book::BookSortField::AuthorTitle,
        },
        direction: match sort.dir {
            SortDir::Asc => bb_core::book::SortDirection::Asc,
            SortDir::Desc => bb_core::book::SortDirection::Desc,
        },
    }
}

// ---------------------------------------------------------------------------
// Server functions: persistence
// ---------------------------------------------------------------------------

#[cfg(feature = "server")]
use {
    crate::routes::server_helpers::{authenticated_user, to_server_err},
    crate::server::AuthSession,
    bb_core::CoreServices,
    std::sync::Arc,
};

const SORT_SETTING_KEY: &str = "book_sort_order";

#[get(
    "/api/v1/settings/sort",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn get_sort_preference() -> Result<Option<SortOrder>, ServerFnError> {
    let user_id = authenticated_user(&auth_session)?.id();

    let setting = core_services.user_setting_service.get(user_id, SORT_SETTING_KEY).await.map_err(to_server_err)?;

    Ok(setting.and_then(|s| serde_json::from_str(&s.value).ok()))
}

#[post(
    "/api/v1/settings/sort",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn set_sort_preference(sort: SortOrder) -> Result<(), ServerFnError> {
    let user_id = authenticated_user(&auth_session)?.id();

    let value = serde_json::to_string(&sort).map_err(to_server_err)?;

    core_services
        .user_setting_service
        .set(user_id, SORT_SETTING_KEY, &value)
        .await
        .map_err(to_server_err)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Client-side sorting (for manual shelves)
// ---------------------------------------------------------------------------

pub(crate) fn sort_books_client_side(mut books: Vec<BookSummary>, sort: SortOrder) -> Vec<BookSummary> {
    match sort.field {
        SortField::DateAdded => {
            books.sort_by(|a, b| {
                let cmp = a.created_at.cmp(&b.created_at);
                match sort.dir {
                    SortDir::Asc => cmp,
                    SortDir::Desc => cmp.reverse(),
                }
            });
        }
        SortField::Title => {
            books.sort_by(|a, b| {
                let cmp = a.title.to_lowercase().cmp(&b.title.to_lowercase());
                match sort.dir {
                    SortDir::Asc => cmp,
                    SortDir::Desc => cmp.reverse(),
                }
            });
        }
        SortField::AuthorTitle => {
            books.sort_by(|a, b| {
                let author_a = a.authors.first().map_or(String::new(), |a| a.name.to_lowercase());
                let author_b = b.authors.first().map_or(String::new(), |a| a.name.to_lowercase());
                let cmp = author_a.cmp(&author_b).then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
                match sort.dir {
                    SortDir::Asc => cmp,
                    SortDir::Desc => cmp.reverse(),
                }
            });
        }
    }
    books
}

// ---------------------------------------------------------------------------
// SortControl component
// ---------------------------------------------------------------------------

/// Segmented button group with 3 sort options: Date Added, Title, Author+Title.
#[component]
pub(crate) fn SortControl() -> Element {
    let sort = SORT_ORDER();

    let segments: [SortField; 3] = [SortField::DateAdded, SortField::Title, SortField::AuthorTitle];

    rsx! {
        div { class: "inline-flex items-center border border-gray-300 rounded-md overflow-hidden",
            for (idx, field) in segments.iter().enumerate() {
                {
                    let field = *field;
                    let is_active = sort.field == field;
                    let is_first = idx == 0;
                    let is_last = idx == segments.len() - 1;

                    let bg = if is_active {
                        "bg-indigo-600 text-white"
                    } else {
                        "bg-gray-50 text-gray-500 hover:bg-gray-100"
                    };
                    let border = if is_first {
                        ""
                    } else {
                        "border-l border-gray-300"
                    };
                    let rounding = match (is_first, is_last) {
                        (true, false) => "rounded-l-md",
                        (false, true) => "rounded-r-md",
                        _ => "",
                    };

                    let tooltip = if is_active {
                        tooltip_text(field, sort.dir)
                    } else {
                        tooltip_text(field, field.default_dir())
                    };

                    rsx! {
                        button {
                            r#type: "button",
                            class: "flex items-center justify-center gap-0.5 px-2 py-1.5 text-xs font-medium cursor-pointer select-none {bg} {border} {rounding}",
                            title: tooltip,
                            onclick: move |_| {
                                let new_sort = if is_active {
                                    SortOrder {
                                        field,
                                        dir: match sort.dir {
                                            SortDir::Asc => SortDir::Desc,
                                            SortDir::Desc => SortDir::Asc,
                                        },
                                    }
                                } else {
                                    SortOrder {
                                        field,
                                        dir: field.default_dir(),
                                    }
                                };
                                *SORT_ORDER.write() = new_sort;
                                spawn(async move {
                                    let _ = set_sort_preference(new_sort).await;
                                });
                            },
                            {sort_icon(field)},
                            if is_active {
                                {chevron_icon(sort.dir)}
                            }
                        }
                    }
                }
            }
        }
    }
}

fn tooltip_text(field: SortField, dir: SortDir) -> &'static str {
    match (field, dir) {
        (SortField::DateAdded, SortDir::Desc) => "Newest first",
        (SortField::DateAdded, SortDir::Asc) => "Oldest first",
        (SortField::Title, SortDir::Asc) => "Title A → Z",
        (SortField::Title, SortDir::Desc) => "Title Z → A",
        (SortField::AuthorTitle, SortDir::Asc) => "Author A → Z",
        (SortField::AuthorTitle, SortDir::Desc) => "Author Z → A",
    }
}

fn sort_icon(field: SortField) -> Element {
    match field {
        // Heroicons mini: calendar
        SortField::DateAdded => rsx! {
            svg {
                class: "w-4 h-4",
                xmlns: "http://www.w3.org/2000/svg",
                view_box: "0 0 20 20",
                fill: "currentColor",
                path {
                    fill_rule: "evenodd",
                    d: "M5.75 2a.75.75 0 01.75.75V4h7V2.75a.75.75 0 011.5 0V4h.25A2.75 2.75 0 0118 6.75v8.5A2.75 2.75 0 0115.25 18H4.75A2.75 2.75 0 012 15.25v-8.5A2.75 2.75 0 014.75 4H5V2.75A.75.75 0 015.75 2zm-1 5.5c-.69 0-1.25.56-1.25 1.25v6.5c0 .69.56 1.25 1.25 1.25h10.5c.69 0 1.25-.56 1.25-1.25v-6.5c0-.69-.56-1.25-1.25-1.25H4.75z",
                    clip_rule: "evenodd",
                }
            }
        },
        // Bold "A" text
        SortField::Title => rsx! {
            span { class: "text-sm font-bold leading-none", "A" }
        },
        // Heroicons mini: user
        SortField::AuthorTitle => rsx! {
            svg {
                class: "w-4 h-4",
                xmlns: "http://www.w3.org/2000/svg",
                view_box: "0 0 20 20",
                fill: "currentColor",
                path { d: "M10 8a3 3 0 100-6 3 3 0 000 6zM3.465 14.493a1.23 1.23 0 00.41 1.412A9.957 9.957 0 0010 18c2.31 0 4.438-.784 6.131-2.1.43-.333.604-.903.408-1.41a7.002 7.002 0 00-13.074.003z" }
            }
        },
    }
}

fn chevron_icon(dir: SortDir) -> Element {
    match dir {
        SortDir::Asc => rsx! {
            svg {
                class: "w-3 h-3",
                xmlns: "http://www.w3.org/2000/svg",
                view_box: "0 0 20 20",
                fill: "currentColor",
                path {
                    fill_rule: "evenodd",
                    d: "M14.77 12.79a.75.75 0 01-1.06-.02L10 8.832 6.29 12.77a.75.75 0 11-1.08-1.04l4.25-4.5a.75.75 0 011.08 0l4.25 4.5a.75.75 0 01-.02 1.06z",
                    clip_rule: "evenodd",
                }
            }
        },
        SortDir::Desc => rsx! {
            svg {
                class: "w-3 h-3",
                xmlns: "http://www.w3.org/2000/svg",
                view_box: "0 0 20 20",
                fill: "currentColor",
                path {
                    fill_rule: "evenodd",
                    d: "M5.23 7.21a.75.75 0 011.06.02L10 11.168l3.71-3.938a.75.75 0 111.08 1.04l-4.25 4.5a.75.75 0 01-1.08 0l-4.25-4.5a.75.75 0 01.02-1.06z",
                    clip_rule: "evenodd",
                }
            }
        },
    }
}
