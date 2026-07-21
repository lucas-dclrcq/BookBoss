//! Settings > Users panel to configure the defaults applied to accounts that
//! are auto-provisioned on first OIDC login. The master on/off switch lives in
//! the `BOOKBOSS__OIDC__AUTO_PROVISION` env var (surfaced read-only here); this
//! panel only edits the role/capabilities/libraries/default-library defaults,
//! persisted via `AppSettingService`.

#[cfg(feature = "server")]
use bb_core::{CoreServices, types::Capability};
use dioxus::prelude::*;
#[cfg(feature = "server")]
use {
    crate::routes::server_helpers::authenticated_user,
    crate::server::{AuthSession, AutoProvisionEnabled},
    std::sync::Arc,
};

use super::users_section::{LibraryAssignRow, list_all_libraries_simple};

// ---------------------------------------------------------------------------
// DTO
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct OidcProvisioningSettings {
    /// Whether `BOOKBOSS__OIDC__AUTO_PROVISION` is enabled (read-only status).
    pub enabled_via_env: bool,
    /// Capability names (`Capability::as_str`) granted to provisioned users.
    pub capabilities: Vec<String>,
    /// Library tokens assigned to provisioned users.
    pub library_tokens: Vec<String>,
    /// Default library token (empty when unset).
    pub default_library: String,
}

// ---------------------------------------------------------------------------
// Server functions
// ---------------------------------------------------------------------------

#[post(
    "/api/v1/settings/oidc-provisioning",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>,
    auto_provision: axum::Extension<AutoProvisionEnabled>
)]
pub(crate) async fn get_oidc_provisioning_settings() -> Result<OidcProvisioningSettings, ServerFnError> {
    let user = authenticated_user(&auth_session)?;
    if !user.permissions.contains("SuperAdmin") && !user.permissions.contains("Admin") {
        return Err(ServerFnError::new("Insufficient permissions"));
    }

    let AutoProvisionEnabled(enabled_via_env) = *auto_provision;

    let defaults = core_services
        .app_setting_service
        .oidc_provisioning_defaults()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    let capabilities = defaults.capabilities.iter().map(|c| c.as_str().to_string()).collect();

    Ok(OidcProvisioningSettings {
        enabled_via_env,
        capabilities,
        library_tokens: defaults.library_tokens,
        default_library: defaults.default_library.unwrap_or_default(),
    })
}

