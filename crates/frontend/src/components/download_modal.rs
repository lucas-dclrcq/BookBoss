use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::components::{IncomingRefresh, JobsRefresh};

/// Whether the download search modal is open.
///
/// A global so the modal can be rendered by `NavBar` — which never unmounts —
/// rather than inside the Incoming page. The Incoming page reloads via
/// `use_server_future(...)?`, which re-suspends on every refresh (SSE
/// job/incoming events, frequent while a download runs). That suspension tears
/// down the whole page subtree, so a modal hosted there would close mid-use.
pub(crate) static DOWNLOAD_MODAL_OPEN: GlobalSignal<bool> = Signal::global(|| false);

/// A single Anna's Archive search result surfaced to the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DownloadResult {
    pub external_id: String,
    pub title: String,
    pub authors: String,
    pub language: Option<String>,
    pub size: Option<String>,
}

/// Outcome of enqueuing a download.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) enum DownloadOutcome {
    Queued,
    Error(String),
}

#[cfg(feature = "server")]
use {
    crate::routes::server_helpers::{require_capability, to_server_err},
    crate::server::{AuthSession, AuthUser, BackendSessionPool},
    axum::http::Method,
    axum_session_auth::{Auth, Rights},
    bb_core::{CoreServices, download::AnnasDownloadPayload, jobs::JobServiceExt, types::Capability, user::UserId},
    std::sync::Arc,
};

/// Extract a bare MD5 hash from a raw hash or an Anna's Archive `/md5/{hash}`
/// URL. Returns `None` when the input isn't a valid 32-char hex hash.
#[cfg(feature = "server")]
fn extract_md5(input: &str) -> Option<String> {
    let input = input.trim();
    let candidate = input.find("/md5/").map_or(input, |idx| &input[idx + "/md5/".len()..]);
    let candidate = candidate.split(['/', '?', '#']).next().unwrap_or_default().trim();
    if candidate.len() == 32 && candidate.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(candidate.to_lowercase())
    } else {
        None
    }
}

/// True when the current user may download *and* the feature is configured.
#[get("/api/v1/download/availability", auth_session: axum::Extension<AuthSession>, core_services: axum::Extension<Arc<CoreServices>>)]
pub(crate) async fn get_download_availability() -> Result<bool, ServerFnError> {
    if core_services.download_source_service.provider().is_none() {
        return Ok(false);
    }
    let current_user = auth_session.current_user.clone().unwrap_or_default();
    let allowed = Auth::<AuthUser, UserId, BackendSessionPool>::build([Method::GET], true)
        .requires(Rights::any([Rights::permission(Capability::DownloadBook.as_str())]))
        .validate(&current_user, &Method::GET, None)
        .await;
    Ok(allowed)
}

#[post("/api/v1/download/search", auth_session: axum::Extension<AuthSession>, core_services: axum::Extension<Arc<CoreServices>>)]
async fn search_downloads(query: String, language: String) -> Result<Vec<DownloadResult>, ServerFnError> {
    require_capability(&auth_session, Capability::DownloadBook, Method::POST).await?;

    let provider = core_services
        .download_source_service
        .provider()
        .ok_or_else(|| ServerFnError::new("Download feature is not enabled"))?;

    let language = language.trim();
    let lang = (!language.is_empty()).then_some(language);
    let candidates = provider.search(query.trim(), lang).await.map_err(to_server_err)?;

    Ok(candidates
        .into_iter()
        .map(|c| DownloadResult {
            external_id: c.external_id,
            title: c.title,
            authors: c.authors,
            language: c.language,
            size: c.size,
        })
        .collect())
}

#[post("/api/v1/download/start", auth_session: axum::Extension<AuthSession>, core_services: axum::Extension<Arc<CoreServices>>)]
async fn start_download(external_id: String, title: String, authors: String, language: String) -> Result<DownloadOutcome, ServerFnError> {
    require_capability(&auth_session, Capability::DownloadBook, Method::POST).await?;

    if core_services.download_source_service.provider().is_none() {
        return Ok(DownloadOutcome::Error("Download feature is not enabled".into()));
    }

    let Some(external_id) = extract_md5(&external_id) else {
        return Ok(DownloadOutcome::Error("Not a valid MD5 hash or Anna's Archive link".into()));
    };

    let payload = AnnasDownloadPayload {
        external_id,
        title: non_empty(title),
        authors: non_empty(authors),
        language: non_empty(language),
    };

    match core_services.job_service.enqueue(&payload).await {
        Ok(()) => Ok(DownloadOutcome::Queued),
        Err(e) => Ok(DownloadOutcome::Error(e.to_string())),
    }
}

