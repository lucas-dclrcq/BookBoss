use std::{any::Any, pin::Pin, sync::Arc};

use derive_builder::Builder;

use crate::{
    Error,
    auth::SessionRepository,
    book::{AuthorRepository, BookRepository, GenreRepository, PublisherRepository, SeriesRepository, TagRepository},
    device::DeviceRepository,
    import::ImportJobRepository,
    jobs::JobRepository,
    reading::UserBookMetadataRepository,
    shelf::ShelfRepository,
    user::{UserRepository, UserSettingRepository},
};

#[derive(Builder)]
#[builder(pattern = "owned")]
pub struct RepositoryService {
    repository: Arc<dyn Repository>,
    session_repository: Arc<dyn SessionRepository>,
    user_repository: Arc<dyn UserRepository>,
    user_setting_repository: Arc<dyn UserSettingRepository>,
    author_repository: Arc<dyn AuthorRepository>,
    series_repository: Arc<dyn SeriesRepository>,
    publisher_repository: Arc<dyn PublisherRepository>,
    genre_repository: Arc<dyn GenreRepository>,
    tag_repository: Arc<dyn TagRepository>,
    book_repository: Arc<dyn BookRepository>,
    import_job_repository: Arc<dyn ImportJobRepository>,
    job_repository: Arc<dyn JobRepository>,
    shelf_repository: Arc<dyn ShelfRepository>,
    user_book_metadata_repository: Arc<dyn UserBookMetadataRepository>,
    device_repository: Arc<dyn DeviceRepository>,
}

impl RepositoryService {
    /// Returns a reference to the main repository for transaction management.
    #[must_use]
    pub fn repository(&self) -> &Arc<dyn Repository> {
        &self.repository
    }

    /// Returns a reference to the session repository.
    #[must_use]
    pub fn session_repository(&self) -> &Arc<dyn SessionRepository> {
        &self.session_repository
    }

    /// Returns a reference to the user repository.
    #[must_use]
    pub fn user_repository(&self) -> &Arc<dyn UserRepository> {
        &self.user_repository
    }

    /// Returns a reference to the user setting repository.
    #[must_use]
    pub fn user_setting_repository(&self) -> &Arc<dyn UserSettingRepository> {
        &self.user_setting_repository
    }

    /// Returns a reference to the author repository.
    #[must_use]
    pub fn author_repository(&self) -> &Arc<dyn AuthorRepository> {
        &self.author_repository
    }

    /// Returns a reference to the series repository.
    #[must_use]
    pub fn series_repository(&self) -> &Arc<dyn SeriesRepository> {
        &self.series_repository
    }

    /// Returns a reference to the publisher repository.
    #[must_use]
    pub fn publisher_repository(&self) -> &Arc<dyn PublisherRepository> {
        &self.publisher_repository
    }

    /// Returns a reference to the genre repository.
    #[must_use]
    pub fn genre_repository(&self) -> &Arc<dyn GenreRepository> {
        &self.genre_repository
    }

    /// Returns a reference to the tag repository.
    #[must_use]
    pub fn tag_repository(&self) -> &Arc<dyn TagRepository> {
        &self.tag_repository
    }

    /// Returns a reference to the book repository.
    #[must_use]
    pub fn book_repository(&self) -> &Arc<dyn BookRepository> {
        &self.book_repository
    }

    /// Returns a reference to the import job repository.
    #[must_use]
    pub fn import_job_repository(&self) -> &Arc<dyn ImportJobRepository> {
        &self.import_job_repository
    }

    /// Returns a reference to the job repository.
    #[must_use]
    pub fn job_repository(&self) -> &Arc<dyn JobRepository> {
        &self.job_repository
    }

    /// Returns a reference to the shelf repository.
    #[must_use]
    pub fn shelf_repository(&self) -> &Arc<dyn ShelfRepository> {
        &self.shelf_repository
    }

    /// Returns a reference to the user book metadata repository.
    #[must_use]
    pub fn user_book_metadata_repository(&self) -> &Arc<dyn UserBookMetadataRepository> {
        &self.user_book_metadata_repository
    }

