#[cfg(feature = "server")]
use bb_core::CoreServices;
use dioxus::prelude::*;
#[cfg(feature = "server")]
use {crate::routes::server_helpers::authenticated_user, crate::server::AuthSession, std::sync::Arc};

use crate::components::SystemMessagesRefresh;

// ---------------------------------------------------------------------------
// DTO
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct MessageRow {
    pub id: u64,
    pub source_task: String,
    pub severity: String,
    pub message: String,
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// Server functions
// ---------------------------------------------------------------------------

#[post(
    "/api/v1/admin/health/messages",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn list_system_messages() -> Result<Vec<MessageRow>, ServerFnError> {
    let user = authenticated_user(&auth_session)?;
    if !user.permissions.contains("SuperAdmin") && !user.permissions.contains("Admin") {
        return Err(ServerFnError::new("Insufficient permissions"));
    }

    let messages = core_services
        .system_message_service
        .list_messages()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(messages
        .into_iter()
        .map(|m| MessageRow {
            id: m.id,
            source_task: m.source_task,
            severity: format!("{:?}", m.severity),
            message: m.message,
            created_at: m.created_at.to_rfc3339(),
        })
        .collect())
}

#[post(
    "/api/v1/admin/health/messages/dismiss",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn dismiss_system_message(id: u64) -> Result<(), ServerFnError> {
    let user = authenticated_user(&auth_session)?;
    if !user.permissions.contains("SuperAdmin") && !user.permissions.contains("Admin") {
        return Err(ServerFnError::new("Insufficient permissions"));
    }

    core_services
        .system_message_service
        .delete_message(id)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(())
}

#[post(
    "/api/v1/admin/health/messages/clear",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn clear_all_system_messages() -> Result<(), ServerFnError> {
    let user = authenticated_user(&auth_session)?;
    if !user.permissions.contains("SuperAdmin") && !user.permissions.contains("Admin") {
        return Err(ServerFnError::new("Insufficient permissions"));
    }

    core_services
        .system_message_service
        .delete_all_messages()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// LocalTime — renders ISO 8601 timestamp in the browser's local timezone.
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// MessagesSection component
// ---------------------------------------------------------------------------

#[component]
pub(crate) fn MessagesSection() -> Element {
    let mut refresh = use_signal(|| 0u32);

    // Subscribe to SSE-driven refresh.
    let sse_refresh = use_context::<SystemMessagesRefresh>();

    let messages_resource = use_resource(move || async move {
        let _ = refresh(); // subscribe to manual refresh
        let _ = sse_refresh.0(); // subscribe to SSE refresh
        list_system_messages().await
    });

    rsx! {
        div { class: "w-full max-w-3xl",
            div { class: "flex items-center justify-between mb-6",
                h2 { class: "text-lg font-semibold text-gray-900", "System Messages" }
                button {
                    class: "px-3 py-1.5 text-sm font-medium rounded bg-red-50 text-red-700 hover:bg-red-100",
                    onclick: move |_| {
                        spawn(async move {
                            let _ = clear_all_system_messages().await;
                            *refresh.write() += 1;
                        });
                    },
                    "Clear All"
                }
            }

            match messages_resource() {
                None => rsx! {
                    div { class: "text-gray-400 text-sm", "Loading..." }
                },
                Some(Err(e)) => rsx! {
                    div { class: "p-3 bg-red-50 border border-red-200 text-red-700 rounded-lg text-sm",
                        "{e}"
                    }
                },
                Some(Ok(rows)) if rows.is_empty() => rsx! {
                    div { class: "rounded-lg border border-gray-200 bg-white p-8 text-center text-gray-400 text-sm",
                        "No system messages."
                    }
                },
                Some(Ok(rows)) => rsx! {
                    div { class: "space-y-2",
                        for row in rows {
                            {
                                let msg_id = row.id;
                                rsx! {
                                    div {
                                        class: match row.severity.as_str() {
                                            "Error" => "flex items-start gap-3 p-3 rounded-lg border border-red-200 bg-red-50",
                                            "Warning" => "flex items-start gap-3 p-3 rounded-lg border border-yellow-200 bg-yellow-50",
                                            _ => "flex items-start gap-3 p-3 rounded-lg border border-blue-200 bg-blue-50",
                                        },
                                        // Severity badge
                                        span {
                                            class: match row.severity.as_str() {
                                                "Error" => "shrink-0 inline-flex items-center px-2 py-0.5 rounded text-xs font-medium bg-red-100 text-red-800",
                                                "Warning" => "shrink-0 inline-flex items-center px-2 py-0.5 rounded text-xs font-medium bg-yellow-100 text-yellow-800",
                                                _ => "shrink-0 inline-flex items-center px-2 py-0.5 rounded text-xs font-medium bg-blue-100 text-blue-800",
                                            },
                                            "{row.severity}"
                                        }
                                        // Message body
                                        div { class: "flex-1 min-w-0",
                                            p { class: "text-sm text-gray-900", "{row.message}" }
                                            p { class: "text-xs text-gray-500 mt-0.5",
                                                "{row.source_task} — "
                                                LocalTime { iso: row.created_at.clone() }
                                            }
                                        }
                                        // Dismiss button
                                        button {
                                            class: "shrink-0 p-1 text-gray-400 hover:text-gray-600 rounded hover:bg-white/50",
                                            title: "Dismiss",
                                            onclick: move |_| {
                                                spawn(async move {
                                                    let _ = dismiss_system_message(msg_id).await;
                                                    *refresh.write() += 1;
                                                });
                                            },
                                            "x"
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