#[post(
    "/api/v1/settings/oidc-provisioning/save",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn set_oidc_provisioning_settings(
    capabilities: Vec<String>,
    library_tokens: Vec<String>,
    default_library: String,
) -> Result<(), ServerFnError> {
    use bb_core::app_setting::OidcProvisioningDefaults;

    let actor = authenticated_user(&auth_session)?;
    if !actor.permissions.contains("SuperAdmin") && !actor.permissions.contains("Admin") {
        return Err(ServerFnError::new("Insufficient permissions"));
    }

    // Same role rules as manual user creation: never SuperAdmin; Admin only when
    // the acting admin is a Super Admin.
    let caps = super::users_section::parse_capabilities(&capabilities)?;
    if caps.contains(&Capability::SuperAdmin) {
        return Err(ServerFnError::new("Cannot assign Super Admin role"));
    }
    if caps.contains(&Capability::Admin) && !actor.permissions.contains("SuperAdmin") {
        return Err(ServerFnError::new("Only Super Admin can assign the Admin role"));
    }

    let default_library = {
        let trimmed = default_library.trim();
        if trimmed.is_empty() {
            None
        } else {
            // A default library that isn't among the assigned libraries can never
            // be applied at provision time (set_default_library requires the user
            // to be assigned to it), so reject it here rather than persist a
            // dangling default.
            if !library_tokens.iter().any(|t| t == trimmed) {
                return Err(ServerFnError::new("Default library must be one of the selected libraries"));
            }
            Some(trimmed.to_string())
        }
    };

    let defaults = OidcProvisioningDefaults {
        capabilities: caps,
        library_tokens,
        default_library,
    };

    core_services
        .app_setting_service
        .set_oidc_provisioning_defaults(&defaults)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// OidcProvisioningPanel component
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum RoleChoice {
    Admin,
    User,
}

const GRANTABLE: [(&str, &str); 5] = [
    ("ApproveImports", "Approve Imports"),
    ("ConvertBook", "Convert Books"),
    ("DeleteBook", "Delete Books"),
    ("EditBook", "Edit Books"),
    ("OpdsAccess", "OPDS Access"),
];

#[component]
pub(crate) fn OidcProvisioningPanel(is_super_admin: bool) -> Element {
    let mut enabled_via_env = use_signal(|| false);
    let mut role = use_signal(|| RoleChoice::User);
    let mut user_caps: Signal<Vec<String>> = use_signal(Vec::new);
    let mut checked_library_tokens: Signal<Vec<String>> = use_signal(Vec::new);
    let mut default_library_token = use_signal(String::new);
    let mut saving = use_signal(|| false);
    let mut saved = use_signal(|| false);
    let mut error_msg: Signal<Option<String>> = use_signal(|| None);

    let settings_resource = use_resource(get_oidc_provisioning_settings);
    // Only fetch libraries once we know provisioning is enabled.
    let libraries_resource = use_resource(move || async move { if enabled_via_env() { Some(list_all_libraries_simple().await) } else { None } });

    // Populate the form once the stored defaults load.
    use_effect(move || {
        if let Some(Ok(s)) = settings_resource() {
            enabled_via_env.set(s.enabled_via_env);
            let is_admin_role = s.capabilities.iter().any(|c| c == "Admin");
            role.set(if is_admin_role { RoleChoice::Admin } else { RoleChoice::User });
            user_caps.set(
                s.capabilities
                    .iter()
                    .filter(|c| c.as_str() != "Admin" && c.as_str() != "SuperAdmin")
                    .cloned()
                    .collect(),
            );
            checked_library_tokens.set(s.library_tokens.clone());
            default_library_token.set(s.default_library.clone());
        }
    });

    // Only surface the panel when auto-provisioning is enabled via the env var.
    // Hidden (fail-closed) until the stored settings confirm it is enabled.
    if !enabled_via_env() {
        return rsx! {};
    }

    rsx! {
        div { class: "w-full max-w-3xl mb-8",
            div { class: "rounded-lg border border-gray-200 bg-white p-6 dark:border-slate-700 dark:bg-slate-800",
                div { class: "mb-4",
                    h3 { class: "text-base font-semibold text-gray-900 dark:text-slate-100", "OIDC Auto-Provisioning" }
                    p { class: "text-sm text-gray-500 mt-0.5 dark:text-slate-400",
                        "Defaults applied to accounts created automatically the first time a user signs in via SSO with an email that has no existing account."
                    }
                }

                // Env-gate status (the panel only renders when enabled).
                div { class: "mb-4 flex items-center gap-2 text-sm text-green-700 dark:text-green-400",
                    span { "✓" }
                    span { "Auto-provisioning is enabled via " code { "BOOKBOSS__OIDC__AUTO_PROVISION" } "." }
                }

                // Role
                div { class: "mb-4",
                    label { class: "block text-sm font-medium text-gray-700 mb-1 dark:text-slate-300", "Role" }
                    select {
                        class: "w-full px-3 py-2 border border-gray-300 rounded-lg focus:outline-hidden focus:ring-2 focus:ring-indigo-500 text-sm bg-white dark:bg-slate-700 dark:border-slate-600 dark:text-slate-100",
                        disabled: saving(),
                        onchange: move |e| {
                            let new_role = if e.value() == "Admin" { RoleChoice::Admin } else { RoleChoice::User };
                            if new_role == RoleChoice::User {
                                user_caps.set(Vec::new());
                            }
                            role.set(new_role);
                            saved.set(false);
                        },
                        option { value: "User", selected: role() == RoleChoice::User, "User" }
                        option {
                            value: "Admin",
                            selected: role() == RoleChoice::Admin,
                            disabled: !is_super_admin,
                            "Admin"
                        }
                    }
                }

                // Capabilities (User role only)
                if role() == RoleChoice::User {
                    div { class: "mb-4",
                        label { class: "block text-sm font-medium text-gray-700 mb-2 dark:text-slate-300", "Capabilities" }
                        div { class: "space-y-2 rounded-lg border border-gray-200 p-3 dark:border-slate-700",
                            for (cap_key, cap_label) in GRANTABLE {
                                {
                                    let key = cap_key.to_string();
                                    let key_remove = key.clone();
                                    let is_checked = user_caps().contains(&key);
                                    rsx! {
                                        label { class: "flex items-center gap-2 cursor-pointer select-none",
                                            input {
                                                r#type: "checkbox",
                                                class: "rounded border-gray-300 text-indigo-600 focus:ring-indigo-500",
                                                checked: is_checked,
                                                disabled: saving(),
                                                onchange: move |e| {
                                                    if e.checked() {
                                                        user_caps.write().push(key.clone());
                                                    } else {
                                                        user_caps.write().retain(|c| c != &key_remove);
                                                    }
                                                    saved.set(false);
                                                },
                                            }
                                            span { class: "text-sm text-gray-700 dark:text-slate-300", "{cap_label}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Libraries
                div { class: "mb-4",
                    label { class: "block text-sm font-medium text-gray-700 mb-2 dark:text-slate-300", "Libraries" }
                    match libraries_resource() {
                        None | Some(None) => rsx! {
                            div { class: "text-gray-400 text-xs dark:text-slate-500", "Loading libraries…" }
                        },
                        Some(Some(Err(e))) => rsx! {
                            div { class: "p-2 bg-red-50 border border-red-200 text-red-700 rounded text-xs dark:bg-red-900/30 dark:border-red-800 dark:text-red-400",
                                "{e}"
                            }
                        },
                        Some(Some(Ok(libs))) => {
                            let checked = checked_library_tokens();
                            let all_checked: Vec<LibraryAssignRow> =
                                libs.iter().filter(|l| checked.contains(&l.token)).cloned().collect();
                            rsx! {
                                div { class: "rounded-lg border border-gray-200 p-3 space-y-1.5 max-h-40 overflow-y-auto dark:border-slate-600",
                                    if libs.is_empty() {
                                        div { class: "text-xs text-gray-400 dark:text-slate-500", "No libraries configured." }
                                    }
                                    for lib in &libs {
                                        {
                                            let tok = lib.token.clone();
                                            let tok_remove = tok.clone();
                                            let tok_def = tok.clone();
                                            let is_checked = checked_library_tokens().contains(&tok);
                                            rsx! {
                                                label { class: "flex items-center gap-2 cursor-pointer select-none",
                                                    input {
                                                        r#type: "checkbox",
                                                        class: "rounded border-gray-300 text-indigo-600 focus:ring-indigo-500",
                                                        checked: is_checked,
                                                        disabled: saving(),
                                                        onchange: move |e| {
                                                            if e.checked() {
                                                                checked_library_tokens.write().push(tok.clone());
                                                                if default_library_token().is_empty() {
                                                                    default_library_token.set(tok.clone());
                                                                }
                                                            } else {
                                                                checked_library_tokens.write().retain(|t| t != &tok_remove);
                                                                if default_library_token() == tok_def {
                                                                    default_library_token.set(String::new());
                                                                }
                                                            }
                                                            saved.set(false);
                                                        },
                                                    }
                                                    span { class: "text-sm text-gray-700 dark:text-slate-300", "{lib.name}" }
                                                    if lib.is_system {
                                                        span { class: "text-xs text-blue-500 font-medium dark:text-blue-400", "(system)" }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }

                                // Default library picker
                                if !all_checked.is_empty() {
                                    div { class: "mt-3",
                                        label { class: "block text-xs font-medium text-gray-600 mb-1 dark:text-slate-400", "Default Library" }
                                        select {
                                            class: "w-full px-3 py-2 border border-gray-300 rounded-lg focus:outline-hidden focus:ring-2 focus:ring-indigo-500 text-sm bg-white dark:bg-slate-700 dark:border-slate-600 dark:text-slate-100",
                                            disabled: saving(),
                                            value: default_library_token,
                                            onchange: move |e| {
                                                default_library_token.set(e.value());
                                                saved.set(false);
                                            },
                                            option { value: "", disabled: true, selected: default_library_token().is_empty(), "Select default…" }
                                            for lib in &all_checked {
                                                option {
                                                    value: lib.token.clone(),
                                                    selected: default_library_token() == lib.token,
                                                    "{lib.name}"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Inline error
                if let Some(ref msg) = error_msg() {
                    div { class: "mb-4 p-3 bg-red-50 border border-red-200 text-red-700 rounded-lg text-sm dark:bg-red-900/30 dark:border-red-800 dark:text-red-400",
                        "{msg}"
                    }
                }

                // Actions
                div { class: "flex items-center justify-end gap-3 pt-2",
                    if saved() {
                        span { class: "text-sm text-green-600 dark:text-green-400", "Saved ✓" }
                    }
                    button {
                        class: "px-4 py-2 text-sm font-medium rounded-lg bg-indigo-600 text-white hover:bg-indigo-700 disabled:opacity-50",
                        disabled: saving(),
                        onclick: move |_| {
                            let capabilities: Vec<String> = match role() {
                                RoleChoice::Admin => vec!["Admin".to_string()],
                                RoleChoice::User => user_caps(),
                            };
                            let lib_tokens = checked_library_tokens();
                            let def_lib = default_library_token();
                            error_msg.set(None);
                            saved.set(false);
                            saving.set(true);
                            spawn(async move {
                                match set_oidc_provisioning_settings(capabilities, lib_tokens, def_lib).await {
                                    Ok(()) => {
                                        saving.set(false);
                                        saved.set(true);
                                    }
                                    Err(ServerFnError::ServerError { message, .. }) => {
                                        error_msg.set(Some(message));
                                        saving.set(false);
                                    }
                                    Err(e) => {
                                        error_msg.set(Some(e.to_string()));
                                        saving.set(false);
                                    }
                                }
                            });
                        },
                        if saving() { "Saving…" } else { "Save" }
                    }
                }
            }
        }
    }
}
