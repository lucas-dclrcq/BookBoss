use crate::{
    Error,
    book::{Genre, GenreId, GenreToken, NewGenre},
    repository::Transaction,
};

#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait GenreRepository: Send + Sync {
    async fn add_genre(&self, transaction: &dyn Transaction, genre: NewGenre) -> Result<Genre, Error>;
    async fn update_genre(&self, transaction: &dyn Transaction, genre: Genre) -> Result<Genre, Error>;
    async fn find_by_id(&self, transaction: &dyn Transaction, id: GenreId) -> Result<Option<Genre>, Error>;
    async fn find_by_token(&self, transaction: &dyn Transaction, token: &GenreToken) -> Result<Option<Genre>, Error>;
    async fn find_by_name(&self, transaction: &dyn Transaction, name: &str) -> Result<Option<Genre>, Error>;
    async fn list_genres(&self, transaction: &dyn Transaction, start_id: Option<GenreId>, page_size: Option<u64>) -> Result<Vec<Genre>, Error>;
    async fn list_all_genres(&self, transaction: &dyn Transaction) -> Result<Vec<Genre>, Error>;
}
