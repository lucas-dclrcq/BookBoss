mod editor;
mod server;
mod types;

use dioxus::prelude::*;
pub(crate) use editor::ReviewEditor;
pub(crate) use server::get_book_for_edit;
use server::get_review_data;

#[component]
pub(crate) fn ReviewPage(token: String) -> Element {
    let nav = use_navigator();
    let mut incoming_refresh: Signal<u32> = use_context();
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
                    on_back: move |_| {
                        *incoming_refresh.write() += 1;
                        nav.push(crate::Route::IncomingPage {});
                    },
                }
            }
        }
    }
}
