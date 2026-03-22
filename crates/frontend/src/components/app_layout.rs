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

#[get("/api/v1/check_auth", auth_session: axum::Extension<AuthSession>)]
async fn check_auth() -> Result<bool, ServerFnError> {
    Ok(auth_session.current_user.as_ref().is_some_and(|u| !u.username.is_empty()))
}

#[component]
pub(crate) fn AppLayout() -> Element {
    // Shared counter bumped after approve/reject so NavBar re-fetches the pending
    // count.
    use_context_provider(|| Signal::new(0u32));

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
