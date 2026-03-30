use crate::{
    Error,
    book::{
        AuthorId, AuthorRole, Book, BookAuthor, BookFile, BookId, BookIdentifier, BookQuery, BookToken, FileFormat, FileRole, Genre, GenreId, IdentifierType,
        NewBook, Tag, TagId,
    },
    repository::Transaction,
};

#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait BookRepository: Send + Sync {
    async fn add_book(&self, transaction: &dyn Transaction, book: NewBook) -> Result<Book, Error>;
    async fn update_book(&self, transaction: &dyn Transaction, book: Book) -> Result<Book, Error>;
    async fn find_by_id(&self, transaction: &dyn Transaction, id: BookId) -> Result<Option<Book>, Error>;
    async fn find_by_token(&self, transaction: &dyn Transaction, token: BookToken) -> Result<Option<Book>, Error>;
    async fn list_books(&self, transaction: &dyn Transaction, filter: &BookQuery, offset: Option<u64>, page_size: Option<u64>) -> Result<Vec<Book>, Error>;
    async fn authors_for_book(&self, transaction: &dyn Transaction, book_id: BookId) -> Result<Vec<BookAuthor>, Error>;
    async fn files_for_book(&self, transaction: &dyn Transaction, book_id: BookId) -> Result<Vec<BookFile>, Error>;
    async fn identifiers_for_book(&self, transaction: &dyn Transaction, book_id: BookId) -> Result<Vec<BookIdentifier>, Error>;
    async fn find_file_by_hash(&self, transaction: &dyn Transaction, hash: &str) -> Result<Option<BookFile>, Error>;
    async fn add_book_file(
        &self,
        transaction: &dyn Transaction,
        book_id: BookId,
        format: FileFormat,
        file_role: FileRole,
        path: String,
        file_size: i64,
        file_hash: String,
    ) -> Result<BookFile, Error>;
    /// Updates the `path` field of all Enriched `book_files` records for a book
    /// whose path starts with `old_slug` to use `new_slug` instead.
    /// Called after a filesystem rename to keep DB and disk in sync.
    async fn update_enriched_paths(&self, transaction: &dyn Transaction, book_id: BookId, old_slug: &str, new_slug: &str) -> Result<(), Error>;
    async fn add_book_author(
        &self,
        transaction: &dyn Transaction,
        book_id: BookId,
        author_id: AuthorId,
        role: AuthorRole,
        sort_order: i32,
    ) -> Result<(), Error>;
    async fn add_book_identifier(&self, transaction: &dyn Transaction, book_id: BookId, identifier_type: IdentifierType, value: String) -> Result<(), Error>;
    async fn delete_book(&self, transaction: &dyn Transaction, book_id: BookId) -> Result<(), Error>;
    async fn delete_book_authors(&self, transaction: &dyn Transaction, book_id: BookId) -> Result<(), Error>;
    async fn delete_book_identifiers(&self, transaction: &dyn Transaction, book_id: BookId) -> Result<(), Error>;
    async fn count_books_for_author(&self, transaction: &dyn Transaction, author_id: AuthorId) -> Result<u64, Error>;
    async fn genres_for_book(&self, transaction: &dyn Transaction, book_id: BookId) -> Result<Vec<Genre>, Error>;
    async fn tags_for_book(&self, transaction: &dyn Transaction, book_id: BookId) -> Result<Vec<Tag>, Error>;
    async fn add_book_genre(&self, transaction: &dyn Transaction, book_id: BookId, genre_id: GenreId) -> Result<(), Error>;
    async fn add_book_tag(&self, transaction: &dyn Transaction, book_id: BookId, tag_id: TagId) -> Result<(), Error>;
    async fn delete_book_genres(&self, transaction: &dyn Transaction, book_id: BookId) -> Result<(), Error>;
    async fn delete_book_tags(&self, transaction: &dyn Transaction, book_id: BookId) -> Result<(), Error>;
    /// Deletes a specific file record for a book (identified by format + role).
    /// No-op if the record does not exist.
    async fn delete_book_file_by_role(&self, transaction: &dyn Transaction, book_id: BookId, format: FileFormat, role: FileRole) -> Result<(), Error>;
    /// Returns the IDs of books that have an Original EPUB but no Enriched EPUB
    /// in `book_files`. Used at startup to re-enqueue any enrichment jobs that
    /// were lost due to a crash.
    async fn find_book_ids_needing_enrichment(&self, transaction: &dyn Transaction) -> Result<Vec<BookId>, Error>;
    /// Returns the IDs of books that have an Enriched EPUB but no Enriched
    /// KEPUB in `book_files`. Used at startup to re-enqueue any KEPUB
    /// conversion jobs that were lost due to a crash.
    async fn find_book_ids_needing_kepub_conversion(&self, transaction: &dyn Transaction) -> Result<Vec<BookId>, Error>;
    /// Returns all book file records. Used by the file integrity health check
    /// to verify that every recorded file exists on disk.
    async fn list_all_book_files(&self, transaction: &dyn Transaction) -> Result<Vec<BookFile>, Error>;
    /// Returns the IDs of books that have an Enriched EPUB whose `created_at`
    /// is older than the book's `updated_at` — i.e. metadata changed after
    /// the enriched file was generated, so re-enrichment is needed.
    async fn find_book_ids_with_stale_enrichment(&self, transaction: &dyn Transaction) -> Result<Vec<BookId>, Error>;
    /// Returns IDs of Available books that have the given genre attached.
    /// Used to enqueue re-enrichment after a genre is deleted.
    async fn available_book_ids_for_genre(&self, transaction: &dyn Transaction, genre_id: GenreId) -> Result<Vec<BookId>, Error>;
    /// Returns IDs of Available books that have the given tag attached.
    /// Used to enqueue re-enrichment after a tag is deleted.
    async fn available_book_ids_for_tag(&self, transaction: &dyn Transaction, tag_id: TagId) -> Result<Vec<BookId>, Error>;

    /// Returns up to `batch_size` IDs of Available books with `id > after_id`,
    /// ordered by id ASC. Used by cursor sweep jobs to process the library in
    /// bounded batches without loading the full library into memory.
    async fn find_available_books_for_sweep(&self, transaction: &dyn Transaction, after_id: Option<BookId>, batch_size: u64) -> Result<Vec<BookId>, Error>;

    /// Sets (or clears) the `sidecar_fingerprint` for a book.
    ///
    /// Called with `Some(hash)` after a successful enriched-EPUB write,
    /// and with `None` to invalidate whenever metadata mutations occur.
    async fn update_sidecar_fingerprint(&self, transaction: &dyn Transaction, book_id: BookId, fingerprint: Option<String>) -> Result<(), Error>;

    /// Returns up to `batch_size` IDs of Available books that need any
    /// enrichment work, with `id > after_id`, ordered by id ASC.
    ///
    /// A book qualifies if any of the following are true:
    /// - Has an original EPUB but no enriched EPUB (needs initial enrichment)
    /// - Has an enriched EPUB but no enriched KEPUB (needs KEPUB conversion)
    /// - Has an enriched EPUB whose `created_at` is older than the book's
    ///   `updated_at` (metadata changed, enrichment is stale)
    /// - Has a NULL `sidecar_fingerprint` (metadata written but fingerprint
    ///   never recorded, or invalidated by a metadata mutation)
    ///
    /// Used by the `EnsureEnrichmentsHandler` cursor sweep.
    async fn find_book_ids_needing_any_enrichment(
        &self,
        transaction: &dyn Transaction,
        after_id: Option<BookId>,
        batch_size: u64,
    ) -> Result<Vec<BookId>, Error>;
}
