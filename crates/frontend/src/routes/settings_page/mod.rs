mod messages_section;
mod tasks_section;
mod users_section;

use dioxus::prelude::*;
use messages_section::MessagesSection;
use tasks_section::TasksSection;
use users_section::UsersSection;

use crate::Route;
#[cfg(feature = "server")]
use crate::routes::server_helpers::authenticated_user;
#[cfg(feature = "server")]
use crate::server::AuthSession;

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
    let user = authenticated_user(&auth_session)?;

    let is_super_admin = user.permissions.contains("SuperAdmin");
    let is_admin = is_super_admin || user.permissions.contains("Admin");

    Ok(SettingsContext {
        is_admin,
        is_super_admin,
        current_user_token: bb_core::user::UserToken::new(user.id()).to_string(),
    })
}

// ---------------------------------------------------------------------------
// Section tabs
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Section {
    Users,
    Tasks,
    Messages,
}

// ---------------------------------------------------------------------------
// SettingsPage
// ---------------------------------------------------------------------------

#[component]
pub(crate) fn SettingsPage() -> Element {
    let navigator = use_navigator();
    let ctx = use_server_future(get_settings_context)?;

    let context = ctx().and_then(std::result::Result::ok).unwrap_or(SettingsContext {
        is_admin: false,
        is_super_admin: false,
        current_user_token: String::new(),
    });

    use_effect(move || match ctx() {
        Some(Err(_)) => {
            navigator.replace(Route::LandingPage {});
        }
        Some(Ok(ref c)) if !c.is_admin => {
            navigator.replace(Route::BooksPage {});
        }
        _ => {}
    });

    let mut active_section = use_signal(|| Section::Users);

    let nav_button_class = |section: Section| {
        if active_section() == section {
            "w-full text-left px-4 py-2 text-sm font-medium bg-indigo-50 text-indigo-700 border-r-2 border-indigo-600"
        } else {
            "w-full text-left px-4 py-2 text-sm font-medium text-gray-600 hover:bg-gray-50 hover:text-gray-900"
        }
    };

    rsx! {
        div { class: "flex h-full flex-1",
            // ----------------------------------------------------------------
            // Left panel — section list
            // ----------------------------------------------------------------
            nav { class: "w-48 shrink-0 border-r border-gray-200 bg-white",
                ul { class: "py-4",
                    li {
                        button {
                            class: nav_button_class(Section::Users),
                            onclick: move |_| active_section.set(Section::Users),
                            "Users"
                        }
                    }
                    li {
                        button {
                            class: nav_button_class(Section::Tasks),
                            onclick: move |_| active_section.set(Section::Tasks),
                            "Tasks"
                        }
                    }
                    li {
                        button {
                            class: nav_button_class(Section::Messages),
                            onclick: move |_| active_section.set(Section::Messages),
                            "Messages"
                        }
                    }
                }
            }
            // ----------------------------------------------------------------
            // Right panel — section content
            // ----------------------------------------------------------------
            div { class: "flex-1 overflow-auto p-8 flex flex-col items-center",
                match active_section() {
                    Section::Users => rsx! {
                        UsersSection {
                            is_super_admin: context.is_super_admin,
                            current_user_token: context.current_user_token.clone(),
                        }
                    },
                    Section::Tasks => rsx! {
                        TasksSection {}
                    },
                    Section::Messages => rsx! {
                        MessagesSection {}
                    },
                }
            }
        }
    }
}