#[cfg(feature = "server")]
fn non_empty(s: String) -> Option<String> {
    (!s.trim().is_empty()).then_some(s)
}

/// Header button that opens the download search modal. Renders nothing unless
/// the Anna's Archive feature is enabled and the current user holds
/// `DownloadBook` — mirrors the availability gating used elsewhere.
///
/// The button only flips [`DOWNLOAD_MODAL_OPEN`]; the modal itself is rendered
/// by `NavBar` so it survives the Incoming page's suspense-driven remounts.
#[component]
pub(crate) fn AddDownloadButton() -> Element {
    let available = use_server_future(get_download_availability)?;
    let show = available().and_then(|r: Result<bool, ServerFnError>| r.ok()).unwrap_or(false);
    if !show {
        return rsx! {};
    }

    rsx! {
        button {
            class: "inline-flex items-center gap-1.5 px-3 py-2 rounded bg-indigo-600 text-white text-sm font-medium hover:bg-indigo-700 cursor-pointer",
            onclick: move |_| *DOWNLOAD_MODAL_OPEN.write() = true,
            svg {
                class: "w-4 h-4",
                fill: "none",
                view_box: "0 0 24 24",
                stroke_width: "1.5",
                stroke: "currentColor",
                path {
                    stroke_linecap: "round",
                    stroke_linejoin: "round",
                    d: "M12 4.5v15m7.5-7.5h-15",
                }
            }
            span { "Add download" }
        }
    }
}

