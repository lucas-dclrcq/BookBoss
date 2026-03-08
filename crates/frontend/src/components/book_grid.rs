use dioxus::prelude::*;

use crate::{
    Route,
    routes::{book_detail_page::delete_library_book, books_page::BookSummary, shelf_page::remove_book_from_shelf},
};

// ---------------------------------------------------------------------------
// Context types
// ---------------------------------------------------------------------------

/// Describes the viewing context so `BookCard` knows what the × button does.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum BookGridContext {
    /// All-books view: × deletes the book from the library (requires
    /// capability).
    AllBooks { can_delete: bool },
    /// Owner is viewing one of their own shelves: × removes the book from the
    /// shelf.
    OwnShelf { shelf_token: String },
    /// Read-only view (e.g. someone else's shelf, author/series pages): no ×
    /// button.
    ReadOnly,
}

// ---------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------

#[component]
pub(crate) fn BookGrid(
    books: Vec<BookSummary>,
    context: BookGridContext,
    /// Called after any successful card action (delete / remove from shelf).
    /// The parent page should use this to restart its book-list resource.
    on_action: EventHandler<()>,
) -> Element {
    use_context_provider(|| context);
    use_context_provider(|| on_action);
    rsx! {
        div { class: "flex-1 overflow-auto p-4",
            div { class: "grid gap-x-8 gap-y-4",
                style: "grid-template-columns: repeat(auto-fill, minmax(120px, 1fr))",
                for book in &books {
                    BookCard { book: book.clone() }
                }
            }
        }
    }
}

#[component]
fn BookCard(book: BookSummary) -> Element {
    let navigator = use_navigator();
    let token = book.token.clone();
    let ctx = use_context::<BookGridContext>();
    let on_action = use_context::<EventHandler<()>>();

    let author_str = book.author_names.join(", ");
    let series_line = match (&book.series_name, &book.series_number) {
        (Some(name), Some(num)) => Some(format!("{name} #{num}")),
        (Some(name), None) => Some(name.clone()),
        _ => None,
    };

    // Build the × button action (if any) based on context.
    let remove_action: Option<Box<dyn Fn() + 'static>> = match ctx {
        BookGridContext::AllBooks { can_delete: true } => {
            let tok = book.token.clone();
            Some(Box::new(move || {
                let tok = tok.clone();
                spawn(async move {
                    if delete_library_book(tok).await.is_ok() {
                        on_action.call(());
                    }
                });
            }))
        }
        BookGridContext::OwnShelf { shelf_token } => {
            let stok = shelf_token.clone();
            let btok = book.token.clone();
            Some(Box::new(move || {
                let s = stok.clone();
                let b = btok.clone();
                spawn(async move {
                    if remove_book_from_shelf(s, b).await.is_ok() {
                        on_action.call(());
                    }
                });
            }))
        }
        _ => None,
    };

    rsx! {
        div { class: "flex flex-col",
            div { class: "relative cursor-pointer",
                onclick: move |_| { navigator.push(Route::BookDetailPage { token: token.clone() }); },
                img {
                    src: "/api/v1/covers/{book.token}",
                    alt: "{book.title}",
                    class: "w-full object-cover rounded shadow-sm",
                    style: "aspect-ratio: 2/3",
                }
                if let Some(action) = remove_action {
                    button {
                        class: "absolute top-1 right-1 w-5 h-5 flex items-center justify-center rounded-full bg-black/50 text-white text-xs hover:bg-red-600/80 leading-none",
                        title: "Remove",
                        onclick: move |e| {
                            e.stop_propagation();
                            action();
                        },
                        "×"
                    }
                }
            }
            div { class: "mt-1 px-0.5",
                p { class: "text-xs font-semibold text-gray-900 leading-tight line-clamp-2",
                    "{book.title}"
                }
                p { class: "text-xs text-gray-500 leading-tight truncate mt-0.5",
                    "{author_str}"
                }
                if let Some(series) = series_line {
                    p { class: "text-xs text-gray-400 leading-tight truncate mt-0.5",
                        "{series}"
                    }
                }
            }
        }
    }
}
