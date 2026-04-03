use dioxus::prelude::*;
#[cfg(feature = "server")]
use {
    crate::routes::server_helpers::{require_capability, to_server_err},
    crate::server::{AuthSession, AuthUser, BackendSessionPool},
    axum::http::Method,
    axum_session_auth::{Auth, Rights},
    bb_core::{CoreServices, types::Capability, user::UserId},
    std::sync::Arc,
};

use crate::components::{IncomingRefresh, JobsRefresh};

#[post("/api/v1/incoming/trigger_scan", auth_session: axum::Extension<AuthSession>, core_services: axum::Extension<Arc<CoreServices>>)]
async fn trigger_bookdrop_scan() -> Result<(), ServerFnError> {
    require_capability(&auth_session, Capability::ApproveImports, Method::POST).await?;
    core_services.import_job_service.trigger_scan();
    Ok(())
}

use super::{
    SEARCH_TEXT,
    search::{PLACEHOLDER_TIPS, apply_completion, compute_completion, next_cycle_input},
};
use crate::Route;

#[get("/api/v1/incoming/pending_count", auth_session: axum::Extension<AuthSession>, core_services: axum::Extension<Arc<CoreServices>>)]
async fn get_pending_count() -> Result<Option<u32>, ServerFnError> {
    let current_user = auth_session.current_user.clone().unwrap_or_default();
    let has_permission = Auth::<AuthUser, UserId, BackendSessionPool>::build([Method::GET], true)
        .requires(Rights::any([Rights::permission(Capability::ApproveImports.as_str())]))
        .validate(&current_user, &Method::GET, None)
        .await;
    if !has_permission {
        return Ok(None);
    }
    let jobs = core_services.import_job_service.list_needs_review(None, None).await.map_err(to_server_err)?;
    #[expect(clippy::cast_possible_truncation, reason = "pending review count; will never approach u32::MAX")]
    let count = jobs.len() as u32;
    Ok(Some(count))
}

#[get("/api/v1/jobs/queue_count", core_services: axum::Extension<Arc<CoreServices>>)]
async fn get_job_queue_count() -> Result<u64, ServerFnError> {
    core_services.job_service.count_all_pending().await.map_err(to_server_err)
}

