#[cfg(feature = "server")]
use bb_core::health::HealthTaskState;
use dioxus::prelude::*;
#[cfg(feature = "server")]
use {crate::routes::server_helpers::authenticated_user, crate::server::AuthSession, std::sync::Arc};

// ---------------------------------------------------------------------------
// DTO
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct TaskRow {
    pub name: String,
    pub job_type: String,
    pub run_on_startup: bool,
    pub interval_minutes: u64,
    pub last_run_at: Option<String>,
    pub next_run_at: String,
}

// ---------------------------------------------------------------------------
// Server functions
// ---------------------------------------------------------------------------

#[post(
    "/api/v1/admin/health/tasks",
    auth_session: axum::Extension<AuthSession>,
    health_task_state: axum::Extension<Arc<HealthTaskState>>
)]
pub(crate) async fn list_health_tasks() -> Result<Vec<TaskRow>, ServerFnError> {
    let user = authenticated_user(&auth_session)?;
    if !user.permissions.contains("SuperAdmin") && !user.permissions.contains("Admin") {
        return Err(ServerFnError::new("Insufficient permissions"));
    }

    let tasks = health_task_state.list_tasks().await;
    Ok(tasks
        .into_iter()
        .map(|t| TaskRow {
            name: t.name,
            job_type: t.job_type,
            run_on_startup: t.run_on_startup,
            interval_minutes: t.interval_minutes,
            last_run_at: t.last_run_at.map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()),
            next_run_at: t.next_run_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        })
        .collect())
}

#[post(
    "/api/v1/admin/health/tasks/trigger",
    auth_session: axum::Extension<AuthSession>,
    health_task_state: axum::Extension<Arc<HealthTaskState>>
)]
pub(crate) async fn trigger_health_task(job_type: String) -> Result<(), ServerFnError> {
    let user = authenticated_user(&auth_session)?;
    if !user.permissions.contains("SuperAdmin") && !user.permissions.contains("Admin") {
        return Err(ServerFnError::new("Insufficient permissions"));
    }

    // Verify the job_type is a known health task.
    let tasks = health_task_state.list_tasks().await;
    if !tasks.iter().any(|t| t.job_type == job_type) {
        return Err(ServerFnError::new("Unknown health task"));
    }

    // Mark the task as due immediately so the subsystem's poll loop picks it up.
    health_task_state.mark_due_now(&job_type).await;

    Ok(())
}

// ---------------------------------------------------------------------------
// TasksSection component
// ---------------------------------------------------------------------------

#[component]
pub(crate) fn TasksSection() -> Element {
    let mut refresh = use_signal(|| 0u32);

    let tasks_resource = use_resource(move || async move {
        let _ = refresh(); // subscribe
        list_health_tasks().await
    });

    rsx! {
        div { class: "w-full max-w-3xl",
            div { class: "flex items-center justify-between mb-6",
                h2 { class: "text-lg font-semibold text-gray-900", "Health Tasks" }
            }

            match tasks_resource() {
                None => rsx! {
                    div { class: "text-gray-400 text-sm", "Loading..." }
                },
                Some(Err(e)) => rsx! {
                    div { class: "p-3 bg-red-50 border border-red-200 text-red-700 rounded-lg text-sm",
                        "{e}"
                    }
                },
                Some(Ok(rows)) => rsx! {
                    div { class: "rounded-lg border border-gray-200 bg-white overflow-hidden",
                        table { class: "w-full text-sm",
                            thead {
                                tr { class: "bg-gray-50 border-b border-gray-200",
                                    th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wide", "Task" }
                                    th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wide", "Interval" }
                                    th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wide", "Last Run" }
                                    th { class: "px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wide", "Next Run" }
                                    th { class: "px-4 py-3 text-right text-xs font-medium text-gray-500 uppercase tracking-wide", "Actions" }
                                }
                            }
                            tbody { class: "divide-y divide-gray-100",
                                for row in rows {
                                    {
                                        let jt = row.job_type.clone();
                                        rsx! {
                                            tr { class: "hover:bg-gray-50",
                                                td { class: "px-4 py-3",
                                                    div { class: "font-medium text-gray-900", "{row.name}" }
                                                    if row.run_on_startup {
                                                        span { class: "inline-flex items-center px-1.5 py-0.5 rounded text-xs font-medium bg-blue-100 text-blue-700 mt-0.5",
                                                            "startup"
                                                        }
                                                    }
                                                }
                                                td { class: "px-4 py-3 text-gray-600", "{format_interval(row.interval_minutes)}" }
                                                td { class: "px-4 py-3 text-gray-600",
                                                    if let Some(ref last) = row.last_run_at {
                                                        "{last}"
                                                    } else {
                                                        span { class: "text-gray-400", "Never" }
                                                    }
                                                }
                                                td { class: "px-4 py-3 text-gray-600", "{row.next_run_at}" }
                                                td { class: "px-4 py-3",
                                                    div { class: "flex items-center justify-end",
                                                        RunNowButton { job_type: jt, on_triggered: move || *refresh.write() += 1 }
                                                    }
                                                }
                                            }
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

#[component]
fn RunNowButton(job_type: String, on_triggered: EventHandler<()>) -> Element {
    let mut running = use_signal(|| false);

    rsx! {
        button {
            class: "px-2.5 py-1 text-xs font-medium rounded bg-indigo-50 text-indigo-700 hover:bg-indigo-100 disabled:opacity-50",
            disabled: running(),
            onclick: move |_| {
                let jt = job_type.clone();
                running.set(true);
                spawn(async move {
                    let _ = trigger_health_task(jt).await;
                    on_triggered.call(());
                    running.set(false);
                });
            },
            if running() { "Running..." } else { "Run Now" }
        }
    }
}

fn format_interval(minutes: u64) -> String {
    if minutes >= 1440 && minutes % 1440 == 0 {
        let days = minutes / 1440;
        if days == 1 { "1 day".to_string() } else { format!("{days} days") }
    } else if minutes >= 60 && minutes % 60 == 0 {
        let hours = minutes / 60;
        if hours == 1 { "1 hour".to_string() } else { format!("{hours} hours") }
    } else if minutes == 1 {
        "1 minute".to_string()
    } else {
        format!("{minutes} minutes")
    }
}
