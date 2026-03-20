use crate::{
    Error,
    book::{Book, BookId},
    filter::BookFilter,
    repository::Transaction,
    user::UserId,
};

#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait LibraryRepository: Send + Sync {
    /// Returns the count of books with `status = available`.
    async fn count_available_books(&self, transaction: &dyn Transaction) -> Result<u64, Error>;

    /// Returns the total number of authors.
    async fn count_authors(&self, transaction: &dyn Transaction) -> Result<u64, Error>;

    /// Returns paginated books matching the given filter.
    async fn books_for_filter(
        &self,
        transaction: &dyn Transaction,
        filter: &BookFilter,
        user_id: UserId,
        start_id: Option<BookId>,
        page_size: Option<u64>,
    ) -> Result<Vec<Book>, Error>;

    /// Returns the total count of books matching the given filter.
    async fn count_for_filter(&self, transaction: &dyn Transaction, filter: &BookFilter, user_id: UserId) -> Result<u64, Error>;
}
