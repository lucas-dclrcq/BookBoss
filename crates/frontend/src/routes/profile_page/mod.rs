mod devices_section;

use devices_section::DevicesSectionContent;
use dioxus::prelude::*;
#[cfg(feature = "server")]
use {
    crate::routes::server_helpers::authenticated_user,
    crate::server::{AuthSession, kobo::KoboConfig},
    bb_core::{
        CoreServices,
        device::{DeviceToken, OnRemovalAction},
        reading::{AUTO_READ_THRESHOLD_KEY, DEFAULT_AUTO_READ_THRESHOLD},
        types::EmailAddress,
        user::User,
    },
    chrono::{DateTime, Utc},
    std::str::FromStr,
    std::sync::Arc,
};

use crate::Route;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct ProfileInfo {
    full_name: String,
    email: String,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct ReadingSettings {
    auto_read_threshold_pct: u8,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct DeviceRow {
    token: String,
    name: String,
    device_type: String,
    on_removal_action: String,
    sync_token_display: String,
    sync_url: String,
    last_synced_at: String,
    companion_shelf_name: Option<String>,
    companion_shelf_token: Option<String>,
}

// ---------------------------------------------------------------------------
// Server functions
// ---------------------------------------------------------------------------

#[get(
    "/api/v1/profile/context",
    auth_session: axum::Extension<AuthSession>
)]
async fn get_profile_context() -> Result<(), ServerFnError> {
    authenticated_user(&auth_session)?;
    Ok(())
}

#[get(
    "/api/v1/profile/info",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
async fn get_profile_info() -> Result<ProfileInfo, ServerFnError> {
    let user_id = authenticated_user(&auth_session)?.id();

    let user = core_services
        .user_service
        .find_by_id(user_id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .ok_or_else(|| ServerFnError::new("User not found"))?;

    Ok(ProfileInfo {
        full_name: user.full_name.clone(),
        email: user.email_address.to_string(),
    })
}

#[post(
    "/api/v1/profile/update",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
async fn update_profile(full_name: String, email: String) -> Result<(), ServerFnError> {
    let full_name = full_name.trim().to_string();
    if full_name.is_empty() {
        return Err(ServerFnError::new("Full name must not be empty"));
    }
    let email_address = EmailAddress::new(&email).map_err(|e| ServerFnError::new(e.to_string()))?;

    let user_id = authenticated_user(&auth_session)?.id();

    let existing = core_services
        .user_service
        .find_by_id(user_id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .ok_or_else(|| ServerFnError::new("User not found"))?;

    core_services
        .user_service
        .update_user(User {
            full_name,
            email_address,
            ..existing
        })
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(())
}

#[get(
    "/api/v1/settings/reading",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
async fn get_reading_settings() -> Result<ReadingSettings, ServerFnError> {
    let user_id = authenticated_user(&auth_session)?.id();
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
    let user_id = authenticated_user(&auth_session)?.id();

    if threshold_pct > 100 {
        return Err(ServerFnError::new("Threshold must be between 0 and 100"));
    }
    let bps = u16::from(threshold_pct) * 100;
    core_services
        .user_setting_service
        .set(user_id, AUTO_READ_THRESHOLD_KEY, &bps.to_string())
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(())
}

#[post(
    "/api/v1/profile/change-password",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
async fn change_password(current: String, new_password: String) -> Result<(), ServerFnError> {
    if new_password.trim().is_empty() {
        return Err(ServerFnError::new("New password must not be empty"));
    }

    let user_id = authenticated_user(&auth_session)?.id();

    let existing = core_services
        .user_service
        .find_by_id(user_id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .ok_or_else(|| ServerFnError::new("User not found"))?;

    if !existing.check_password(&current) {
        return Err(ServerFnError::new("Current password is incorrect"));
    }

    let new_hash = User::encrypt_password(&new_password).map_err(|e| ServerFnError::new(e.to_string()))?;

    core_services
        .user_service
        .update_user(User {
            password_hash: new_hash,
            ..existing
        })
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Device server functions
// ---------------------------------------------------------------------------

#[cfg(feature = "server")]
fn removal_action_to_str(a: &OnRemovalAction) -> &'static str {
    match a {
        OnRemovalAction::Nothing => "nothing",
        OnRemovalAction::MarkRead => "mark_read",
        OnRemovalAction::MarkDnf => "mark_dnf",
    }
}

#[cfg(feature = "server")]
fn parse_removal_action(s: &str) -> Result<OnRemovalAction, ServerFnError> {
    match s {
        "nothing" => Ok(OnRemovalAction::Nothing),
        "mark_read" => Ok(OnRemovalAction::MarkRead),
        "mark_dnf" => Ok(OnRemovalAction::MarkDnf),
        _ => Err(ServerFnError::new("Invalid on_removal_action")),
    }
}

#[get(
    "/api/v1/profile/devices",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>,
    kobo_config: axum::Extension<Arc<KoboConfig>>
)]
async fn get_devices_for_profile() -> Result<Vec<DeviceRow>, ServerFnError> {
    let user_id = authenticated_user(&auth_session)?.id();

    let devices = core_services
        .device_service
        .list_devices_for_user(user_id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    let mut rows = Vec::with_capacity(devices.len());
    for device in devices {
        let companion = core_services
            .device_service
            .get_companion_shelf(device.id)
            .await
            .map_err(|e| ServerFnError::new(e.to_string()))?;

        let sync_token_display = device.token.to_string().trim_start_matches("DV_").to_string();
        let sync_url = kobo_config.sync_url(&sync_token_display);
        let last_synced_at = humanize_synced_at(device.last_synced_at);

        rows.push(DeviceRow {
            token: device.token.to_string(),
            name: device.name,
            device_type: device.device_type,
            on_removal_action: removal_action_to_str(&device.on_removal_action).to_string(),
            sync_token_display,
            sync_url,
            last_synced_at,
            companion_shelf_name: companion.as_ref().map(|s| s.name.clone()),
            companion_shelf_token: companion.as_ref().map(|s| s.token.to_string()),
        });
    }
    Ok(rows)
}

#[cfg(feature = "server")]
fn humanize_synced_at(ts: Option<DateTime<Utc>>) -> String {
    let Some(ts) = ts else { return "Never".to_string() };
    let secs = (Utc::now() - ts).num_seconds().max(0);
    match secs {
        s if s < 60 => "Just now".to_string(),
        s if s < 3600 => format!("{} min ago", s / 60),
        s if s < 86400 => format!("{} hr ago", s / 3600),
        s if s < 604_800 => format!("{} days ago", s / 86400),
        _ => ts.format("%Y-%m-%d").to_string(),
    }
}

#[get(
    "/api/v1/profile/devices/default-name",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
async fn get_default_device_name() -> Result<String, ServerFnError> {
    let user_id = authenticated_user(&auth_session)?.id();

    core_services
        .device_service
        .default_device_name(user_id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))
}

#[post(
    "/api/v1/profile/devices/create",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
async fn create_device_for_profile(name: String, device_type: String, on_removal_action: String) -> Result<(), ServerFnError> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(ServerFnError::new("Device name must not be empty"));
    }

    let user_id = authenticated_user(&auth_session)?.id();

    let action = parse_removal_action(&on_removal_action)?;

    core_services
        .device_service
        .create_device(user_id, name, device_type, action)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(())
}

