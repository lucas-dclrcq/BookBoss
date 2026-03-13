mod users_section;

#[cfg(feature = "server")]
use bb_core::{
    CoreServices,
    reading::{AUTO_READ_THRESHOLD_KEY, DEFAULT_AUTO_READ_THRESHOLD},
};
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
// Reading settings
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ReadingSettings {
    pub auto_read_threshold_pct: u8,
}

#[get(
    "/api/v1/settings/reading",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
async fn get_reading_settings() -> Result<ReadingSettings, ServerFnError> {
    let user = auth_session
        .current_user
        .as_ref()
        .filter(|u| !u.username.is_empty())
        .ok_or_else(|| ServerFnError::new("Not authenticated"))?;

    let user_id = user.id();
    let setting = core_services
        .user_setting_service
        .get(user_id, AUTO_READ_THRESHOLD_KEY)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    let threshold_bps = setting.and_then(|s| s.value.parse::<u16>().ok()).unwrap_or(DEFAULT_AUTO_READ_THRESHOLD);

    #[expect(clippy::cast_possible_truncation, reason = "bps / 100 gives 0–100 percentage; always fits u8")]
    let auto_read_threshold_pct = (threshold_bps / 100) as u8;
    Ok(ReadingSettings { auto_read_threshold_pct })
}

#[post(
    "/api/v1/settings/reading/auto-read-threshold",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
async fn save_auto_read_threshold(threshold_pct: u8) -> Result<(), ServerFnError> {
    let user = auth_session
        .current_user
        .as_ref()
        .filter(|u| !u.username.is_empty())
        .ok_or_else(|| ServerFnError::new("Not authenticated"))?;

    if threshold_pct > 100 {
        return Err(ServerFnError::new("Threshold must be between 0 and 100"));
    }

    let user_id = user.id();
    let bps = threshold_pct as u16 * 100;
    core_services
        .user_setting_service
        .set(user_id, AUTO_READ_THRESHOLD_KEY, &bps.to_string())
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Settings sections
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
enum SettingSection {
    Users,
    Reading,
    About,
}

impl SettingSection {
    fn all() -> &'static [Self] {
        &[Self::Users, Self::Reading, Self::About]
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Users => "Users",
            Self::Reading => "Reading",
            Self::About => "About",
        }
    }

    fn is_visible(&self, ctx: &SettingsContext) -> bool {
        match self {
            Self::Users => ctx.is_admin || ctx.is_super_admin,
            Self::Reading | Self::About => true,
        }
    }
}

// ---------------------------------------------------------------------------
// SettingsPage
// ---------------------------------------------------------------------------

#[component]
pub(crate) fn SettingsPage() -> Element {
    let navigator = use_navigator();
    let mut active_section = use_signal(|| SettingSection::Reading);
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
                    SettingSection::Reading => rsx! { ReadingSection {} },
                    SettingSection::About => rsx! {
                        AboutSection { stats: stats().and_then(std::result::Result::ok) }
                    },
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Reading section
// ---------------------------------------------------------------------------

#[component]
fn ReadingSection() -> Element {
    let settings = use_server_future(get_reading_settings)?;
    let mut threshold = use_signal(|| 95u8);
    let mut saving = use_signal(|| false);
    let mut saved = use_signal(|| false);

    use_effect(move || {
        if let Some(Ok(s)) = settings() {
            threshold.set(s.auto_read_threshold_pct);
        }
    });

    rsx! {
        div { class: "w-full max-w-lg",
            h2 { class: "text-lg font-semibold text-gray-900 mb-6", "Reading" }

            div { class: "rounded-lg border border-gray-200 bg-white divide-y divide-gray-100",
                div { class: "px-4 py-4",
                    label { class: "block text-sm font-medium text-gray-900 mb-1",
                        "Auto-read threshold"
                    }
                    p { class: "text-xs text-gray-500 mb-3",
                        "Automatically mark a book as Read when progress reaches this percentage."
                    }
                    div { class: "flex items-center gap-4",
                        input {
                            r#type: "range",
                            min: "0",
                            max: "100",
                            value: threshold,
                            class: "flex-1 accent-indigo-600",
                            oninput: move |e| {
                                saved.set(false);
                                if let Ok(v) = e.value().parse::<u8>() {
                                    threshold.set(v);
                                }
                            },
                        }
                        span { class: "text-sm font-medium text-gray-900 w-12 text-right",
                            "{threshold}%"
                        }
                    }
                    div { class: "flex items-center justify-end gap-3 mt-3",
                        if saved() {
                            span { class: "text-xs text-green-600", "Saved!" }
                        }
                        button {
                            class: "px-3 py-1.5 text-sm font-medium rounded bg-indigo-600 text-white hover:bg-indigo-700 disabled:opacity-50",
                            disabled: saving(),
                            onclick: move |_| {
                                let pct = threshold();
                                saving.set(true);
                                saved.set(false);
                                spawn(async move {
                                    if save_auto_read_threshold(pct).await.is_ok() {
                                        saved.set(true);
                                    }
                                    saving.set(false);
                                });
                            },
                            if saving() { "Saving…" } else { "Save" }
                        }
                    }
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