    /// Returns a reference to the device repository.
    #[must_use]
    pub fn device_repository(&self) -> &Arc<dyn DeviceRepository> {
        &self.device_repository
    }
}

#[async_trait::async_trait]
pub trait Transaction: Any + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    async fn commit(self: Box<Self>) -> Result<(), Error>;
    async fn rollback(self: Box<Self>) -> Result<(), Error>;
}

/// Execute an async operation within a read-write transaction.
///
/// Clones one or more repositories, begins a transaction, executes the body,
/// and commits on success or rolls back on error.
///
/// # Examples
/// ```ignore
/// // Single repository
/// with_transaction!(self, user_repository, |tx| {
///     user_repository.add_user(tx, user).await
/// })
///
/// // Multiple repositories
/// with_transaction!(self, user_repository, order_repository, |tx| {
///     let user = user_repository.add_user(tx, user).await?;
///     order_repository.create_order(tx, user.id, order).await
/// })
/// ```
#[macro_export]
macro_rules! with_transaction {
    ($self:expr, $($repo:ident),+ , |$tx:ident| $body:expr) => {{
        $(let $repo = $self.repository_service.$repo().clone();)+
        $crate::repository::transaction(&**$self.repository_service.repository(), |$tx| Box::pin(async move { $body })).await
    }};
}

/// Execute an async operation within a read-only transaction.
///
/// Clones one or more repositories and executes the body within a read-only
/// transaction.
///
/// # Examples
/// ```ignore
/// // Single repository
/// with_read_only_transaction!(self, user_repository, |tx| {
///     user_repository.find_by_id(tx, id).await
/// })
///
/// // Multiple repositories
/// with_read_only_transaction!(self, user_repository, order_repository, |tx| {
///     let user = user_repository.find_by_id(tx, id).await?;
///     let orders = order_repository.find_by_user(tx, user.id).await?;
///     Ok((user, orders))
/// })
/// ```
#[macro_export]
macro_rules! with_read_only_transaction {
    ($self:expr, $($repo:ident),+ , |$tx:ident| $body:expr) => {{
        $(let $repo = $self.repository_service.$repo().clone();)+
        $crate::repository::read_only_transaction(&**$self.repository_service.repository(), |$tx| Box::pin(async move { $body })).await
    }};
}

#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait Repository: Send + Sync {
    async fn begin(&self) -> Result<Box<dyn Transaction>, Error>;
    async fn begin_read_only(&self) -> Result<Box<dyn Transaction>, Error>;
    async fn close(&self) -> Result<(), Error>;
}

/// Execute a closure within a transaction, automatically committing on success
/// or rolling back on error.
///
/// # Example
/// ```ignore
/// let result = transaction(&*repository, |tx| Box::pin(async move {
///     // do stuff with tx
///     Ok(result)
/// })).await?;
/// ```
pub async fn transaction<F, T>(repository: &dyn Repository, callback: F) -> Result<T, Error>
where
    F: for<'c> FnOnce(&'c dyn Transaction) -> Pin<Box<dyn Future<Output = Result<T, Error>> + Send + 'c>> + Send,
    T: Send,
{
    let tx = repository.begin().await?;
    match callback(&*tx).await {
        Ok(result) => {
            tx.commit().await?;
            Ok(result)
        }
        Err(e) => {
            // Best effort rollback - if it fails, we still return the original error
            let _ = tx.rollback().await;
            Err(e)
        }
    }
}

/// Execute a closure within a read-only transaction.
///
/// # Example
/// ```ignore
/// let result = read_only_transaction(&*repository, |tx| Box::pin(async move {
///     // do stuff with tx
///     Ok(result)
/// })).await?;
/// ```
pub async fn read_only_transaction<F, T>(repository: &dyn Repository, callback: F) -> Result<T, Error>
where
    F: for<'c> FnOnce(&'c dyn Transaction) -> Pin<Box<dyn Future<Output = Result<T, Error>> + Send + 'c>> + Send,
    T: Send,
{
    let tx = repository.begin_read_only().await?;
    callback(&*tx).await
}
