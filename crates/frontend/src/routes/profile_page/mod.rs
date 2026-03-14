use dioxus::prelude::*;

use crate::Route;
#[cfg(feature = "server")]
use crate::server::AuthSession;

// ---------------------------------------------------------------------------
// Auth check
// ---------------------------------------------------------------------------

#[get(
    "/api/v1/profile/context",
    auth_session: axum::Extension<AuthSession>
)]
async fn get_profile_context() -> Result<(), ServerFnError> {
    auth_session
        .current_user
        .as_ref()
        .filter(|u| !u.username.is_empty())
        .ok_or_else(|| ServerFnError::new("Not authenticated"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// ProfilePage
// ---------------------------------------------------------------------------

#[component]
pub(crate) fn ProfilePage() -> Element {
    let navigator = use_navigator();
    let auth = use_server_future(get_profile_context)?;

    use_effect(move || {
        if let Some(Err(_)) = auth() {
            navigator.replace(Route::LandingPage {});
        }
    });

    rsx! {
        div { class: "flex-1 overflow-auto p-8",
            div { class: "max-w-lg mx-auto flex flex-col gap-10",

                // ── Profile ──────────────────────────────────────────────
                section {
                    h2 { class: "text-lg font-semibold text-gray-900 mb-4", "Profile" }
                    p { class: "text-sm text-gray-500", "Profile settings coming soon." }
                }

                hr { class: "border-gray-200" }

                // ── Reading ───────────────────────────────────────────────
                section {
                    h2 { class: "text-lg font-semibold text-gray-900 mb-4", "Reading" }
                    p { class: "text-sm text-gray-500", "Reading settings coming soon." }
                }

                hr { class: "border-gray-200" }

                // ── My Devices ────────────────────────────────────────────
                section {
                    h2 { class: "text-lg font-semibold text-gray-900 mb-4", "My Devices" }
                    p { class: "text-sm text-gray-500", "Device management coming soon." }
                }
            }
        }
    }
}
