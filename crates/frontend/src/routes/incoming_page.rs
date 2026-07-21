use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use dioxus::{html::HasFileData, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{
    Route,
    components::{AddDownloadButton, IncomingRefresh, JobsRefresh},
};

/// Maps a raw import origin string to a display label and badge classes.
fn source_display(source: &str) -> (&'static str, &'static str) {
    match source {
        "upload" => ("Upload", "bg-sky-100 text-sky-700 dark:bg-sky-900/40 dark:text-sky-300"),
        "annas_archive" => ("Anna's Archive", "bg-purple-100 text-purple-700 dark:bg-purple-900/40 dark:text-purple-300"),
        _ => ("Auto-import", "bg-gray-100 text-gray-600 dark:bg-slate-700 dark:text-slate-300"),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct IncomingBookSummary {
    pub job_token: String,
    pub file_path: String,
    pub file_format: String,
    pub detected_at: String,
    /// Raw origin string (`bookdrop` / `upload` / `annas_archive`).
    pub source: String,
    pub title: Option<String>,
    pub author_names: Vec<String>,
    pub has_cover: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) enum UploadOutcome {
    Queued,
    AlreadyImported,
    InvalidFile,
    Error(String),
}

/// A single in-flight (or recently failed) Anna's Archive download, merged into
/// the Incoming list as a row until it completes and becomes an import job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DownloadActivity {
    pub title: String,
    pub authors: String,
    pub external_id: String,
    pub status: String,
    pub error: Option<String>,
}

/// Incoming review items, populated by [`IncomingDataLoader`] and read by
/// [`IncomingList`]. `None` until the first load resolves.
///
/// The fetch lives in a separate, *suspending* loader so the display component
/// never suspends. Otherwise a background refresh (the SSE job/incoming events
/// that fire constantly while a download runs) would re-suspend the whole page,
/// remounting the header — and its download button — on every event, spamming
/// the availability endpoint and churning the vdom.
static INCOMING_ITEMS: GlobalSignal<Option<Result<Vec<IncomingBookSummary>, String>>> = Signal::global(|| None);
/// In-flight download activity, populated by [`IncomingDataLoader`].
static DOWNLOAD_ACTIVITY: GlobalSignal<Vec<DownloadActivity>> = Signal::global(Vec::new);

#[cfg(feature = "server")]
use {
    crate::routes::server_helpers::{require_capability, to_server_err},
    crate::server::AuthSession,
    axum::http::Method,
    base64::prelude::*,
    bb_core::{
        CoreServices,
        book::BookId,
        download::AnnasDownloadPayload,
        error::ErrorKind,
        import::{ImportJobToken, ImportOrigin, service::FileQueueStatus},
        jobs::JobStatus,
        types::Capability,
    },
    std::collections::HashMap,
    std::sync::Arc,
};

#[post("/api/v1/incoming/scan_on_enter", auth_session: axum::Extension<AuthSession>, core_services: axum::Extension<Arc<CoreServices>>)]
async fn scan_on_enter() -> Result<(), ServerFnError> {
    require_capability(&auth_session, Capability::ApproveImports, Method::POST).await?;
    core_services.import_job_service.trigger_scan();
    Ok(())
}

#[get("/api/v1/incoming", auth_session: axum::Extension<AuthSession>, core_services: axum::Extension<Arc<CoreServices>>)]
async fn list_incoming_books() -> Result<Vec<IncomingBookSummary>, ServerFnError> {
    require_capability(&auth_session, Capability::ApproveImports, Method::GET).await?;

    let import_service = &core_services.import_job_service;
    let book_service = &core_services.book_service;

    let jobs = import_service.list_all_needs_review().await.map_err(to_server_err)?;

    // Collect all candidate book IDs, then fetch books and hydration data in bulk.
    let book_ids: Vec<BookId> = jobs.iter().filter_map(|j| j.candidate_book_id).collect();

    let books = book_service.find_books_by_ids(&book_ids).await.map_err(to_server_err)?;
    let hydration = book_service.fetch_hydration_data(&book_ids, &[]).await.map_err(to_server_err)?;

    let book_map: HashMap<BookId, _> = books.iter().map(|b| (b.id, b)).collect();

    let mut book_authors_map: HashMap<BookId, Vec<_>> = HashMap::new();
    for ba in &hydration.book_authors {
        book_authors_map.entry(ba.book_id).or_default().push(ba);
    }

    let author_map: HashMap<_, _> = hydration.authors.iter().map(|a| (a.id, a)).collect();

    let mut summaries: Vec<IncomingBookSummary> = jobs
        .iter()
        .map(|job| {
            let (title, author_names, has_cover) = if let Some(book_id) = job.candidate_book_id {
                if let Some(book) = book_map.get(&book_id) {
                    let mut bas = book_authors_map.get(&book_id).cloned().unwrap_or_default();
                    bas.sort_by_key(|ba| ba.sort_order);
                    let names = bas.iter().filter_map(|ba| author_map.get(&ba.author_id).map(|a| a.name.clone())).collect();
                    (Some(book.title.clone()), names, book.has_cover)
                } else {
                    (None, vec![], false)
                }
            } else {
                (None, vec![], false)
            };

            let filename = std::path::Path::new(&job.file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&job.file_path)
                .to_owned();

            IncomingBookSummary {
                job_token: job.token.to_string(),
                file_path: filename,
                file_format: job.file_format.as_str().to_owned(),
                detected_at: job.detected_at.to_rfc3339(),
                source: job.source.as_str().to_owned(),
                title,
                author_names,
                has_cover,
            }
        })
        .collect();

    summaries.sort_by(|a, b| {
        let a_author = a.author_names.first().map(|s| s.to_lowercase());
        let b_author = b.author_names.first().map(|s| s.to_lowercase());
        a_author.cmp(&b_author).then_with(|| {
            let a_title = a.title.as_deref().map(str::to_lowercase);
            let b_title = b.title.as_deref().map(str::to_lowercase);
            a_title.cmp(&b_title)
        })
    });

    Ok(summaries)
}

#[get("/api/v1/incoming/downloads", auth_session: axum::Extension<AuthSession>, core_services: axum::Extension<Arc<CoreServices>>)]
async fn list_download_activity() -> Result<Vec<DownloadActivity>, ServerFnError> {
    require_capability(&auth_session, Capability::ApproveImports, Method::GET).await?;

    let jobs = core_services.job_service.list_active_by_type("annas_download").await.map_err(to_server_err)?;

    let mut activity = Vec::new();
    for job in jobs {
        let status = match job.status {
            JobStatus::Pending => "Queued",
            JobStatus::Running => "Downloading…",
            JobStatus::Failed => "Failed",
            JobStatus::Completed => continue,
        };
        let payload: Option<AnnasDownloadPayload> = serde_json::from_value(job.payload).ok();
        let external_id = payload.as_ref().map(|p| p.external_id.clone()).unwrap_or_default();
        let title = payload
            .as_ref()
            .and_then(|p| p.title.clone())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| external_id.clone());
        let authors = payload.as_ref().and_then(|p| p.authors.clone()).unwrap_or_default();
        activity.push(DownloadActivity {
            title,
            authors,
            external_id,
            status: status.to_owned(),
            error: job.error_message,
        });
    }
    Ok(activity)
}

#[put("/api/v1/incoming/reject", auth_session: axum::Extension<AuthSession>, core_services: axum::Extension<Arc<CoreServices>>)]
async fn reject_incoming_book(job_token: String) -> Result<(), ServerFnError> {
    require_capability(&auth_session, Capability::ApproveImports, Method::PUT).await?;

    let token: ImportJobToken = job_token.parse().map_err(|_| ServerFnError::new("Invalid token"))?;

    core_services.collection_service.reject_book(token).await.map_err(to_server_err)?;

    Ok(())
}

#[post("/api/v1/incoming/upload", auth_session: axum::Extension<AuthSession>, core_services: axum::Extension<Arc<CoreServices>>)]
async fn upload_incoming_epub(filename: String, data_base64: String) -> Result<UploadOutcome, ServerFnError> {
    require_capability(&auth_session, Capability::ApproveImports, Method::POST).await?;

    let bytes = BASE64_STANDARD
        .decode(data_base64.as_bytes())
        .map_err(|_| ServerFnError::new("Invalid base64"))?;

    if bytes.len() > 50 * 1024 * 1024 {
        return Ok(UploadOutcome::Error("File exceeds 50 MB limit".into()));
    }

    match core_services.import_job_service.queue_bytes_if_new(filename, bytes, ImportOrigin::Upload).await {
        Ok(FileQueueStatus::Queued) => Ok(UploadOutcome::Queued),
        Ok(_) => Ok(UploadOutcome::AlreadyImported),
        Err(e) if e.kind() == ErrorKind::InvalidInput => Ok(UploadOutcome::InvalidFile),
        Err(e) => Ok(UploadOutcome::Error(e.to_string())),
    }
}

/// Renders an ISO 8601 timestamp, reformatting it to the browser's local
/// timezone after hydration via a `use_effect`.
#[component]
fn LocalTime(iso: String) -> Element {
    let mut display = use_signal(|| iso.clone());

    use_effect(move || {
        let iso = iso.clone();
        spawn(async move {
            let js = format!(r#"return new Date("{iso}").toLocaleString(undefined, {{dateStyle: "medium", timeStyle: "short"}})"#);
            if let Ok(val) = document::eval(&js).await {
                if let Some(s) = val.as_str() {
                    display.set(s.to_owned());
                }
            }
        });
    });

    rsx! { "{display}" }
}

#[component]
pub(crate) fn IncomingPage() -> Element {
    let mut incoming_refresh = use_context::<IncomingRefresh>();
    let mut jobs_refresh = use_context::<JobsRefresh>();

    // Trigger a bookdrop scan automatically when the page is entered
    use_effect(move || {
        spawn(async move {
            let _ = scan_on_enter().await;
            *incoming_refresh.0.write() += 1;
        });
    });

    let mut rejecting: Signal<Option<String>> = use_signal(|| None);
    let mut drag_active: Signal<bool> = use_signal(|| false);
    let mut drag_depth: Signal<i32> = use_signal(|| 0);
    let mut uploading: Signal<bool> = use_signal(|| false);
    let mut upload_results: Signal<Vec<(String, UploadOutcome)>> = use_signal(Vec::new);

    rsx! {
        div {
            class: "flex-1 flex flex-col overflow-hidden relative",
            ondragenter: move |evt: DragEvent| {
                evt.prevent_default();
                *drag_depth.write() += 1;
                drag_active.set(true);
            },
            ondragover: move |evt: DragEvent| {
                evt.prevent_default();
            },
            ondragleave: move |_| {
                let depth = {
                    let mut d = drag_depth.write();
                    *d -= 1;
                    *d
                };
                if depth <= 0 {
                    drag_depth.set(0);
                    drag_active.set(false);
                }
            },
            ondrop: move |evt: DragEvent| {
                evt.prevent_default();
                drag_depth.set(0);
                drag_active.set(false);
                let files: Vec<_> = evt.files().into_iter().collect();
                spawn(async move {
                    uploading.set(true);
                    upload_results.set(Vec::new());
                    let mut results: Vec<(String, UploadOutcome)> = Vec::new();
                    for file in files {
                        let name = file.name();
                        let lower = name.to_lowercase();
                        if !lower.ends_with(".epub") {
                            results.push((name, UploadOutcome::InvalidFile));
                            continue;
                        }
                        if file.size() > 50 * 1024 * 1024 {
                            results.push((name, UploadOutcome::Error("File exceeds 50 MB limit".into())));
                            continue;
                        }
                        let Ok(bytes_obj) = file.read_bytes().await else {
                            results.push((name, UploadOutcome::Error("Failed to read file".into())));
                            continue;
                        };
                        let encoded = B64.encode(bytes_obj.as_ref());
                        let outcome = upload_incoming_epub(name.clone(), encoded)
                            .await
                            .unwrap_or(UploadOutcome::Error("Network error".into()));
                        results.push((name, outcome));
                    }
                    uploading.set(false);
                    upload_results.set(results);
                    *incoming_refresh.0.write() += 1;
                    *jobs_refresh.0.write() += 1;
                    // Auto-dismiss toasts after 4 seconds
                    let mut timer = document::eval("setTimeout(() => dioxus.send(true), 4000)");
                    let _ = timer.recv::<bool>().await;
                    upload_results.set(Vec::new());
                });
            },
            if drag_active() {
                div {
                    class: "absolute inset-0 bg-indigo-500/60 flex flex-col items-center justify-center z-50 pointer-events-none",
                    svg {
                        class: "w-16 h-16 text-white mb-4",
                        xmlns: "http://www.w3.org/2000/svg",
                        fill: "none",
                        "viewBox": "0 0 24 24",
                        "stroke-width": "1.5",
                        stroke: "currentColor",
                        path {
                            "stroke-linecap": "round",
                            "stroke-linejoin": "round",
                            d: "M12 6.042A8.967 8.967 0 006 3.75c-1.052 0-2.062.18-3 .512v14.25A8.987 8.987 0 016 18c2.305 0 4.408.867 5.99 2.257M15 19.128v-.003c0-1.113-.285-2.16-.786-3.07M15 19.128v.106A12.318 12.318 0 018.624 21c-2.331 0-4.512-.645-6.374-1.766l-.001-.109a6.375 6.375 0 0111.964-3.07M12 6.042h.774a8.967 8.967 0 015.999 2.253M15 13.5a3 3 0 11-6 0 3 3 0 016 0z"
                        }
                    }
                    span { class: "text-white text-2xl font-semibold", "Drop EPUBs to import" }
                }
            }
            div { class: "px-6 py-4 border-b border-gray-200 dark:border-slate-700 flex items-center justify-between gap-4",
                h1 { class: "text-xl font-semibold text-gray-900 dark:text-slate-100", "Incoming" }
                SuspenseBoundary {
                    fallback: |_| rsx! {},
                    AddDownloadButton {}
                }
            }
            if uploading() {
                div { class: "px-6 py-2 bg-indigo-50 dark:bg-indigo-900/30 text-indigo-700 dark:text-indigo-300 text-sm border-b border-indigo-100 dark:border-indigo-800",
                    "Uploading…"
                }
            }
            {
                let results = upload_results();
                if results.is_empty() {
                    rsx! {}
                } else {
                    let total = results.len();
                    rsx! {
                        if total <= 3 {
                            div { class: "px-6 py-2 flex flex-col gap-1",
                                for (name, outcome) in &results {
                                    {
                                        let (bg, msg) = match outcome {
                                            UploadOutcome::Queued => ("bg-green-50 dark:bg-green-900/30 text-green-800 dark:text-green-300", format!("Added: {name}")),
                                            UploadOutcome::AlreadyImported => ("bg-gray-50 dark:bg-slate-800 text-gray-600 dark:text-slate-400", format!("Already in your library: {name}")),
                                            UploadOutcome::InvalidFile => ("bg-orange-50 dark:bg-orange-900/30 text-orange-800 dark:text-orange-300", format!("Not a valid EPUB: {name}")),
                                            UploadOutcome::Error(e) => ("bg-red-50 dark:bg-red-900/30 text-red-800 dark:text-red-300", format!("Failed: {name} — {e}")),
                                        };
                                        rsx! {
                                            div { class: "px-3 py-2 rounded text-sm {bg}", "{msg}" }
                                        }
                                    }
                                }
                            }
                        } else {
                            {
                                let added = results.iter().filter(|(_, o)| matches!(o, UploadOutcome::Queued)).count();
                                let failed = results.iter().filter(|(_, o)| matches!(o, UploadOutcome::Error(_))).count();
                                let skipped = total - added - failed;
                                let summary = match (skipped, failed) {
                                    (0, 0) => format!("{added} added"),
                                    (s, 0) => format!("{added} added, {s} skipped"),
                                    (0, f) => format!("{added} added, {f} failed"),
                                    (s, f) => format!("{added} added, {s} skipped, {f} failed"),
                                };
                                rsx! {
                                    div { class: "px-6 py-2 bg-gray-50 dark:bg-slate-800 text-gray-700 dark:text-slate-300 text-sm border-b border-gray-100 dark:border-slate-700",
                                        "{summary}"
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // Invisible loader owns the suspending fetches and writes the
            // globals below. Isolated in its own SuspenseBoundary so its
            // refetch-suspension never unmounts this page or its header.
            SuspenseBoundary {
                fallback: |_| rsx! {},
                IncomingDataLoader {}
            }
            match INCOMING_ITEMS() {
                None => rsx! {
                    div { class: "flex-1 flex items-center justify-center text-gray-400 dark:text-slate-500 text-sm",
                        "Loading…"
                    }
                },
                Some(Err(e)) => rsx! {
                    div { class: "flex-1 flex items-center justify-center text-red-600 dark:text-red-400 text-sm",
                        "{e}"
                    }
                },
                Some(Ok(items)) => {
                    let download_rows = DOWNLOAD_ACTIVITY();
                    if items.is_empty() && download_rows.is_empty() {
                        rsx! {
                            div { class: "flex-1 flex items-center justify-center text-gray-400 dark:text-slate-500 text-sm",
                                "No books awaiting review."
                            }
                        }
                    } else {
                        rsx! {
                            div { class: "flex-1 overflow-auto",
                                table { class: "min-w-full divide-y divide-gray-200 dark:divide-slate-700 text-sm",
                                    thead { class: "bg-gray-50 dark:bg-slate-800",
                                        tr {
                                            th { class: "px-6 py-3 text-left font-medium text-gray-500 dark:text-slate-400 uppercase tracking-wider", "Title" }
                                            th { class: "px-6 py-3 text-left font-medium text-gray-500 dark:text-slate-400 uppercase tracking-wider", "Authors" }
                                            th { class: "px-6 py-3 text-left font-medium text-gray-500 dark:text-slate-400 uppercase tracking-wider", "Source" }
                                            th { class: "px-6 py-3 text-center font-medium text-gray-500 dark:text-slate-400 uppercase tracking-wider", "Format" }
                                            th { class: "px-6 py-3 text-left font-medium text-gray-500 dark:text-slate-400 uppercase tracking-wider", "File" }
                                            th { class: "px-6 py-3 text-left font-medium text-gray-500 dark:text-slate-400 uppercase tracking-wider", "Detected" }
                                            th { class: "px-6 py-3" }
                                        }
                                    }
                                    tbody { class: "bg-white dark:bg-slate-800 divide-y divide-gray-100 dark:divide-slate-700",
                                        // In-flight / failed downloads first — they carry a live
                                        // status where the review actions will later appear.
                                        for d in download_rows {
                                            {
                                                let (src_label, src_cls) = source_display("annas_archive");
                                                let status_cls = match d.status.as_str() {
                                                    "Failed" => "text-red-600 dark:text-red-400",
                                                    "Downloading…" => "text-indigo-600 dark:text-indigo-400",
                                                    _ => "text-gray-500 dark:text-slate-400",
                                                };
                                                rsx! {
                                                    tr { key: "dl-{d.external_id}",
                                                        td { class: "px-6 py-4 text-gray-900 dark:text-slate-100", "{d.title}" }
                                                        td { class: "px-6 py-4 text-gray-600 dark:text-slate-300",
                                                            if d.authors.is_empty() {
                                                                span { class: "text-gray-400 dark:text-slate-500 italic", "Unknown" }
                                                            } else {
                                                                "{d.authors}"
                                                            }
                                                        }
                                                        td { class: "px-6 py-4",
                                                            span { class: "inline-flex px-2 py-0.5 rounded-full text-xs font-medium {src_cls}", "{src_label}" }
                                                        }
                                                        td { class: "px-6 py-4 text-gray-600 dark:text-slate-300 text-center", "EPUB" }
                                                        td { class: "px-6 py-4 text-gray-500 dark:text-slate-400 font-mono text-xs",
                                                            if d.external_id.is_empty() {
                                                                "—"
                                                            } else {
                                                                "{d.external_id}"
                                                            }
                                                        }
                                                        td { class: "px-6 py-4 text-gray-400 dark:text-slate-500", "—" }
                                                        td { class: "px-6 py-4 text-right",
                                                            div { class: "flex flex-col items-end gap-0.5",
                                                                span { class: "{status_cls} whitespace-nowrap font-medium", "{d.status}" }
                                                                if let Some(err) = &d.error {
                                                                    span { class: "text-xs text-red-500 dark:text-red-400 max-w-xs truncate", title: "{err}", "{err}" }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        for item in items {
                                            {
                                                let (src_label, src_cls) = source_display(&item.source);
                                                rsx! {
                                                    tr { key: "{item.job_token}",
                                                        td { class: "px-6 py-4 text-gray-900 dark:text-slate-100",
                                                            match &item.title {
                                                                Some(t) => rsx! { "{t}" },
                                                                None => rsx! {
                                                                    span { class: "text-gray-400 dark:text-slate-500 italic", "Unknown" }
                                                                },
                                                            }
                                                        }
                                                        td { class: "px-6 py-4 text-gray-600 dark:text-slate-300",
                                                            if item.author_names.is_empty() {
                                                                span { class: "text-gray-400 dark:text-slate-500 italic", "Unknown" }
                                                            } else {
                                                                "{item.author_names.join(\", \")}"
                                                            }
                                                        }
                                                        td { class: "px-6 py-4",
                                                            span { class: "inline-flex px-2 py-0.5 rounded-full text-xs font-medium {src_cls}", "{src_label}" }
                                                        }
                                                        td { class: "px-6 py-4 text-gray-600 dark:text-slate-300 text-center", "{item.file_format}" }
                                                        td { class: "px-6 py-4 text-gray-500 dark:text-slate-400 font-mono text-xs", "{item.file_path}" }
                                                        td { class: "px-6 py-4 text-gray-500 dark:text-slate-400 whitespace-nowrap",
                                                            LocalTime { iso: item.detected_at.clone() }
                                                        }
                                                        td { class: "px-6 py-4 text-right flex items-center justify-end gap-3",
                                                            Link {
                                                                to: Route::ReviewPage { token: item.job_token.clone() },
                                                                class: "px-3 py-1 rounded border border-indigo-300 dark:border-indigo-600 text-sm font-medium text-indigo-600 dark:text-indigo-400 hover:bg-indigo-50 dark:hover:bg-indigo-900/30",
                                                                "Review"
                                                            }
                                                            {
                                                                let token = item.job_token.clone();
                                                                let is_rejecting = rejecting.read().as_deref() == Some(&token);
                                                                let any_rejecting = rejecting.read().is_some();
                                                                let btn_class = if any_rejecting {
                                                                    "px-3 py-1 rounded border border-red-300 dark:border-red-700 text-sm font-medium text-red-600 dark:text-red-400 opacity-40 cursor-not-allowed"
                                                                } else {
                                                                    "px-3 py-1 rounded border border-red-300 dark:border-red-700 text-sm font-medium text-red-600 dark:text-red-400 hover:bg-red-50 dark:hover:bg-red-900/30 cursor-pointer"
                                                                };
                                                                rsx! {
                                                                    button {
                                                                        class: "{btn_class}",
                                                                        disabled: any_rejecting,
                                                                        onclick: move |_| {
                                                                            let token = token.clone();
                                                                            rejecting.set(Some(token.clone()));
                                                                            spawn(async move {
                                                                                let result = reject_incoming_book(token).await;
                                                                                rejecting.set(None);
                                                                                if result.is_ok() {
                                                                                    *incoming_refresh.0.write() += 1;
                                                                                }
                                                                            });
                                                                        },
                                                                        if is_rejecting { "Rejecting…" } else { "Reject" }
                                                                    }
                                                                }
                                                            }
                                                        }
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

/// Invisible loader: owns the suspending `use_server_future` fetches for the
/// incoming list and download activity, writing both into module globals.
///
/// Kept separate from `IncomingPage` (the display) so the display never
/// suspends — mirrors the `IncomingCountLoader`/`IncomingBadge` split in the
/// nav bar. A background refresh only re-suspends this (invisible) component,
/// leaving the page, its header, and the download button mounted.
#[component]
fn IncomingDataLoader() -> Element {
    let incoming_refresh = use_context::<IncomingRefresh>();
    let jobs_refresh = use_context::<JobsRefresh>();

    let items = use_server_future(move || {
        let _rev = (incoming_refresh.0)();
        let _jrev = (jobs_refresh.0)();
        list_incoming_books()
    })?;
    let downloads = use_server_future(move || {
        let _rev = (incoming_refresh.0)();
        let _jrev = (jobs_refresh.0)();
        list_download_activity()
    })?;

    // Publish resolved data to the globals the page reads. Previous values are
    // left in place while a refetch is pending, so the list never flashes empty.
    use_effect(move || {
        if let Some(res) = items() {
            *INCOMING_ITEMS.write() = Some(res.map_err(|e| e.to_string()));
        }
    });
    use_effect(move || {
        if let Some(Ok(list)) = downloads() {
            *DOWNLOAD_ACTIVITY.write() = list;
        }
    });

    rsx! {}
}
