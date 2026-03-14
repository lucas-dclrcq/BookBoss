use dioxus::prelude::*;
#[cfg(feature = "server")]
use crate::server::AuthSession;

use crate::Route;

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
// Sections
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
enum ProfileSection {
    Profile,
    Reading,
    Devices,
}

impl ProfileSection {
    fn all() -> &'static [Self] {
        &[Self::Profile, Self::Reading, Self::Devices]
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Profile => "Profile",
            Self::Reading => "Reading",
            Self::Devices => "My Devices",
        }
    }
}

// ---------------------------------------------------------------------------
// ProfilePage
// ---------------------------------------------------------------------------

#[component]
pub(crate) fn ProfilePage() -> Element {
    let navigator = use_navigator();
    let mut active_section = use_signal(|| ProfileSection::Profile);
    let auth = use_server_future(get_profile_context)?;

    use_effect(move || {
        if let Some(Err(_)) = auth() {
            navigator.replace(Route::LandingPage {});
        }
    });

    rsx! {
        div { class: "flex h-full flex-1",
            // ----------------------------------------------------------------
            // Left panel — section list
            // ----------------------------------------------------------------
            nav { class: "w-48 shrink-0 border-r border-gray-200 bg-white",
                ul { class: "py-4",
                    for section in ProfileSection::all() {
                        {
                            let is_active = *active_section.read() == *section;
                            let item_class = if is_active {
                                "w-full text-left px-4 py-2 text-sm font-medium bg-indigo-50 text-indigo-700 border-r-2 border-indigo-600"
                            } else {
                                "w-full text-left px-4 py-2 text-sm text-gray-700 hover:bg-gray-50 cursor-pointer"
                            };
                            let section_clone = section.clone();
                            rsx! {
                                li {
                                    button {
                                        class: item_class,
                                        onclick: move |_| active_section.set(section_clone.clone()),
                                        { section.label() }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // ----------------------------------------------------------------
            // Right panel — section content
            // ----------------------------------------------------------------
            div { class: "flex-1 overflow-auto p-8 flex flex-col items-center",
                match *active_section.read() {
                    ProfileSection::Profile => rsx! { ProfileSectionContent {} },
                    ProfileSection::Reading => rsx! { ReadingSectionContent {} },
                    ProfileSection::Devices => rsx! { DevicesSectionContent {} },
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Section stubs (replaced in MP.10, MP.11, MP.12)
// ---------------------------------------------------------------------------

#[component]
fn ProfileSectionContent() -> Element {
    rsx! {
        div { class: "w-full max-w-lg",
            h2 { class: "text-lg font-semibold text-gray-900 mb-6", "Profile" }
            p { class: "text-sm text-gray-500", "Profile settings coming soon." }
        }
    }
}

#[component]
fn ReadingSectionContent() -> Element {
    rsx! {
        div { class: "w-full max-w-lg",
            h2 { class: "text-lg font-semibold text-gray-900 mb-6", "Reading" }
            p { class: "text-sm text-gray-500", "Reading settings coming soon." }
        }
    }
}

#[component]
fn DevicesSectionContent() -> Element {
    rsx! {
        div { class: "w-full max-w-lg",
            h2 { class: "text-lg font-semibold text-gray-900 mb-6", "My Devices" }
            p { class: "text-sm text-gray-500", "Device management coming soon." }
        }
    }
}