#[get("/api/v1/user/is_admin", auth_session: axum::Extension<AuthSession>)]
async fn get_is_admin() -> Result<bool, ServerFnError> {
    let Some(user) = auth_session.current_user.as_ref().filter(|u| !u.username.is_empty()) else {
        return Ok(false);
    };
    let is_super_admin = user.permissions.contains("SuperAdmin");
    Ok(is_super_admin || user.permissions.contains("Admin"))
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct CollectionStats {
    pub books: u64,
    pub authors: u64,
}

#[get(
    "/api/v1/library/stats",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
async fn get_collection_stats() -> Result<CollectionStats, ServerFnError> {
    auth_session
        .current_user
        .as_ref()
        .filter(|u| !u.username.is_empty())
        .ok_or_else(|| ServerFnError::new("Not authenticated"))?;

    let stats = core_services.collection_service.collection_stats().await.map_err(to_server_err)?;

    Ok(CollectionStats {
        books: stats.books,
        authors: stats.authors,
    })
}

#[put("/api/v1/logout", auth_session: axum::Extension<AuthSession>)]
async fn logout() -> Result<(), ServerFnError> {
    auth_session.logout_user();
    Ok(())
}

// ── Badge / button sub-components
// ─────────────────────────────────────────────────────────────────────────────

/// Renders the Incoming nav link with its pending-count badge.
///
/// Isolated into its own component so that `use_server_future` can suspend
/// with `?` here without affecting the rest of `NavBar`. `NavBar` wraps this
/// in a `SuspenseBoundary` so the link simply stays hidden until the count
/// resolves, rather than leaving it permanently absent after a page refresh.
#[component]
fn IncomingBadge() -> Element {
    let mut incoming_refresh = use_context::<IncomingRefresh>();
    let pending_count = use_server_future(move || {
        let _rev = (incoming_refresh.0)();
        get_pending_count()
    })?;
    let mut scanning = use_signal(|| false);
    let route = use_route::<Route>();

    let count_opt = pending_count().and_then(|r: Result<Option<u32>, ServerFnError>| r.ok()).flatten();

    rsx! {
        {count_opt.map(|count| {
            let on_incoming_page = route == Route::IncomingPage {};
            rsx! {
                div { class: "flex items-center gap-2",
                    Link { to: Route::IncomingPage {}, class: "relative text-sm hover:text-indigo-200 flex items-center gap-1.5",
                        "Incoming"
                        if count > 0 {
                            span {
                                class: "inline-flex items-center justify-center min-w-[1.1rem] h-[1.1rem] px-1 rounded-full bg-red-500 text-white text-[0.6rem] font-bold leading-none",
                                "{count}"
                            }
                        }
                    }
                    if on_incoming_page {
                        button {
                            class: "flex items-center text-indigo-200 hover:text-white disabled:opacity-40 disabled:cursor-not-allowed cursor-pointer",
                            title: "Scan bookdrop now",
                            disabled: scanning(),
                            onclick: move |_| {
                                spawn(async move {
                                    scanning.set(true);
                                    let _ = trigger_bookdrop_scan().await;
                                    *incoming_refresh.0.write() += 1;
                                    scanning.set(false);
                                });
                            },
                            svg {
                                class: if scanning() { "w-3.5 h-3.5 animate-spin" } else { "w-3.5 h-3.5" },
                                xmlns: "http://www.w3.org/2000/svg",
                                fill: "none",
                                view_box: "0 0 24 24",
                                stroke_width: "2",
                                stroke: "currentColor",
                                path {
                                    stroke_linecap: "round",
                                    stroke_linejoin: "round",
                                    d: "M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.993 0 3.181 3.183a8.25 8.25 0 0 0 13.803-3.7M4.031 9.865a8.25 8.25 0 0 1 13.803-3.7l3.181 3.182m0-4.991v4.99",
                                }
                            }
                        }
                    }
                }
            }
        })}
    }
}

/// Shows a subtle count badge when background jobs are in flight.
/// Hidden when the count is zero. Uses the same SuspenseBoundary pattern
/// as `IncomingBadge`.
#[component]
fn JobQueueBadge() -> Element {
    let jobs_refresh = use_context::<JobsRefresh>();
    let pending_count = use_server_future(move || {
        let _rev = (jobs_refresh.0)();
        get_job_queue_count()
    })?;
    let count = pending_count().and_then(|r: Result<u64, ServerFnError>| r.ok()).unwrap_or(0);

    if count == 0 {
        return rsx! {};
    }

    rsx! {
        span { class: "inline-flex items-center gap-1 text-sm text-indigo-300",
            svg {
                class: "w-3.5 h-3.5 animate-spin",
                xmlns: "http://www.w3.org/2000/svg",
                fill: "none",
                view_box: "0 0 24 24",
                circle {
                    class: "opacity-25",
                    cx: "12",
                    cy: "12",
                    r: "10",
                    stroke: "currentColor",
                    stroke_width: "4",
                }
                path {
                    class: "opacity-75",
                    fill: "currentColor",
                    d: "M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z",
                }
            }
            span { class: "inline-flex items-center justify-center min-w-[1.1rem] h-[1.1rem] px-1 rounded-full bg-gray-500 text-white text-[0.6rem] font-bold leading-none",
                "{count}"
            }
        }
    }
}

/// Settings gear icon — only rendered for admin / super-admin users.
///
/// Uses the same `SuspenseBoundary` isolation pattern as `IncomingBadge` so
/// the icon is simply absent for non-admins without affecting NavBar layout.
#[component]
fn AdminSettingsButton() -> Element {
    let navigator = use_navigator();
    let is_admin = use_server_future(get_is_admin)?;
    let admin = is_admin().and_then(|r: Result<bool, ServerFnError>| r.ok()).unwrap_or(false);

    if !admin {
        return rsx! {};
    }

    rsx! {
        button {
            class: "flex items-center hover:text-indigo-200 ml-4 cursor-pointer",
            title: "Settings",
            onclick: move |_| { navigator.push(Route::SettingsPage {}); },
            svg {
                class: "w-5 h-5",
                fill: "none",
                view_box: "0 0 24 24",
                stroke_width: "1.5",
                stroke: "currentColor",
                path {
                    stroke_linecap: "round",
                    stroke_linejoin: "round",
                    d: "M9.594 3.94c.09-.542.56-.94 1.11-.94h2.593c.55 0 1.02.398 1.11.94l.213 1.281c.063.374.313.686.645.87.074.04.147.083.22.127.325.196.72.257 1.075.124l1.217-.456a1.125 1.125 0 0 1 1.37.49l1.296 2.247a1.125 1.125 0 0 1-.26 1.431l-1.003.827c-.293.241-.438.613-.43.992a7.723 7.723 0 0 1 0 .255c-.008.378.137.75.43.991l1.004.827c.424.35.534.955.26 1.43l-1.298 2.247a1.125 1.125 0 0 1-1.369.491l-1.217-.456c-.355-.133-.75-.072-1.076.124a6.47 6.47 0 0 1-.22.128c-.331.183-.581.495-.644.869l-.213 1.281c-.09.543-.56.94-1.11.94h-2.594c-.55 0-1.019-.398-1.11-.94l-.213-1.281c-.062-.374-.312-.686-.644-.87a6.52 6.52 0 0 1-.22-.127c-.325-.196-.72-.257-1.076-.124l-1.217.456a1.125 1.125 0 0 1-1.369-.49l-1.297-2.247a1.125 1.125 0 0 1 .26-1.431l1.004-.827c.292-.24.437-.613.43-.991a6.932 6.932 0 0 1 0-.255c.007-.38-.138-.751-.43-.992l-1.004-.827a1.125 1.125 0 0 1-.26-1.43l1.297-2.247a1.125 1.125 0 0 1 1.37-.491l1.216.456c.356.133.751.072 1.076-.124.072-.044.146-.086.22-.128.332-.183.582-.495.644-.869l.214-1.28Z",
                }
                path {
                    stroke_linecap: "round",
                    stroke_linejoin: "round",
                    d: "M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z",
                }
            }
        }
    }
}

// ── About modal
// ─────────────────────────────────────────────────────────────────────────────

/// Modal showing app version and library statistics.
///
/// Stats are fetched when the modal mounts and fill in asynchronously;
/// the modal itself appears immediately without waiting for the response.
#[component]
fn AboutModal(on_close: EventHandler<()>) -> Element {
    let stats_res = use_server_future(get_collection_stats);
    let stats = match stats_res {
        Ok(ref r) => r().and_then(|r: Result<CollectionStats, ServerFnError>| r.ok()),
        Err(_) => None,
    };

    rsx! {
        div {
            class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
            tabindex: -1,
            onmounted: move |e| async move { let _ = e.set_focus(true).await; },
            onclick: move |_| on_close(()),
            onkeydown: move |e| { if e.key() == Key::Escape { on_close(()); } },
            div {
                class: "bg-white rounded-xl shadow-xl w-full max-w-md mx-4",
                onclick: |e| e.stop_propagation(),
                // Header
                div { class: "flex items-center justify-between px-6 pt-5 pb-2",
                    h2 { class: "text-lg font-semibold text-gray-900", "About" }
                    button {
                        class: "text-gray-400 hover:text-gray-600 cursor-pointer",
                        onclick: move |_| on_close(()),
                        svg {
                            class: "w-5 h-5",
                            fill: "none",
                            view_box: "0 0 24 24",
                            stroke_width: "1.5",
                            stroke: "currentColor",
                            path {
                                stroke_linecap: "round",
                                stroke_linejoin: "round",
                                d: "M6 18 18 6M6 6l12 12",
                            }
                        }
                    }
                }
                // Body
                div { class: "px-6 pb-6",
                    img {
                        src: asset!("/assets/BookBoss-Banner.png"),
                        alt: "BookBoss",
                        class: "w-full mb-2",
                    }
                    p { class: "text-sm text-gray-500 mb-6 text-center",
                        { format!("Version: {}", clap::crate_version!()) }
                    }
                    h3 { class: "text-sm font-semibold text-gray-900 mb-3", "Library Statistics" }
                    dl { class: "divide-y divide-gray-100 rounded-lg border border-gray-200 bg-white",
                        AboutStatRow {
                            label: "Books",
                            value: stats.as_ref().map(|s| s.books.to_string()),
                        }
                        AboutStatRow {
                            label: "Authors",
                            value: stats.as_ref().map(|s| s.authors.to_string()),
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn AboutStatRow(label: &'static str, value: Option<String>) -> Element {
    rsx! {
        div { class: "flex justify-between px-4 py-3",
            dt { class: "text-sm text-gray-500", { label } }
            dd { class: "text-sm font-medium text-gray-900",
                { value.as_deref().unwrap_or("—") }
            }
        }
    }
}

// ── NavBar
// ─────────────────────────────────────────────────────────────────────────────

#[component]
pub(crate) fn NavBar() -> Element {
    let navigator = use_navigator();
    let mut user_menu_open = use_signal(|| false);
    let mut show_about = use_signal(|| false);
    let route = use_route::<Route>();

    let mut focused = use_signal(|| false);
    let mut help_open = use_signal(|| false);
    let mut hint_seen = use_signal(|| false);
    let mut tip_index = use_signal(|| 0usize);
    let mut completion = use_signal(String::new); // ghost-text suffix (e.g., "hor:")
    let mut cycle_prefix = use_signal(String::new); // prefix being cycled (e.g., "s")
    let mut cycle_idx = use_signal(|| 0usize); // position in cycle

    use_hook(move || {
        spawn(async move {
            if let Ok(val) = document::eval("return window.localStorage.getItem('search_hint_seen')").await {
                if !val.is_null() {
                    hint_seen.set(true);
                }
            }
        });
    });

    use_hook(move || {
        spawn(async move {
            loop {
                let mut timer = document::eval("setTimeout(() => dioxus.send(true), 3000)");
                let _ = timer.recv::<bool>().await;
                if *focused.peek() && SEARCH_TEXT.peek().is_empty() {
                    tip_index.with_mut(|i| *i = (*i + 1) % PLACEHOLDER_TIPS.len());
                }
            }
        });
    });

    let show_hint = use_memo(move || {
        let empty = SEARCH_TEXT().is_empty();
        (focused() && empty && !hint_seen()) || (help_open() && empty)
    });

    let search_active = matches!(
        route,
        Route::BooksPage | Route::ShelfPage { .. } | Route::AuthorDetailPage { .. } | Route::SeriesDetailPage { .. } | Route::AuthorsPage | Route::SeriesPage
    );

    let search_placeholder: &str = if focused() && SEARCH_TEXT().is_empty() {
        PLACEHOLDER_TIPS[tip_index() % PLACEHOLDER_TIPS.len()]
    } else {
        match route {
            Route::AuthorsPage => "Search authors…",
            Route::SeriesPage => "Search series…",
            _ => "Search books…",
        }
    };

    let on_logout = move |_| {
        user_menu_open.set(false);
        spawn(async move {
            let _ = logout().await;
            navigator.push(Route::LandingPage {});
        });
    };

    rsx! {
        nav { class: "relative bg-indigo-700 text-white px-6 py-3 flex items-center shadow-sm",
            div { class: "flex items-center gap-6 shrink-0",
                button {
                    class: "flex items-center cursor-pointer hover:opacity-80",
                    title: "About",
                    onclick: move |_| show_about.set(true),
                    img {
                        src: asset!("/assets/BookBoss-Title.png"),
                        alt: "BookBoss",
                        class: "h-8 w-auto",
                    }
                }
                Link { to: Route::BooksPage {}, class: "text-sm hover:text-indigo-200",
                    onclick: move |_| *SEARCH_TEXT.write() = String::new(),
                    "Library"
                }
                Link { to: Route::AuthorsPage {}, class: "text-sm hover:text-indigo-200",
                    onclick: move |_| *SEARCH_TEXT.write() = String::new(),
                    "Authors"
                }
                Link { to: Route::SeriesPage {}, class: "text-sm hover:text-indigo-200",
                    onclick: move |_| *SEARCH_TEXT.write() = String::new(),
                    "Series"
                }
                SuspenseBoundary {
                    fallback: |_| rsx! {},
                    IncomingBadge {}
                }
                SuspenseBoundary {
                    fallback: |_| rsx! {},
                    JobQueueBadge {}
                }
            }
            div { class: "absolute left-1/2 -translate-x-1/2 w-full max-w-md px-4",
                if search_active {
                    div { class: "flex items-center gap-2",
                        // Input column — flex-1 takes all space; relative for hint strip positioning
                        div { class: "relative flex-1",
                            // ── Input container ──────────────────────────────────────────
                            div { class: "relative w-full bg-white/90 focus-within:bg-white rounded focus-within:ring-2 focus-within:ring-indigo-300",
                                // Search icon
                                svg {
                                    class: "absolute left-2.5 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400 pointer-events-none",
                                    xmlns: "http://www.w3.org/2000/svg",
                                    fill: "none",
                                    view_box: "0 0 24 24",
                                    stroke_width: "2",
                                    stroke: "currentColor",
                                    path {
                                        stroke_linecap: "round",
                                        stroke_linejoin: "round",
                                        d: "m21 21-5.197-5.197m0 0A7.5 7.5 0 1 0 5.196 5.196a7.5 7.5 0 0 0 10.607 10.607Z",
                                    }
                                }
                                if !completion().is_empty() {
                                    span {
                                        class: "absolute inset-0 flex items-center pl-9 pr-8 text-sm pointer-events-none overflow-hidden select-none",
                                        "aria-hidden": "true",
                                        span {
                                            style: "color: transparent; white-space: pre;",
                                            "{SEARCH_TEXT()}"
                                        }
                                        span {
                                            class: "text-gray-400",
                                            style: "white-space: pre;",
                                            "{completion()}"
                                        }
                                    }
                                }
                                input {
                                    class: "relative w-full pl-9 pr-8 py-1.5 text-sm text-gray-900 bg-transparent placeholder-gray-400 outline-none",
                                    r#type: "text",
                                    placeholder: "{search_placeholder}",
                                    value: SEARCH_TEXT(),
                                    onfocus: move |_| {
                                        focused.set(true);
                                    },
                                    onblur: move |_| {
                                        focused.set(false);
                                        help_open.set(false);
                                        if !hint_seen() {
                                            hint_seen.set(true);
                                            spawn(async move {
                                                let _ = document::eval(
                                                    "window.localStorage.setItem('search_hint_seen','1')",
                                                )
                                                .await;
                                            });
                                        }
                                    },
                                    oninput: move |e| {
                                        let val = e.value();
                                        let new_completion = compute_completion(&val, 0);
                                        *SEARCH_TEXT.write() = val;
                                        help_open.set(false);
                                        if !hint_seen() {
                                            hint_seen.set(true);
                                            spawn(async move {
                                                let _ = document::eval(
                                                    "window.localStorage.setItem('search_hint_seen','1')",
                                                )
                                                .await;
                                            });
                                        }
                                        // Recompute completion from scratch on every input change
                                        cycle_prefix.set(String::new());
                                        cycle_idx.set(0);
                                        completion.set(new_completion);
                                    },
                                    onkeydown: move |e: KeyboardEvent| {
                                        match e.key() {
                                            Key::Tab => {
                                                let current_completion = completion();
                                                if !cycle_prefix().is_empty() && !current_completion.is_empty() {
                                                    // Already cycling (e.g. status:r → status:read) and a ghost
                                                    // suffix is showing — apply it without resetting the cycle
                                                    // origin, then advance the counter.
                                                    e.prevent_default();
                                                    let applied = apply_completion(&SEARCH_TEXT(), &current_completion);
                                                    (*SEARCH_TEXT.write()).clone_from(&applied);
                                                    let new_idx = cycle_idx() + 1;
                                                    cycle_idx.set(new_idx);
                                                    completion.set(String::new());
                                                } else if !current_completion.is_empty() {
                                                    // First Tab: apply completion and enter cycling mode.
                                                    e.prevent_default();
                                                    let current_input = SEARCH_TEXT();
                                                    // Capture the token being completed as the cycle origin.
                                                    let prefix = current_input
                                                        .split_whitespace()
                                                        .last()
                                                        .unwrap_or("")
                                                        .to_string();
                                                    let applied = apply_completion(&current_input, &current_completion);
                                                    (*SEARCH_TEXT.write()).clone_from(&applied);
                                                    cycle_prefix.set(prefix);
                                                    let new_idx = cycle_idx() + 1;
                                                    cycle_idx.set(new_idx);
                                                    completion.set(String::new());
                                                } else if !cycle_prefix().is_empty() {
                                                    // Subsequent Tab with no ghost (current text is an exact
                                                    // match for one candidate) — cycle to the next match.
                                                    e.prevent_default();
                                                    let prefix = cycle_prefix();
                                                    let current_idx = cycle_idx();
                                                    let new_idx = current_idx + 1;
                                                    cycle_idx.set(new_idx);
                                                    // Pass current_idx (pre-increment): next_cycle_input
                                                    // selects the slot to transition *into*.
                                                    if let Some(cycled) = next_cycle_input(&SEARCH_TEXT(), &prefix, current_idx) {
                                                        *SEARCH_TEXT.write() = cycled;
                                                        completion.set(String::new());
                                                    }
                                                }
                                            }
                                            Key::Escape => {
                                                if !completion().is_empty() || !cycle_prefix().is_empty() {
                                                    completion.set(String::new());
                                                    cycle_prefix.set(String::new());
                                                    cycle_idx.set(0);
                                                } else {
                                                    *SEARCH_TEXT.write() = String::new();
                                                }
                                            }
                                            _ => {
                                                // Any key other than Tab resets completion cycling
                                                if !completion().is_empty() || !cycle_prefix().is_empty() {
                                                    completion.set(String::new());
                                                    cycle_prefix.set(String::new());
                                                    cycle_idx.set(0);
                                                }
                                            }
                                        }
                                    },
                                }
                                // Clear button
                                if !SEARCH_TEXT().is_empty() {
                                    button {
                                        class: "absolute right-2 top-1/2 -translate-y-1/2 text-gray-400 hover:text-gray-600 cursor-pointer",
                                        onclick: move |_| *SEARCH_TEXT.write() = String::new(),
                                        svg {
                                            class: "w-4 h-4",
                                            xmlns: "http://www.w3.org/2000/svg",
                                            fill: "none",
                                            view_box: "0 0 24 24",
                                            stroke_width: "2",
                                            stroke: "currentColor",
                                            path {
                                                stroke_linecap: "round",
                                                stroke_linejoin: "round",
                                                d: "M6 18 18 6M6 6l12 12",
                                            }
                                        }
                                    }
                                }
                            }
                            // ── Hint strip (absolute dropdown below the input) ────────────
                            if show_hint() {
                                div {
                                    class: "absolute top-full left-0 right-0 mt-1 bg-blue-50 border border-blue-200 rounded-md px-3 py-2 text-xs text-blue-700 z-50 shadow-sm leading-relaxed",
                                    span { class: "font-semibold", "field:value" }
                                    " to narrow results — "
                                    for field in ["author:", "series:", "genre:", "tag:", "status:", "title:"] {
                                        code { class: "inline-block bg-blue-100 rounded px-1 mr-1 font-mono", "{field}" }
                                    }
                                    " · Quote multi-word values: "
                                    code { class: "bg-blue-100 rounded px-1 font-mono", "author:\"Brad Thor\"" }
                                }
                            }
                        }
                        // ── ? button ──────────────────────────────────────────────────────
                        button {
                            class: "shrink-0 text-xs px-2 py-0.5 rounded-full bg-indigo-500 hover:bg-indigo-400 text-white font-medium cursor-pointer leading-tight",
                            title: "Search help",
                            onclick: move |_| help_open.set(!help_open()),
                            "?"
                        }
                    }
                }
            }
            div { class: "flex items-center gap-4 shrink-0 ml-auto",
                SuspenseBoundary {
                    fallback: |_| rsx! {},
                    AdminSettingsButton {}
                }
                div { class: "relative",
                    button {
                        class: "flex items-center hover:text-indigo-200",
                        title: "User",
                        onclick: move |_| user_menu_open.toggle(),
                        svg {
                            class: "w-5 h-5",
                            fill: "none",
                            view_box: "0 0 24 24",
                            stroke_width: "1.5",
                            stroke: "currentColor",
                            path {
                                stroke_linecap: "round",
                                stroke_linejoin: "round",
                                d: "M15.75 6a3.75 3.75 0 1 1-7.5 0 3.75 3.75 0 0 1 7.5 0ZM4.501 20.118a7.5 7.5 0 0 1 14.998 0A17.933 17.933 0 0 1 12 21.75c-2.676 0-5.216-.584-7.499-1.632Z",
                            }
                        }
                    }
                    if user_menu_open() {
                        div {
                            class: "fixed inset-0 z-40",
                            onclick: move |_| user_menu_open.set(false),
                        }
                        div { class: "absolute right-0 top-full mt-1 w-36 bg-white rounded-lg shadow-lg py-1 z-50",
                            button {
                                class: "w-full text-left px-4 py-2 text-sm text-gray-700 hover:bg-gray-100",
                                onclick: move |_| {
                                    user_menu_open.set(false);
                                    navigator.push(Route::ProfilePage {});
                                },
                                "Profile"
                            }
                            button {
                                class: "w-full text-left px-4 py-2 text-sm text-gray-700 hover:bg-gray-100",
                                onclick: on_logout,
                                "Logout"
                            }
                        }
                    }
                }
            }
        }
        if show_about() {
            AboutModal { on_close: move |()| show_about.set(false) }
        }
    }
}
