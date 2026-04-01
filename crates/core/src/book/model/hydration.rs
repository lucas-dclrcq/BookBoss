use crate::book::{Author, BookAuthor, BookId, Genre, Series, Tag};

/// All data needed to hydrate a slice of `Book`s into `BookSummary`
/// view-models, fetched in O(1) queries regardless of library size.
#[derive(Debug, Clone, Default)]
pub struct BookHydrationData {
    /// All `book_authors` join-table rows for the requested books.
    pub book_authors: Vec<BookAuthor>,
    /// All unique `Author` records referenced by `book_authors`.
    pub authors: Vec<Author>,
    /// All unique `Series` records for the requested books.
    pub series: Vec<Series>,
    /// All `(book_id, Genre)` associations for the requested books.
    pub genres: Vec<(BookId, Genre)>,
    /// All `(book_id, Tag)` associations for the requested books.
    pub tags: Vec<(BookId, Tag)>,
}
