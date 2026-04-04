use std::sync::Arc;

use md5::{Digest, Md5};

use crate::{Error, book::BookId, repository::RepositoryService, with_read_only_transaction, with_transaction};

#[async_trait::async_trait]
pub trait KoReaderService: Send + Sync {
    /// Computes md5(filename) and md5(contents), inserts both as document
    /// hashes. Duplicate hashes (same hash + book_id) are silently ignored.
    async fn register_hashes(&self, book_id: BookId, filename: &str, contents: &[u8]) -> Result<(), Error>;

    /// Prefix-matches the incoming KOReader document digest against stored
    /// hashes. Returns the BookId for the first match, or None if not
    /// found.
    async fn find_book_by_digest(&self, digest_prefix: &str) -> Result<Option<BookId>, Error>;
}

pub struct KoReaderServiceImpl {
    repository_service: Arc<RepositoryService>,
}

impl KoReaderServiceImpl {
    pub fn new(repository_service: Arc<RepositoryService>) -> Self {
        Self { repository_service }
    }
}

fn md5_hex(data: &[u8]) -> String {
    let hash = Md5::digest(data);
    format!("{hash:x}")
}

#[async_trait::async_trait]
impl KoReaderService for KoReaderServiceImpl {
    async fn register_hashes(&self, book_id: BookId, filename: &str, contents: &[u8]) -> Result<(), Error> {
        let filename_hash = md5_hex(filename.as_bytes());
        let contents_hash = md5_hex(contents);
        let hashes = if filename_hash == contents_hash {
            vec![filename_hash]
        } else {
            vec![filename_hash, contents_hash]
        };
        with_transaction!(self, koreader_document_hash_repository, |tx| {
            koreader_document_hash_repository.insert_hashes(tx, book_id, hashes).await
        })
    }

    async fn find_book_by_digest(&self, digest_prefix: &str) -> Result<Option<BookId>, Error> {
        let digest_prefix = digest_prefix.to_owned();
        with_read_only_transaction!(self, koreader_document_hash_repository, |tx| {
            koreader_document_hash_repository.find_book_by_digest_prefix(tx, &digest_prefix).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::koreader::repository::MockKoReaderDocumentHashRepository;

    fn create_service(mock: MockKoReaderDocumentHashRepository) -> KoReaderServiceImpl {
        let repository_service = Arc::new(
            crate::repository::testing::default_repository_service_builder()
                .koreader_document_hash_repository(Arc::new(mock))
                .build()
                .expect("all fields provided"),
        );
        KoReaderServiceImpl::new(repository_service)
    }

    #[tokio::test]
    async fn register_hashes_calls_insert_with_two_hashes_for_distinct_filename_and_contents() {
        let mut mock = MockKoReaderDocumentHashRepository::new();
        mock.expect_insert_hashes()
            .withf(|_, book_id, hashes| *book_id == 1 && hashes.len() == 2)
            .returning(|_, _, _| Box::pin(async { Ok(()) }));
        let svc = create_service(mock);
        svc.register_hashes(1, "Foundation.epub", b"epub-bytes").await.unwrap();
    }

    #[tokio::test]
    async fn register_hashes_deduplicates_when_filename_and_content_hashes_match() {
        let mut mock = MockKoReaderDocumentHashRepository::new();
        mock.expect_insert_hashes()
            .withf(|_, _, hashes| hashes.len() == 1)
            .returning(|_, _, _| Box::pin(async { Ok(()) }));
        let svc = create_service(mock);
        let data = b"Foundation.epub";
        svc.register_hashes(1, "Foundation.epub", data).await.unwrap();
    }

    #[tokio::test]
    async fn find_book_by_digest_returns_book_id_on_match() {
        let mut mock = MockKoReaderDocumentHashRepository::new();
        mock.expect_find_book_by_digest_prefix().returning(|_, _| Box::pin(async { Ok(Some(42)) }));
        let svc = create_service(mock);
        let result = svc.find_book_by_digest("abcd1234").await.unwrap();
        assert_eq!(result, Some(42));
    }

    #[tokio::test]
    async fn find_book_by_digest_returns_none_for_unknown_prefix() {
        let mut mock = MockKoReaderDocumentHashRepository::new();
        mock.expect_find_book_by_digest_prefix().returning(|_, _| Box::pin(async { Ok(None) }));
        let svc = create_service(mock);
        let result = svc.find_book_by_digest("0000ffff").await.unwrap();
        assert_eq!(result, None);
    }
}