/// Modal wrapping the Anna's Archive search + manual-import UI. Queued
/// downloads land in the Incoming list; the modal bumps the incoming/jobs
/// refresh signals so the list updates live behind it.
#[component]
pub(crate) fn DownloadModal(on_close: EventHandler<()>) -> Element {
    let mut incoming_refresh = use_context::<IncomingRefresh>();
    let mut jobs_refresh = use_context::<JobsRefresh>();

    let mut query = use_signal(String::new);
    let mut language = use_signal(String::new);
    let mut results: Signal<Option<Result<Vec<DownloadResult>, String>>> = use_signal(|| None);
    let mut searching = use_signal(|| false);
    // external_id currently being enqueued, and those already queued this session.
    let mut downloading: Signal<Option<String>> = use_signal(|| None);
    let mut queued: Signal<Vec<String>> = use_signal(Vec::new);
    let mut manual_input = use_signal(String::new);
    let mut manual_msg: Signal<Option<Result<String, String>>> = use_signal(|| None);
    // Error surfaced from a results-row download (only one runs at a time).
    let mut download_err: Signal<Option<String>> = use_signal(|| None);

    // Bump the refresh signals so the Incoming list picks up the new download.
    let mut notify_incoming = move || {
        *incoming_refresh.0.write() += 1;
        *jobs_refresh.0.write() += 1;
    };

    let run_search = move || {
        if searching() {
            return;
        }
        let q = query.read().trim().to_owned();
        if q.is_empty() {
            return;
        }
        let lang = language.read().clone();
        spawn(async move {
            searching.set(true);
            results.set(None);
            let outcome = search_downloads(q, lang).await.map_err(|e| e.to_string());
            results.set(Some(outcome));
            searching.set(false);
        });
    };

    rsx! {
        div {
            class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4",
            tabindex: -1,
            onmounted: move |e| async move { let _ = e.set_focus(true).await; },
            onclick: move |_| on_close(()),
            onkeydown: move |e| { if e.key() == Key::Escape { on_close(()); } },
            div {
                class: "bg-white dark:bg-slate-800 rounded-xl shadow-xl w-full max-w-3xl max-h-[85vh] flex flex-col",
                onclick: |e| e.stop_propagation(),
                // ── Header ────────────────────────────────────────────────────
                div { class: "flex items-center justify-between px-6 pt-5 pb-3 border-b border-gray-200 dark:border-slate-700",
                    div {
                        h2 { class: "text-lg font-semibold text-gray-900 dark:text-slate-100", "Add download" }
                        p { class: "text-sm text-gray-500 dark:text-slate-400 mt-0.5",
                            "Search Anna's Archive for EPUBs. Downloads land in Incoming for review."
                        }
                    }
                    button {
                        class: "text-gray-400 dark:text-slate-500 hover:text-gray-600 dark:hover:text-slate-300 cursor-pointer",
                        onclick: move |_| on_close(()),
                        svg {
                            class: "w-5 h-5",
                            fill: "none",
                            view_box: "0 0 24 24",
                            stroke_width: "1.5",
                            stroke: "currentColor",
                            path { stroke_linecap: "round", stroke_linejoin: "round", d: "M6 18 18 6M6 6l12 12" }
                        }
                    }
                }

                // ── Body (scrollable) ─────────────────────────────────────────
                div { class: "px-6 py-4 overflow-y-auto flex flex-col",
                    // Search controls
                    div { class: "flex flex-wrap items-center gap-3",
                        input {
                            class: "flex-1 min-w-[12rem] px-3 py-2 rounded border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-800 text-sm text-gray-900 dark:text-slate-100",
                            r#type: "text",
                            placeholder: "Title, author, ISBN…",
                            value: "{query}",
                            oninput: move |e| query.set(e.value()),
                            onkeydown: move |e| {
                                if e.key() == Key::Enter {
                                    run_search();
                                }
                            },
                        }
                        select {
                            class: "px-3 py-2 rounded border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-800 text-sm text-gray-900 dark:text-slate-100",
                            value: "{language}",
                            onchange: move |e| language.set(e.value()),
                            option { value: "", "Any language" }
                            for code in crate::components::LANGUAGE_CODES.iter() {
                                option { value: "{code}", "{code.to_uppercase()}" }
                            }
                        }
                        button {
                            class: "px-4 py-2 rounded bg-indigo-600 text-white text-sm font-medium hover:bg-indigo-700 disabled:opacity-40 disabled:cursor-not-allowed cursor-pointer",
                            disabled: searching(),
                            onclick: move |_| run_search(),
                            if searching() { "Searching…" } else { "Search" }
                        }
                    }

                    // Manual import by MD5 / URL
                    div { class: "mt-3 flex flex-wrap items-center gap-3",
                        span { class: "text-xs text-gray-500 dark:text-slate-400", "Or import directly:" }
                        input {
                            class: "flex-1 min-w-[12rem] px-3 py-2 rounded border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-800 text-sm text-gray-900 dark:text-slate-100 font-mono",
                            r#type: "text",
                            placeholder: "MD5 hash or https://annas-archive.org/md5/…",
                            value: "{manual_input}",
                            oninput: move |e| manual_input.set(e.value()),
                        }
                        button {
                            class: "px-4 py-2 rounded border border-indigo-300 dark:border-indigo-600 text-indigo-600 dark:text-indigo-400 text-sm font-medium hover:bg-indigo-50 dark:hover:bg-indigo-900/30 disabled:opacity-40 disabled:cursor-not-allowed cursor-pointer",
                            disabled: downloading().is_some(),
                            onclick: move |_| {
                                let input = manual_input.read().trim().to_owned();
                                if input.is_empty() {
                                    return;
                                }
                                downloading.set(Some(input.clone()));
                                spawn(async move {
                                    let outcome = start_download(input, String::new(), String::new(), String::new()).await;
                                    downloading.set(None);
                                    match outcome {
                                        Ok(DownloadOutcome::Queued) => {
                                            manual_msg.set(Some(Ok("Queued — track it in Incoming.".to_owned())));
                                            manual_input.set(String::new());
                                            notify_incoming();
                                        }
                                        Ok(DownloadOutcome::Error(e)) => manual_msg.set(Some(Err(e))),
                                        Err(e) => manual_msg.set(Some(Err(e.to_string()))),
                                    }
                                });
                            },
                            "Import"
                        }
                        {
                            match manual_msg() {
                                Some(Ok(m)) => rsx! { span { class: "text-xs text-green-700 dark:text-green-400", "{m}" } },
                                Some(Err(e)) => rsx! { span { class: "text-xs text-red-600 dark:text-red-400", "{e}" } },
                                None => rsx! {},
                            }
                        }
                    }

                    // Results
                    if let Some(err) = download_err() {
                        div { class: "mt-3 px-3 py-2 rounded bg-red-50 dark:bg-red-900/20 text-red-600 dark:text-red-400 text-sm",
                            "{err}"
                        }
                    }
                    div { class: "mt-4",
                        match results() {
                            None => rsx! {
                                div { class: "flex items-center justify-center text-gray-400 dark:text-slate-500 text-sm py-10",
                                    "Search for a book to get started."
                                }
                            },
                            Some(Err(e)) => rsx! {
                                div { class: "flex items-center justify-center text-red-600 dark:text-red-400 text-sm py-10", "{e}" }
                            },
                            Some(Ok(items)) if items.is_empty() => rsx! {
                                div { class: "flex items-center justify-center text-gray-400 dark:text-slate-500 text-sm py-10", "No EPUB results found." }
                            },
                            Some(Ok(items)) => rsx! {
                                table { class: "min-w-full divide-y divide-gray-200 dark:divide-slate-700 text-sm",
                                    thead {
                                        tr {
                                            th { class: "px-3 py-2 text-left font-medium text-gray-500 dark:text-slate-400 uppercase tracking-wider", "Title" }
                                            th { class: "px-3 py-2 text-left font-medium text-gray-500 dark:text-slate-400 uppercase tracking-wider", "Authors" }
                                            th { class: "px-3 py-2 text-center font-medium text-gray-500 dark:text-slate-400 uppercase tracking-wider", "Lang" }
                                            th { class: "px-3 py-2 text-center font-medium text-gray-500 dark:text-slate-400 uppercase tracking-wider", "Size" }
                                            th { class: "px-3 py-2" }
                                        }
                                    }
                                    tbody { class: "divide-y divide-gray-100 dark:divide-slate-700",
                                        for item in items {
                                            {
                                                let id = item.external_id.clone();
                                                let is_queued = queued.read().contains(&id);
                                                let is_downloading = downloading.read().as_deref() == Some(id.as_str());
                                                let busy = downloading.read().is_some();
                                                let title = item.title.clone();
                                                let authors = item.authors.clone();
                                                let lang = item.language.clone().unwrap_or_default();
                                                rsx! {
                                                    tr { key: "{item.external_id}",
                                                        td { class: "px-3 py-2 text-gray-900 dark:text-slate-100", "{item.title}" }
                                                        td { class: "px-3 py-2 text-gray-600 dark:text-slate-300",
                                                            if item.authors.is_empty() {
                                                                span { class: "text-gray-400 dark:text-slate-500 italic", "Unknown" }
                                                            } else {
                                                                "{item.authors}"
                                                            }
                                                        }
                                                        td { class: "px-3 py-2 text-gray-500 dark:text-slate-400 text-center uppercase", "{lang}" }
                                                        td { class: "px-3 py-2 text-gray-500 dark:text-slate-400 text-center", {item.size.clone().unwrap_or_default()} }
                                                        td { class: "px-3 py-2 text-right",
                                                            if is_queued {
                                                                span { class: "text-sm font-medium text-green-700 dark:text-green-400", "Queued ✓" }
                                                            } else {
                                                                button {
                                                                    class: "px-3 py-1 rounded border border-indigo-300 dark:border-indigo-600 text-sm font-medium text-indigo-600 dark:text-indigo-400 hover:bg-indigo-50 dark:hover:bg-indigo-900/30 disabled:opacity-40 disabled:cursor-not-allowed cursor-pointer",
                                                                    disabled: busy,
                                                                    onclick: move |_| {
                                                                        let id = id.clone();
                                                                        let (title, authors, lang) = (title.clone(), authors.clone(), lang.clone());
                                                                        downloading.set(Some(id.clone()));
                                                                        download_err.set(None);
                                                                        spawn(async move {
                                                                            let outcome = start_download(id.clone(), title, authors, lang).await;
                                                                            downloading.set(None);
                                                                            match outcome {
                                                                                Ok(DownloadOutcome::Queued) => {
                                                                                    queued.write().push(id);
                                                                                    notify_incoming();
                                                                                }
                                                                                Ok(DownloadOutcome::Error(e)) => download_err.set(Some(e)),
                                                                                Err(e) => download_err.set(Some(e.to_string())),
                                                                            }
                                                                        });
                                                                    },
                                                                    if is_downloading { "Starting…" } else { "Download" }
                                                                }
                                                            }
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
        }
    }
}
