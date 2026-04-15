// SeaORM uses i64 for all primary keys; domain types use u64. Auto-increment
// IDs are always positive and will not exceed i64::MAX in practice, so these
// casts are safe at this boundary. Page sizes/counts are similarly bounded.
#![allow(
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    reason = "SeaORM i64/u64 boundary — IDs and page values are always in range"
)]

use std::sync::Arc;

use bb_core::{
    Error,
    app_setting::AppSettingRepository,
    auth::SessionRepository,
    book::{AuthorRepository, BookRepository, GenreRepository, PublisherRepository, SeriesRepository, TagRepository},
    collection::CollectionRepository,
    device::DeviceRepository,
    import::ImportJobRepository,
    jobs::JobRepository,
    koreader::KoReaderDocumentHashRepository,
    library::LibraryRepository,
    message::SystemMessageRepository,
    reading::UserBookMetadataRepository,
    repository::{Repository, RepositoryService, RepositoryServiceBuilder},
    shelf::ShelfRepository,
    user::{UserRepository, UserSettingRepository},
};
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;
use serde::Deserialize;

pub mod error;

pub use error::*;

mod adapters;
mod entities;
mod filter;
mod migrations;
mod repository;
mod sort;
mod transaction;

use crate::{
    adapters::{
        app_setting::AppSettingRepositoryAdapter, author::AuthorRepositoryAdapter, book::BookRepositoryAdapter, collection::CollectionRepositoryAdapter,
        device::DeviceRepositoryAdapter, genre::GenreRepositoryAdapter, import_job::ImportJobRepositoryAdapter, job::JobRepositoryAdapter,
        koreader_document_hash::KoReaderDocumentHashRepositoryAdapter, library::LibraryRepositoryAdapter, publisher::PublisherRepositoryAdapter,
        series::SeriesRepositoryAdapter, session::SessionRepositoryAdapter, shelf::ShelfRepositoryAdapter, system_message::SystemMessageRepositoryAdapter,
        tag::TagRepositoryAdapter, user::UserRepositoryAdapter, user_book_metadata::UserBookMetadataRepositoryAdapter,
        user_settings::UserSettingRepositoryAdapter,
    },
    migrations::Migrator,
    repository::RepositoryImpl,
    transaction::TransactionImpl,
};

#[derive(Debug, Deserialize)]
pub struct DatabaseConfig {
    /// (required) Fully qualified URL for accessing the database.
    /// e.g. <postgres://user:password@host/database>
    pub database_url: String,
}

pub async fn open_database(config: &DatabaseConfig) -> Result<DatabaseConnection, Error> {
    tracing::debug!("Connecting to database...");
    let mut opt = ConnectOptions::new(&config.database_url);
    opt.max_connections(9)
        .min_connections(5)
        .sqlx_logging(true)
        .sqlx_logging_level(tracing::log::LevelFilter::Info);

    Ok(Database::connect(opt).await.map_err(handle_dberr)?)
}

pub async fn create_repository_service(database: DatabaseConnection) -> Result<Arc<RepositoryService>, Error> {
    let span = tracing::span!(tracing::Level::TRACE, "Migrations").entered();
    Migrator::up(&database, None).await.map_err(handle_dberr)?;
    span.exit();

    let repository_service = RepositoryServiceBuilder::default()
        .repository(Arc::new(RepositoryImpl::new(database)) as Arc<dyn Repository>)
        .session_repository(Arc::new(SessionRepositoryAdapter::new()) as Arc<dyn SessionRepository>)
        .user_repository(Arc::new(UserRepositoryAdapter::new()) as Arc<dyn UserRepository>)
        .user_setting_repository(Arc::new(UserSettingRepositoryAdapter::new()) as Arc<dyn UserSettingRepository>)
        .author_repository(Arc::new(AuthorRepositoryAdapter::new()) as Arc<dyn AuthorRepository>)
        .series_repository(Arc::new(SeriesRepositoryAdapter::new()) as Arc<dyn SeriesRepository>)
        .publisher_repository(Arc::new(PublisherRepositoryAdapter::new()) as Arc<dyn PublisherRepository>)
        .genre_repository(Arc::new(GenreRepositoryAdapter::new()) as Arc<dyn GenreRepository>)
        .tag_repository(Arc::new(TagRepositoryAdapter::new()) as Arc<dyn TagRepository>)
        .book_repository(Arc::new(BookRepositoryAdapter::new()) as Arc<dyn BookRepository>)
        .import_job_repository(Arc::new(ImportJobRepositoryAdapter::new()) as Arc<dyn ImportJobRepository>)
        .job_repository(Arc::new(JobRepositoryAdapter::new()) as Arc<dyn JobRepository>)
        .collection_repository(Arc::new(CollectionRepositoryAdapter::new()) as Arc<dyn CollectionRepository>)
        .library_repository(Arc::new(LibraryRepositoryAdapter::new()) as Arc<dyn LibraryRepository>)
        .shelf_repository(Arc::new(ShelfRepositoryAdapter::new()) as Arc<dyn ShelfRepository>)
        .user_book_metadata_repository(Arc::new(UserBookMetadataRepositoryAdapter::new()) as Arc<dyn UserBookMetadataRepository>)
        .device_repository(Arc::new(DeviceRepositoryAdapter::new()) as Arc<dyn DeviceRepository>)
        .system_message_repository(Arc::new(SystemMessageRepositoryAdapter::new()) as Arc<dyn SystemMessageRepository>)
        .koreader_document_hash_repository(Arc::new(KoReaderDocumentHashRepositoryAdapter::new()) as Arc<dyn KoReaderDocumentHashRepository>)
        .app_setting_repository(Arc::new(AppSettingRepositoryAdapter::new()) as Arc<dyn AppSettingRepository>)
        .build()
        .map_err(|e| Error::Infrastructure(e.to_string()))?;

    Ok(Arc::new(repository_service))
}