#[post(
    "/api/v1/profile/devices/update",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
async fn update_device_for_profile(token: String, name: String, on_removal_action: String) -> Result<(), ServerFnError> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(ServerFnError::new("Device name must not be empty"));
    }

    let user_id = authenticated_user(&auth_session)?.id();

    let device_token = DeviceToken::from_str(&token).map_err(|e| ServerFnError::new(e.to_string()))?;
    let action = parse_removal_action(&on_removal_action)?;

    core_services
        .device_service
        .update_device(&device_token, name, action, user_id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(())
}

#[post(
    "/api/v1/profile/devices/delete",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
async fn delete_device_for_profile(token: String, delete_shelf: bool) -> Result<(), ServerFnError> {
    let user_id = authenticated_user(&auth_session)?.id();

    let device_token = DeviceToken::from_str(&token).map_err(|e| ServerFnError::new(e.to_string()))?;

    core_services
        .device_service
        .delete_device(&device_token, delete_shelf, user_id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(())
}

#[post(
    "/api/v1/profile/devices/reset-sync",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
async fn reset_device_sync_for_profile(token: String) -> Result<(), ServerFnError> {
    let user_id = authenticated_user(&auth_session)?.id();

    let device_token = DeviceToken::from_str(&token).map_err(|e| ServerFnError::new(e.to_string()))?;

    core_services
        .device_service
        .reset_device_sync(&device_token, user_id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

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
                    ProfileSectionContent {}
                }

                hr { class: "border-gray-200" }

                // ── Reading ───────────────────────────────────────────────
                section {
                    h2 { class: "text-lg font-semibold text-gray-900 mb-4", "Reading" }
                    ReadingSectionContent {}
                }

                hr { class: "border-gray-200" }

                // ── My Devices ────────────────────────────────────────────
                section {
                    DevicesSectionContent {}
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Reading section
// ---------------------------------------------------------------------------

#[component]
fn ReadingSectionContent() -> Element {
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

// ---------------------------------------------------------------------------
// Profile section
// ---------------------------------------------------------------------------

#[component]
fn ProfileSectionContent() -> Element {
    let info = use_server_future(get_profile_info)?;

    // Profile info signals
    let mut full_name = use_signal(String::new);
    let mut email = use_signal(String::new);
    let mut profile_saving = use_signal(|| false);
    let mut profile_saved = use_signal(|| false);
    let mut profile_error: Signal<Option<String>> = use_signal(|| None);

    // Change-password modal signals
    let mut pw_modal_open = use_signal(|| false);
    let mut current_pw = use_signal(String::new);
    let mut new_pw = use_signal(String::new);
    let mut confirm_pw = use_signal(String::new);
    let mut pw_saving = use_signal(|| false);
    let mut pw_error: Signal<Option<String>> = use_signal(|| None);

    use_effect(move || {
        if let Some(Ok(i)) = info() {
            full_name.set(i.full_name.clone());
            email.set(i.email.clone());
        }
    });

    let passwords_match = use_memo(move || confirm_pw().is_empty() || new_pw() == confirm_pw());

    let mut close_pw_modal = move || {
        pw_modal_open.set(false);
        current_pw.set(String::new());
        new_pw.set(String::new());
        confirm_pw.set(String::new());
        pw_error.set(None);
    };

    let input_class = "w-full rounded-md border border-gray-300 px-3 py-1.5 text-sm focus:outline-none focus:ring-1 focus:ring-indigo-500";
    let label_class = "block text-sm font-medium text-gray-700 mb-1";

    rsx! {
        // ── Profile info card ─────────────────────────────────────────────
        div { class: "rounded-lg border border-gray-200 bg-white px-4 py-4 flex flex-col gap-3",
            div {
                label { class: label_class, "Full Name" }
                input {
                    r#type: "text",
                    class: input_class,
                    value: full_name,
                    oninput: move |e| {
                        profile_saved.set(false);
                        full_name.set(e.value());
                    },
                }
            }
            div {
                label { class: label_class, "Email" }
                input {
                    r#type: "email",
                    class: input_class,
                    value: email,
                    oninput: move |e| {
                        profile_saved.set(false);
                        email.set(e.value());
                    },
                }
            }
            if let Some(err) = profile_error() {
                p { class: "text-xs text-red-600", "{err}" }
            }
            div { class: "flex items-center justify-between",
                // Left — Change Password trigger
                button {
                    class: "px-3 py-1.5 text-sm font-medium rounded border border-gray-300 text-gray-700 hover:bg-gray-50",
                    onclick: move |_| pw_modal_open.set(true),
                    "Change Password"
                }
                // Right — Save
                div { class: "flex items-center gap-3",
                    if profile_saved() {
                        span { class: "text-xs text-green-600", "Saved!" }
                    }
                    button {
                        class: "px-3 py-1.5 text-sm font-medium rounded bg-indigo-600 text-white hover:bg-indigo-700 disabled:opacity-50",
                        disabled: profile_saving(),
                        onclick: move |_| {
                            let name = full_name();
                            let em = email();
                            profile_saving.set(true);
                            profile_saved.set(false);
                            profile_error.set(None);
                            spawn(async move {
                                match update_profile(name, em).await {
                                    Ok(()) => profile_saved.set(true),
                                    Err(e) => profile_error.set(Some(e.to_string())),
                                }
                                profile_saving.set(false);
                            });
                        },
                        if profile_saving() { "Saving…" } else { "Save" }
                    }
                }
            }
        }

        // ── Change Password modal ─────────────────────────────────────────
        if pw_modal_open() {
            div { class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
                div { class: "bg-white rounded-2xl shadow-xl w-full max-w-sm p-6",
                    h3 { class: "text-base font-semibold text-gray-900 mb-4", "Change Password" }

                    div { class: "flex flex-col gap-3",
                        div {
                            label { class: label_class, "Current password" }
                            input {
                                r#type: "password",
                                class: input_class,
                                value: current_pw,
                                oninput: move |e| current_pw.set(e.value()),
                            }
                        }
                        div {
                            label { class: label_class, "New password" }
                            input {
                                r#type: "password",
                                class: input_class,
                                value: new_pw,
                                oninput: move |e| new_pw.set(e.value()),
                            }
                        }
                        div {
                            label { class: label_class, "Confirm new password" }
                            input {
                                r#type: "password",
                                class: input_class,
                                value: confirm_pw,
                                oninput: move |e| confirm_pw.set(e.value()),
                            }
                            if !passwords_match() {
                                p { class: "text-xs text-red-600 mt-1", "Passwords do not match" }
                            }
                        }
                        if let Some(err) = pw_error() {
                            p { class: "text-xs text-red-600", "{err}" }
                        }
                    }

                    div { class: "flex justify-end gap-3 mt-5",
                        button {
                            class: "px-3 py-1.5 text-sm font-medium rounded border border-gray-300 text-gray-700 hover:bg-gray-50",
                            disabled: pw_saving(),
                            onclick: move |_| close_pw_modal(),
                            "Cancel"
                        }
                        button {
                            class: "px-3 py-1.5 text-sm font-medium rounded bg-indigo-600 text-white hover:bg-indigo-700 disabled:opacity-50",
                            disabled: pw_saving() || !passwords_match() || new_pw().is_empty(),
                            onclick: move |_| {
                                let cur = current_pw();
                                let new = new_pw();
                                pw_saving.set(true);
                                pw_error.set(None);
                                spawn(async move {
                                    match change_password(cur, new).await {
                                        Ok(()) => close_pw_modal(),
                                        Err(e) => pw_error.set(Some(e.to_string())),
                                    }
                                    pw_saving.set(false);
                                });
                            },
                            if pw_saving() { "Saving…" } else { "Change Password" }
                        }
                    }
                }
            }
        }
    }
}
