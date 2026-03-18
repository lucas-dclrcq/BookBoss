use crate::{
    Error,
    book::BookId,
    reading::{ReadStatus, UserBookMetadata},
    repository::Transaction,
    user::UserId,
};

#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait UserBookMetadataRepository: Send + Sync {
    async fn upsert(&self, transaction: &dyn Transaction, metadata: UserBookMetadata) -> Result<UserBookMetadata, Error>;
    async fn find_by_user_and_book(&self, transaction: &dyn Transaction, user_id: UserId, book_id: BookId) -> Result<Option<UserBookMetadata>, Error>;
    async fn list_for_user(
        &self,
        transaction: &dyn Transaction,
        user_id: UserId,
        status: Option<ReadStatus>,
        start_book_id: Option<BookId>,
        page_size: Option<u64>,
    ) -> Result<Vec<UserBookMetadata>, Error>;

    /// Returns reading state rows for the given user restricted to the
    /// specified book IDs. Rows that do not exist are simply absent from the
    /// result (no row ≡ Unread).
    async fn list_for_user_and_books(&self, transaction: &dyn Transaction, user_id: UserId, book_ids: &[BookId]) -> Result<Vec<UserBookMetadata>, Error>;
}
