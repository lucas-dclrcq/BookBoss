mod editor;
mod server;
mod types;

use dioxus::prelude::*;
pub(crate) use editor::ReviewEditor;
use server::get_review_data;
pub(crate) use server::{get_book_for_edit, get_picklist_data, list_non_system_libraries};
pub(crate) use types::BulkEditFields;

use crate::components::IncomingRefresh;

#[component]
pub(crate) fn ReviewPage(token: String) -> Element {
    let nav = use_navigator();
    let mut incoming_refresh = use_context::<IncomingRefresh>();
    let review_data = use_server_future(move || get_review_data(token.clone()))?;

    match review_data() {
        None => rsx! {
            div { class: "flex-1 flex items-center justify-center text-gray-400 text-sm",
                "Loading…"
            }
        },
        Some(Err(e)) => rsx! {
            div { class: "flex-1 flex items-center justify-center text-red-600 text-sm",
                "Failed to load: {e}"
            }
        },
        Some(Ok(data)) => {
            rsx! {
                ReviewEditor {
                    data,
                    edit_mode: false,
                    on_back: move |()| {
                        *incoming_refresh.0.write() += 1;
                        nav.push(crate::Route::IncomingPage {});
                    },
                }
            }
        }
    }
}
