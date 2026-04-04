use crate::{Error, book::BookId, repository::Transaction};

#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait KoReaderDocumentHashRepository: Send + Sync {
    /// Inserts (document_hash, book_id) rows. Ignores duplicates via unique
    /// constraint.
    async fn insert_hashes(&self, transaction: &dyn Transaction, book_id: BookId, hashes: Vec<String>) -> Result<(), Error>;

    /// Prefix-matches document_hash LIKE '{prefix}%'. Returns the first
    /// matching BookId.
    async fn find_book_by_digest_prefix(&self, transaction: &dyn Transaction, prefix: &str) -> Result<Option<BookId>, Error>;
}
