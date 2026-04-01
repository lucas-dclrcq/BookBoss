use crate::{
    Error,
    book::{Author, AuthorId, AuthorToken, NewAuthor},
    repository::Transaction,
};

#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait AuthorRepository: Send + Sync {
    async fn add_author(&self, transaction: &dyn Transaction, author: NewAuthor) -> Result<Author, Error>;
    async fn update_author(&self, transaction: &dyn Transaction, author: Author) -> Result<Author, Error>;
    async fn find_by_id(&self, transaction: &dyn Transaction, id: AuthorId) -> Result<Option<Author>, Error>;
    async fn find_by_token(&self, transaction: &dyn Transaction, token: AuthorToken) -> Result<Option<Author>, Error>;
    async fn list_authors(&self, transaction: &dyn Transaction, start_id: Option<AuthorId>, page_size: Option<u64>) -> Result<Vec<Author>, Error>;
    async fn find_by_name(&self, transaction: &dyn Transaction, name: &str) -> Result<Option<Author>, Error>;
    async fn delete_author(&self, transaction: &dyn Transaction, author_id: AuthorId) -> Result<(), Error>;
    async fn list_all_authors(&self, transaction: &dyn Transaction) -> Result<Vec<Author>, Error>;

    /// Returns all authors whose `id` is in `ids`. Order is unspecified.
    /// Returns an empty vec immediately if `ids` is empty.
    async fn find_by_ids(&self, transaction: &dyn Transaction, ids: &[AuthorId]) -> Result<Vec<Author>, Error>;
}
