mod users_section;

#[cfg(feature = "server")]
use bb_core::CoreServices;
use dioxus::prelude::*;
use users_section::UsersSection;
#[cfg(feature = "server")]
use {crate::server::AuthSession, std::sync::Arc};

use crate::Route;

// ---------------------------------------------------------------------------
// Settings context (admin status + current user identity)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct SettingsContext {
    pub is_admin: bool,
    pub is_super_admin: bool,
    pub current_user_token: String,
}

#[get(
    "/api/v1/settings/context",
    auth_session: axum::Extension<AuthSession>,
)]
async fn get_settings_context() -> Result<SettingsContext, ServerFnError> {
    let user = auth_session
        .current_user
        .as_ref()
        .filter(|u| !u.username.is_empty())
        .ok_or_else(|| ServerFnError::new("Not authenticated"))?;

    let is_super_admin = user.permissions.contains("SuperAdmin");
    let is_admin = is_super_admin || user.permissions.contains("Admin");

    Ok(SettingsContext {
        is_admin,
        is_super_admin,
        current_user_token: bb_core::user::UserToken::new(user.id()).to_string(),
    })
}

// ---------------------------------------------------------------------------
// Library statistics
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct LibraryStats {
    pub books: u64,
    pub authors: u64,
}

#[get(
    "/api/v1/library/stats",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
async fn get_library_stats() -> Result<LibraryStats, ServerFnError> {
    auth_session
        .current_user
        .as_ref()
        .filter(|u| !u.username.is_empty())
        .ok_or_else(|| ServerFnError::new("Not authenticated"))?;

    let stats = core_services
        .library_service
        .library_stats()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(LibraryStats {
        books: stats.books,
        authors: stats.authors,
    })
}

// ---------------------------------------------------------------------------
// Settings sections
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
enum SettingSection {
    Users,
    About,
}

impl SettingSection {
    fn all() -> &'static [Self] {
        &[Self::Users, Self::About]
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Users => "Users",
            Self::About => "About",
        }
    }

    fn is_visible(&self, ctx: &SettingsContext) -> bool {
        match self {
            Self::Users => ctx.is_admin || ctx.is_super_admin,
            Self::About => true,
        }
    }
}

// ---------------------------------------------------------------------------
// SettingsPage
// ---------------------------------------------------------------------------

#[component]
pub(crate) fn SettingsPage() -> Element {
    let navigator = use_navigator();
    let mut active_section = use_signal(|| SettingSection::About);
    let stats = use_server_future(get_library_stats)?;
    let ctx = use_server_future(get_settings_context)?;

    use_effect(move || {
        if let Some(Err(_)) = stats() {
            navigator.replace(Route::LandingPage {});
        }
    });

    let context = ctx().and_then(std::result::Result::ok).unwrap_or(SettingsContext {
        is_admin: false,
        is_super_admin: false,
        current_user_token: String::new(),
    });

    let visible_sections: Vec<&SettingSection> = SettingSection::all().iter().filter(|s| s.is_visible(&context)).collect();

    rsx! {
        div { class: "flex h-full flex-1",
            // ----------------------------------------------------------------
            // Left panel — section list
            // ----------------------------------------------------------------
            nav { class: "w-48 shrink-0 border-r border-gray-200 bg-white",
                ul { class: "py-4",
                    for section in visible_sections {
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
                    SettingSection::Users => rsx! {
                        UsersSection {
                            is_super_admin: context.is_super_admin,
                            current_user_token: context.current_user_token.clone(),
                        }
                    },
                    SettingSection::About => rsx! {
                        AboutSection { stats: stats().and_then(std::result::Result::ok) }
                    },
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// About section
// ---------------------------------------------------------------------------

#[component]
fn AboutSection(stats: Option<LibraryStats>) -> Element {
    rsx! {
        div { class: "w-full max-w-lg",
            img {
                src: asset!("/assets/BookBoss-Banner.png"),
                alt: "BookBoss",
                class: "w-full mb-2",
            }
            p { class: "text-sm text-gray-500 mb-6 text-center",
                { format!("Version: {}", clap::crate_version!()) }
            }
            h2 { class: "text-lg font-semibold text-gray-900 mb-4", "Library Statistics" }
            dl { class: "divide-y divide-gray-100 rounded-lg border border-gray-200 bg-white",
                StatRow {
                    label: "Books",
                    value: stats.as_ref().map(|s| s.books.to_string()),
                }
                StatRow {
                    label: "Authors",
                    value: stats.as_ref().map(|s| s.authors.to_string()),
                }
            }
        }
    }
}

#[component]
fn StatRow(label: &'static str, value: Option<String>) -> Element {
    rsx! {
        div { class: "flex justify-between px-4 py-3",
            dt { class: "text-sm text-gray-500", { label } }
            dd { class: "text-sm font-medium text-gray-900",
                { value.as_deref().unwrap_or("—") }
            }
        }
    }
}
