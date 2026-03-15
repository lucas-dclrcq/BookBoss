use crate::book::{Book, BookFile, BookId};

/// A single book entry in a sync diff — the book and the file to serve.
#[derive(Debug, Clone)]
pub struct BookSyncEntry {
    pub book: Book,
    pub file: BookFile,
}

/// The result of [`DeviceService::compute_sync_diff`].
///
/// Entries within each category are ordered by `book.id` ascending.
/// At most `page_size` total entries (new + upgraded + refreshed) are returned
/// per call; use `has_more` and the last `book.id` as a keyset cursor to fetch
/// subsequent pages.
#[derive(Debug)]
pub struct SyncDiff {
    /// Books that have never been sent to this device.
    pub new_books: Vec<BookSyncEntry>,
    /// Books already on the device where a better file is now available
    /// (different format or file_role than the one previously sent).
    pub upgraded_books: Vec<BookSyncEntry>,
    /// Books already on the device with the same file, but whose metadata has
    /// changed since the last sync (`book.updated_at > since`).
    pub refreshed_books: Vec<BookSyncEntry>,
    /// Book IDs that were previously sent to the device but are no longer on
    /// the companion shelf. Only populated on the first page of a sync
    /// (`after_book_id` is `None`).
    pub removed_book_ids: Vec<BookId>,
    /// `true` when there are more entries beyond this page.
    pub has_more: bool,
}

impl SyncDiff {
    pub fn empty() -> Self {
        Self {
            new_books: Vec::new(),
            upgraded_books: Vec::new(),
            refreshed_books: Vec::new(),
            removed_book_ids: Vec::new(),
            has_more: false,
        }
    }

    /// Total number of add/update entries in this page.
    pub fn entry_count(&self) -> usize {
        self.new_books.len() + self.upgraded_books.len() + self.refreshed_books.len()
    }

    pub fn is_empty(&self) -> bool {
        self.new_books.is_empty() && self.upgraded_books.is_empty() && self.refreshed_books.is_empty() && self.removed_book_ids.is_empty()
    }
}
