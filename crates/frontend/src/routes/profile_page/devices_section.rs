use dioxus::prelude::*;

use super::{
    DeviceRow, create_device_for_profile, delete_device_for_profile, get_default_device_name, get_devices_for_profile, reset_device_sync_for_profile,
    update_device_for_profile,
};
use crate::Route;

// ---------------------------------------------------------------------------
// Local types
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
enum ModalMode {
    Add,
    Edit(DeviceRow),
}

fn removal_label(action: &str) -> &'static str {
    match action {
        "mark_read" => "Mark as Read",
        "mark_dnf" => "Mark as DNF",
        _ => "Nothing",
    }
}

fn device_type_label(device_type: &str) -> &'static str {
    match device_type {
        "kobo" => "Kobo",
        _ => "Unknown",
    }
}

// ---------------------------------------------------------------------------
// DevicesSectionContent
// ---------------------------------------------------------------------------

#[component]
pub(super) fn DevicesSectionContent() -> Element {
    let mut devices = use_server_future(get_devices_for_profile)?;

    let mut modal: Signal<Option<ModalMode>> = use_signal(|| None);
    let mut delete_target: Signal<Option<DeviceRow>> = use_signal(|| None);
    let mut delete_shelf_checked = use_signal(|| false);
    let mut delete_saving = use_signal(|| false);
    let mut delete_error: Signal<Option<String>> = use_signal(|| None);
    let mut reset_target: Signal<Option<DeviceRow>> = use_signal(|| None);
    let mut reset_saving = use_signal(|| false);
    let mut reset_error: Signal<Option<String>> = use_signal(|| None);

    let device_list = devices().and_then(|r| r.ok()).unwrap_or_default();

    rsx! {
        // ── Section header ────────────────────────────────────────────────
        div { class: "flex items-center justify-between mb-4",
            h2 { class: "text-lg font-semibold text-gray-900", "My Devices" }
            button {
                class: "px-3 py-1.5 text-sm font-medium rounded bg-indigo-600 text-white hover:bg-indigo-700",
                onclick: move |_| modal.set(Some(ModalMode::Add)),
                "+ Add Device"
            }
        }

        // ── Device cards ──────────────────────────────────────────────────
        div { class: "flex flex-col gap-3",
            if device_list.is_empty() {
                p { class: "text-sm text-gray-500", "No devices registered yet." }
            }
            for device in device_list.iter() {
                {
                    let d = device.clone();
                    let d_edit = device.clone();
                    let d_delete = device.clone();
                    let d_reset = device.clone();
                    rsx! {
                        DeviceCard {
                            device: d,
                            on_edit: move |_| modal.set(Some(ModalMode::Edit(d_edit.clone()))),
                            on_delete: move |_| {
                                delete_shelf_checked.set(false);
                                delete_error.set(None);
                                delete_target.set(Some(d_delete.clone()));
                            },
                            on_reset: move |_| {
                                reset_error.set(None);
                                reset_target.set(Some(d_reset.clone()));
                            },
                        }
                    }
                }
            }
        }

        // ── Add / Edit modal ──────────────────────────────────────────────
        if modal().is_some() {
            DeviceModal {
                mode: modal().unwrap(),
                on_close: move |_| modal.set(None),
                on_saved: move |_| {
                    modal.set(None);
                    devices.restart();
                },
            }
        }

        // ── Force-resync confirmation dialog ──────────────────────────────
        if let Some(target) = reset_target() {
            div { class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
                div { class: "bg-white rounded-2xl shadow-xl w-full max-w-sm p-6",
                    h3 { class: "text-base font-semibold text-gray-900 mb-2",
                        "Force resync \"{target.name}\"?"
                    }
                    p { class: "text-sm text-gray-500 mb-4",
                        "Clears the sync state so all books re-download on the next Kobo sync. \
                         Trigger a sync on your Kobo after confirming."
                    }
                    if let Some(err) = reset_error() {
                        p { class: "text-xs text-red-600 mb-3", "{err}" }
                    }
                    div { class: "flex justify-end gap-3",
                        button {
                            class: "px-3 py-1.5 text-sm font-medium rounded border border-gray-300 text-gray-700 hover:bg-gray-50",
                            disabled: reset_saving(),
                            onclick: move |_| reset_target.set(None),
                            "Cancel"
                        }
                        button {
                            class: "px-3 py-1.5 text-sm font-medium rounded bg-indigo-600 text-white hover:bg-indigo-700 disabled:opacity-50",
                            disabled: reset_saving(),
                            onclick: move |_| {
                                let tok = target.token.clone();
                                reset_saving.set(true);
                                reset_error.set(None);
                                spawn(async move {
                                    match reset_device_sync_for_profile(tok).await {
                                        Ok(()) => {
                                            reset_target.set(None);
                                            devices.restart();
                                        }
                                        Err(e) => reset_error.set(Some(e.to_string())),
                                    }
                                    reset_saving.set(false);
                                });
                            },
                            if reset_saving() { "Resetting…" } else { "Force Resync" }
                        }
                    }
                }
            }
        }

        // ── Delete confirmation dialog ─────────────────────────────────────
        if let Some(target) = delete_target() {
            div { class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
                div { class: "bg-white rounded-2xl shadow-xl w-full max-w-sm p-6",
                    h3 { class: "text-base font-semibold text-gray-900 mb-2",
                        "Delete device \"{target.name}\"?"
                    }
                    p { class: "text-sm text-gray-500 mb-4",
                        "This action cannot be undone."
                    }

                    if target.companion_shelf_name.is_some() {
                        label { class: "flex items-center gap-2 text-sm text-gray-700 mb-4 cursor-pointer",
                            input {
                                r#type: "checkbox",
                                class: "rounded border-gray-300 text-indigo-600",
                                checked: delete_shelf_checked(),
                                onchange: move |e| delete_shelf_checked.set(e.checked()),
                            }
                            {
                                let shelf_name = target.companion_shelf_name.clone().unwrap_or_default();
                                rsx! { "Also delete companion shelf \"{shelf_name}\"" }
                            }
                        }
                    }

                    if let Some(err) = delete_error() {
                        p { class: "text-xs text-red-600 mb-3", "{err}" }
                    }

                    div { class: "flex justify-end gap-3",
                        button {
                            class: "px-3 py-1.5 text-sm font-medium rounded border border-gray-300 text-gray-700 hover:bg-gray-50",
                            disabled: delete_saving(),
                            onclick: move |_| delete_target.set(None),
                            "Cancel"
                        }
                        button {
                            class: "px-3 py-1.5 text-sm font-medium rounded bg-red-600 text-white hover:bg-red-700 disabled:opacity-50",
                            disabled: delete_saving(),
                            onclick: move |_| {
                                let tok = target.token.clone();
                                let del_shelf = delete_shelf_checked();
                                delete_saving.set(true);
                                delete_error.set(None);
                                spawn(async move {
                                    match delete_device_for_profile(tok, del_shelf).await {
                                        Ok(()) => {
                                            delete_target.set(None);
                                            devices.restart();
                                        }
                                        Err(e) => delete_error.set(Some(e.to_string())),
                                    }
                                    delete_saving.set(false);
                                });
                            },
                            if delete_saving() { "Deleting…" } else { "Delete" }
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// DeviceCard
// ---------------------------------------------------------------------------

#[component]
fn DeviceCard(device: DeviceRow, on_edit: EventHandler<()>, on_delete: EventHandler<()>, on_reset: EventHandler<()>) -> Element {
    let shelf_token = device.companion_shelf_token.clone();
    let mut copied = use_signal(|| false);

    rsx! {
        div { class: "rounded-lg border border-gray-200 bg-white px-4 py-3 flex flex-col gap-2",
            // Row 1: name + type badge + actions
            div { class: "flex items-center justify-between",
                div { class: "flex items-center gap-2",
                    span { class: "text-sm font-semibold text-gray-900", "{device.name}" }
                    span { class: "px-1.5 py-0.5 text-xs font-medium rounded bg-gray-100 text-gray-600",
                        { device_type_label(&device.device_type) }
                    }
                }
                div { class: "flex items-center gap-1",
                    button {
                        class: "p-1.5 text-gray-500 hover:text-indigo-600 hover:bg-indigo-50 rounded",
                        title: "Edit",
                        onclick: move |_| on_edit(()),
                        "✎"
                    }
                    button {
                        class: "p-1.5 text-gray-500 hover:text-red-600 hover:bg-red-50 rounded",
                        title: "Delete",
                        onclick: move |_| on_delete(()),
                        "✕"
                    }
                }
            }

            // Row 2: companion shelf link
            div { class: "text-xs text-gray-500",
                if let Some(shelf_name) = &device.companion_shelf_name {
                    span { "Companion shelf: " }
                    if let Some(tok) = shelf_token {
                        Link {
                            class: "text-indigo-600 hover:underline",
                            to: Route::ShelfPage { token: tok },
                            { shelf_name.clone() }
                        }
                    } else {
                        span { class: "text-gray-700", { shelf_name.clone() } }
                    }
                } else {
                    span { "No companion shelf" }
                }
            }

            // Row 3: on removal · last synced · sync token (click to copy URL)
            div { class: "flex items-center gap-4 text-xs text-gray-500",
                span {
                    span { "On removal: " }
                    span { class: "text-gray-700", { removal_label(&device.on_removal_action) } }
                }
                span { "·" }
                {
                    let synced = device.last_synced_at.clone();
                    if synced == "Never" {
                        rsx! {
                            span { "Last synced: " }
                            span { class: "text-gray-700", "Never" }
                        }
                    } else {
                        rsx! {
                            span { "Last synced: " }
                            button {
                                class: "text-gray-700 hover:text-indigo-600 transition-colors cursor-pointer",
                                title: "Reset sync — clears sync state so all books re-download on next Kobo sync",
                                onclick: move |_| on_reset(()),
                                "{synced}"
                            }
                        }
                    }
                }
                span { "·" }
                {
                    let url = device.sync_url.clone();
                    let token_display = device.sync_token_display.clone();
                    rsx! {
                        button {
                            class: "font-mono text-gray-700 hover:text-indigo-600 transition-colors cursor-pointer min-w-[8ch] text-center",
                            title: "Copies URL for Kobo sync",
                            onclick: move |_| {
                                let url = url.clone();
                                spawn(async move {
                                    // navigator.clipboard requires HTTPS or localhost;
                                    // use execCommand fallback which works over plain HTTP.
                                    document::eval(&format!(
                                        "var t=document.createElement('textarea');\
                                         t.value='{url}';\
                                         t.style.cssText='position:fixed;opacity:0';\
                                         document.body.appendChild(t);\
                                         t.select();\
                                         document.execCommand('copy');\
                                         document.body.removeChild(t);"
                                    ));
                                    copied.set(true);
                                    let mut timer = document::eval("setTimeout(() => dioxus.send(true), 1500)");
                                    let _ = timer.recv::<bool>().await;
                                    copied.set(false);
                                });
                            },
                            if copied() { "✓" } else { "{token_display}" }
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// DeviceModal (Add / Edit)
// ---------------------------------------------------------------------------

#[component]
fn DeviceModal(mode: ModalMode, on_close: EventHandler<()>, on_saved: EventHandler<()>) -> Element {
    let is_edit = matches!(mode, ModalMode::Edit(_));

    let initial_token = match &mode {
        ModalMode::Edit(d) => d.token.clone(),
        ModalMode::Add => String::new(),
    };
    let initial_name = match &mode {
        ModalMode::Edit(d) => d.name.clone(),
        ModalMode::Add => String::new(),
    };
    let initial_type = match &mode {
        ModalMode::Edit(d) => d.device_type.clone(),
        ModalMode::Add => "kobo".to_string(),
    };
    let initial_action = match &mode {
        ModalMode::Edit(d) => d.on_removal_action.clone(),
        ModalMode::Add => "nothing".to_string(),
    };

    let token = use_signal(move || initial_token);
    let mut name = use_signal(move || initial_name);
    let mut device_type = use_signal(move || initial_type);
    let mut on_removal_action = use_signal(move || initial_action);
    let mut saving = use_signal(|| false);
    let mut error: Signal<Option<String>> = use_signal(|| None);

    // Pre-fill default name on Add
    use_effect(move || {
        if !is_edit && name().is_empty() {
            spawn(async move {
                if let Ok(default) = get_default_device_name().await {
                    name.set(default);
                }
            });
        }
    });

    let input_class = "w-full rounded-md border border-gray-300 px-3 py-1.5 text-sm focus:outline-none focus:ring-1 focus:ring-indigo-500";
    let label_class = "block text-sm font-medium text-gray-700 mb-1";
    let title = if is_edit { "Edit Device" } else { "Add Device" };
    let submit_label = if is_edit { "Save" } else { "Add Device" };

    rsx! {
        div { class: "fixed inset-0 z-50 flex items-center justify-center bg-black/40",
            div { class: "bg-white rounded-2xl shadow-xl w-full max-w-sm p-6",
                h3 { class: "text-base font-semibold text-gray-900 mb-4", { title } }

                div { class: "flex flex-col gap-3",
                    div {
                        label { class: label_class, "Name" }
                        input {
                            r#type: "text",
                            class: input_class,
                            value: name,
                            oninput: move |e| name.set(e.value()),
                        }
                    }
                    div {
                        label { class: label_class, "Device type" }
                        select {
                            class: input_class,
                            value: device_type,
                            onchange: move |e| device_type.set(e.value()),
                            option { value: "kobo", "Kobo" }
                        }
                    }
                    div {
                        label { class: label_class, "On removal" }
                        select {
                            class: input_class,
                            value: on_removal_action,
                            onchange: move |e| on_removal_action.set(e.value()),
                            option { value: "nothing", "Nothing" }
                            option { value: "mark_read", "Mark as Read" }
                            option { value: "mark_dnf", "Mark as DNF" }
                        }
                    }
                    if let Some(err) = error() {
                        p { class: "text-xs text-red-600", "{err}" }
                    }
                }

                div { class: "flex justify-end gap-3 mt-5",
                    button {
                        class: "px-3 py-1.5 text-sm font-medium rounded border border-gray-300 text-gray-700 hover:bg-gray-50",
                        disabled: saving(),
                        onclick: move |_| on_close(()),
                        "Cancel"
                    }
                    button {
                        class: "px-3 py-1.5 text-sm font-medium rounded bg-indigo-600 text-white hover:bg-indigo-700 disabled:opacity-50",
                        disabled: saving() || name().trim().is_empty(),
                        onclick: move |_| {
                            let n = name().trim().to_string();
                            let dt = device_type();
                            let action = on_removal_action();
                            let tok = token();
                            saving.set(true);
                            error.set(None);
                            spawn(async move {
                                let result = if tok.is_empty() {
                                    create_device_for_profile(n, dt, action).await
                                } else {
                                    update_device_for_profile(tok, n, action).await
                                };
                                match result {
                                    Ok(()) => on_saved(()),
                                    Err(e) => error.set(Some(e.to_string())),
                                }
                                saving.set(false);
                            });
                        },
                        if saving() { "Saving…" } else { { submit_label } }
                    }
                }
            }
        }
    }
}
