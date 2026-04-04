use bb_core::{Error, book::BookId, koreader::KoReaderDocumentHashRepository, repository::Transaction};
use chrono::Utc;
use sea_orm::{ActiveValue::Set, ColumnTrait, DbErr, EntityTrait, QueryFilter, QuerySelect, sea_query::OnConflict};

use crate::{
    entities::{koreader_document_hashes, prelude},
    error::handle_dberr,
    transaction::TransactionImpl,
};

// ── Adapter
// ───────────────────────────────────────────────────────────────────

pub(crate) struct KoReaderDocumentHashRepositoryAdapter;

impl KoReaderDocumentHashRepositoryAdapter {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl KoReaderDocumentHashRepository for KoReaderDocumentHashRepositoryAdapter {
    async fn insert_hashes(&self, transaction: &dyn Transaction, book_id: BookId, hashes: Vec<String>) -> Result<(), Error> {
        if hashes.is_empty() {
            return Ok(());
        }

        let transaction = TransactionImpl::get_db_transaction(transaction)?;
        let now = Utc::now();

        let models: Vec<koreader_document_hashes::ActiveModel> = hashes
            .into_iter()
            .map(|hash| koreader_document_hashes::ActiveModel {
                book_id: Set(book_id as i64),
                document_hash: Set(hash),
                created_at: Set(now.into()),
                ..Default::default()
            })
            .collect();

        match prelude::KoReaderDocumentHashes::insert_many(models)
            .on_conflict(
                OnConflict::columns([koreader_document_hashes::Column::DocumentHash, koreader_document_hashes::Column::BookId])
                    .do_nothing()
                    .to_owned(),
            )
            .exec(transaction)
            .await
        {
            Ok(_) | Err(DbErr::RecordNotInserted) => Ok(()),
            Err(e) => Err(handle_dberr(e).into()),
        }
    }

    async fn find_book_by_digest_prefix(&self, transaction: &dyn Transaction, prefix: &str) -> Result<Option<BookId>, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        let result = prelude::KoReaderDocumentHashes::find()
            .filter(koreader_document_hashes::Column::DocumentHash.starts_with(prefix))
            .select_only()
            .column(koreader_document_hashes::Column::BookId)
            .into_tuple::<i64>()
            .one(transaction)
            .await
            .map_err(handle_dberr)?;

        Ok(result.map(|id| id as BookId))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bb_core::{
        book::{BookStatus, NewBook},
        repository::RepositoryService,
    };
    use sea_orm::Database;

    use crate::create_repository_service;

    async fn setup() -> Arc<RepositoryService> {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        create_repository_service(db).await.unwrap()
    }

    async fn new_book(svc: &RepositoryService, title: &str) -> u64 {
        let tx = svc.repository().begin().await.unwrap();
        let book = svc
            .book_repository()
            .add_book(
                &*tx,
                NewBook {
                    title: title.to_owned(),
                    status: BookStatus::Available,
                    description: None,
                    published_date: None,
                    language: None,
                    series_id: None,
                    series_number: None,
                    publisher_id: None,
                    page_count: None,
                    rating: None,
                    metadata_source: None,
                    has_cover: false,
                },
            )
            .await
            .unwrap();
        tx.commit().await.unwrap();
        book.id
    }

    #[tokio::test]
    async fn insert_hashes_inserts_rows_for_each_hash() {
        let svc = setup().await;
        let book_id = new_book(&svc, "Dune").await;

        let tx = svc.repository().begin().await.unwrap();
        svc.koreader_document_hash_repository()
            .insert_hashes(&*tx, book_id, vec!["abc123".to_owned(), "def456".to_owned()])
            .await
            .unwrap();
        tx.commit().await.unwrap();

        // Both hashes should be findable
        let tx = svc.repository().begin_read_only().await.unwrap();
        let found1 = svc
            .koreader_document_hash_repository()
            .find_book_by_digest_prefix(&*tx, "abc123")
            .await
            .unwrap();
        let found2 = svc
            .koreader_document_hash_repository()
            .find_book_by_digest_prefix(&*tx, "def456")
            .await
            .unwrap();

        assert_eq!(found1, Some(book_id));
        assert_eq!(found2, Some(book_id));
    }

    #[tokio::test]
    async fn insert_hashes_is_idempotent() {
        let svc = setup().await;
        let book_id = new_book(&svc, "Foundation").await;

        let tx = svc.repository().begin().await.unwrap();
        svc.koreader_document_hash_repository()
            .insert_hashes(&*tx, book_id, vec!["abc123".to_owned()])
            .await
            .unwrap();
        tx.commit().await.unwrap();

        // Second call with same args should be a no-op (no error)
        let tx = svc.repository().begin().await.unwrap();
        svc.koreader_document_hash_repository()
            .insert_hashes(&*tx, book_id, vec!["abc123".to_owned()])
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let tx = svc.repository().begin_read_only().await.unwrap();
        let found = svc
            .koreader_document_hash_repository()
            .find_book_by_digest_prefix(&*tx, "abc123")
            .await
            .unwrap();

        assert_eq!(found, Some(book_id));
    }

    #[tokio::test]
    async fn find_book_by_digest_prefix_returns_none_for_unknown_prefix() {
        let svc = setup().await;

        let tx = svc.repository().begin_read_only().await.unwrap();
        let found = svc
            .koreader_document_hash_repository()
            .find_book_by_digest_prefix(&*tx, "nonexistent")
            .await
            .unwrap();

        assert!(found.is_none());
    }

    #[tokio::test]
    async fn find_book_by_digest_prefix_matches_prefix() {
        let svc = setup().await;
        let book_id = new_book(&svc, "The Left Hand of Darkness").await;

        let full_hash = "deadbeefcafe1234567890abcdef";

        let tx = svc.repository().begin().await.unwrap();
        svc.koreader_document_hash_repository()
            .insert_hashes(&*tx, book_id, vec![full_hash.to_owned()])
            .await
            .unwrap();
        tx.commit().await.unwrap();

        // Find by short prefix
        let tx = svc.repository().begin_read_only().await.unwrap();
        let found = svc
            .koreader_document_hash_repository()
            .find_book_by_digest_prefix(&*tx, "deadbeef")
            .await
            .unwrap();

        assert_eq!(found, Some(book_id));
    }
}
