use dioxus::prelude::*;

#[cfg(feature = "server")]
use crate::server::AuthSession;
use crate::{
    Route,
    components::{
        NavBar,
        selection::{SELECTION_MODE, exit_selection_mode},
        sort_control::{SORT_ORDER, get_sort_preference},
    },
};

/// Newtype wrapper so `use_context` can distinguish the incoming-review refresh
/// signal from other `Signal<u32>` contexts.
#[derive(Clone, Copy)]
pub(crate) struct IncomingRefresh(pub Signal<u32>);

/// Newtype wrapper for the background-jobs refresh signal.
#[derive(Clone, Copy)]
pub(crate) struct JobsRefresh(pub Signal<u32>);

/// Newtype wrapper for the system-messages refresh signal.
#[derive(Clone, Copy)]
pub(crate) struct SystemMessagesRefresh(pub Signal<u32>);

#[get("/api/v1/check_auth", auth_session: axum::Extension<AuthSession>)]
async fn check_auth() -> Result<bool, ServerFnError> {
    Ok(auth_session.current_user.as_ref().is_some_and(|u| !u.username.is_empty()))
}

#[component]
pub(crate) fn AppLayout() -> Element {
    let mut incoming_refresh = use_context_provider(|| IncomingRefresh(Signal::new(0u32)));
    let mut jobs_refresh = use_context_provider(|| JobsRefresh(Signal::new(0u32)));
    let mut messages_refresh = use_context_provider(|| SystemMessagesRefresh(Signal::new(0u32)));

    // Connect to the SSE event stream so the UI updates in real time when the
    // backend processes imports or background jobs.
    //
    // The JS-side guard (`window.__bb_es`) ensures exactly one EventSource
    // exists even if the component remounts during fullstack hydration.  If a
    // prior connection is still open it is closed first, preventing zombie
    // connections from exhausting the browser's per-domain connection limit.
    use_hook(move || {
        spawn(async move {
            let mut eval = document::eval(
                r"
                if (window.__bb_es) {
                    window.__bb_es.close();
                }
                const es = new EventSource('/api/v1/events');
                window.__bb_es = es;
                es.addEventListener('incoming_changed', () => dioxus.send('incoming_changed'));
                es.addEventListener('jobs_changed', () => dioxus.send('jobs_changed'));
                es.addEventListener('system_messages_changed', () => dioxus.send('system_messages_changed'));
                // Keep the eval alive indefinitely — EventSource auto-reconnects.
                await new Promise(() => {});
                ",
            );

            while let Ok(msg) = eval.recv::<String>().await {
                match msg.as_str() {
                    "incoming_changed" => *incoming_refresh.0.write() += 1,
                    "jobs_changed" => *jobs_refresh.0.write() += 1,
                    "system_messages_changed" => *messages_refresh.0.write() += 1,
                    _ => {}
                }
            }
        });
    });

    // Load persisted sort preference once; write to global signal.
    let sort_pref = use_server_future(get_sort_preference);
    use_effect(move || {
        if let Ok(res) = sort_pref {
            if let Some(Ok(Some(order))) = res() {
                *SORT_ORDER.write() = order;
            }
        }
    });

    // Exit selection mode when navigating away from book-grid pages.
    let route = use_route::<Route>();
    use_effect(move || {
        let on_grid_page = matches!(
            route,
            Route::BooksPage | Route::AuthorDetailPage { .. } | Route::SeriesDetailPage { .. } | Route::ShelfPage { .. }
        );
        if !on_grid_page && SELECTION_MODE() {
            exit_selection_mode();
        }
    });

    rsx! {
        document::Stylesheet { href: asset!("/assets/tailwind.css") }
        document::Link { rel: "icon", href: asset!("/assets/favicon.ico") }
        document::Link { rel: "apple-touch-icon", sizes: "180x180", href: asset!("/assets/apple-touch-icon.png") }
        document::Link { rel: "apple-touch-icon", sizes: "32x32", href: asset!("/assets/favicon-32x32.png") }
        document::Link { rel: "apple-touch-icon", sizes: "16x16", href: asset!("/assets/favicon-16x16.png") }
        div { class: "min-h-screen flex flex-col bg-gray-50 text-gray-900",
            NavBar {}
            main { class: "flex-1 flex overflow-hidden",
                SuspenseBoundary {
                    fallback: |_| rsx! {},
                    AuthGate {}
                }
            }
        }
    }
}

/// Wraps the Outlet so that only the page content area suspends during the auth
/// check, leaving the `NavBar` visible immediately.
#[component]
fn AuthGate() -> Element {
    let navigator = use_navigator();
    let auth = use_server_future(check_auth)?;

    use_effect(move || {
        if let Some(Ok(false)) = auth() {
            navigator.replace(Route::LandingPage {});
        }
    });

    rsx! { Outlet::<Route> {} }
}
