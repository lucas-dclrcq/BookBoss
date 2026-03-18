use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use bb_core::{
    Error,
    book::{Author, AuthorId, AuthorRole, Book, BookId, BookStatus, BookToken, FileFormat, NewAuthor, NewBook},
    conversion::ConversionService,
    import::{ImportJob, ImportJobId, ImportStatus, NewImportJob},
    pipeline::{ExtractedMetadata, MetadataExtractor, PipelineServiceImpl},
    repository::{RepositoryService, transaction},
    storage::{BookSidecar, LibraryStore},
    user::{NewUser, User},
};
use chrono::Utc;

// ── Silent library store
// ──────────────────────────────────────────────────────

/// A `LibraryStore` implementation that silently succeeds all operations.
/// Used in integration tests for code paths that touch the store.
pub struct SilentLibraryStore;

#[async_trait]
impl LibraryStore for SilentLibraryStore {
    fn resolve(&self, _relative_path: &str) -> PathBuf {
        PathBuf::new()
    }
    fn cover_path(&self, _token: &BookToken, _filename: &str) -> PathBuf {
        PathBuf::new()
    }
    fn metadata_path(&self, _token: &BookToken) -> PathBuf {
        PathBuf::new()
    }
    async fn store_original_file(&self, _source_hash: &str, original_filename: &str, _source: &Path) -> Result<String, Error> {
        Ok(format!("Originals/{original_filename}"))
    }
    async fn store_book_file(&self, token: &BookToken, slug: &str, _format: FileFormat, _source: &Path) -> Result<String, Error> {
        Ok(format!("{token}/{slug}.epub"))
    }
    async fn store_cover(&self, _token: &BookToken, _filename: &str, _data: &[u8]) -> Result<(), Error> {
        Ok(())
    }
    async fn store_metadata(&self, _token: &BookToken, _sidecar: &BookSidecar) -> Result<(), Error> {
        Ok(())
    }
    async fn rename_book_files(&self, _token: &BookToken, _old_slug: &str, _new_slug: &str) -> Result<(), Error> {
        Ok(())
    }
    async fn delete_book(&self, _token: &BookToken) -> Result<(), Error> {
        Ok(())
    }
    async fn delete_original_file(&self, _relative_path: &str) -> Result<(), Error> {
        Ok(())
    }
}

pub fn silent_library_store() -> Arc<dyn LibraryStore> {
    Arc::new(SilentLibraryStore)
}

// ── Silent conversion service
// ─────────────────────────────────────────────────

/// A `ConversionService` that silently succeeds all operations.
pub struct SilentConversionService;

#[async_trait]
impl ConversionService for SilentConversionService {
    async fn queue_enrich_epub(&self, _book_id: BookId) -> Result<(), Error> {
        Ok(())
    }
    async fn queue_convert_kepub(&self, _book_id: BookId) -> Result<(), Error> {
        Ok(())
    }
    async fn count_pending(&self) -> Result<u32, Error> {
        Ok(0)
    }
}

pub fn silent_conversion_service() -> Arc<dyn ConversionService> {
    Arc::new(SilentConversionService)
}

// ── Stub metadata extractor
// ──────────────────────────────────────────────────

/// A `MetadataExtractor` that returns a fixed `ExtractedMetadata` without
/// reading the file. Used to test the pipeline without real e-book files.
pub struct StubMetadataExtractor {
    pub metadata: ExtractedMetadata,
}

#[async_trait]
impl MetadataExtractor for StubMetadataExtractor {
    async fn extract(&self, _path: &Path, _format: FileFormat) -> Result<ExtractedMetadata, Error> {
        Ok(self.metadata.clone())
    }
}

// ── Pipeline service factory
// ──────────────────────────────────────────────────

/// Builds a `CoreServices` backed by a real `PipelineServiceImpl` using:
/// - The provided stub extractor
/// - `SilentLibraryStore` (no real file I/O)
/// - `SilentConversionService` (no-op enqueue)
/// - No metadata providers (extracted metadata is used as-is)
pub fn pipeline_services(ctx: &crate::context::TestContext, metadata: ExtractedMetadata) -> Arc<bb_core::CoreServices> {
    let extractor = Arc::new(StubMetadataExtractor { metadata });
    let pipeline = Arc::new(PipelineServiceImpl::new(
        ctx.repos.clone(),
        silent_library_store(),
        extractor,
        vec![],
        silent_conversion_service(),
    ));
    bb_core::create_services(
        ctx.repos.clone(),
        silent_library_store(),
        pipeline,
        silent_conversion_service(),
        "test-encryption-secret",
    )
    .unwrap()
}

// ── Fixture helpers
// ───────────────────────────────────────────────────────────

pub async fn insert_book(repos: &RepositoryService, title: &str, status: BookStatus) -> Book {
    let book_repo = repos.book_repository().clone();
    let title = title.to_owned();
    transaction(&**repos.repository(), |tx| {
        let book_repo = book_repo.clone();
        let title = title.clone();
        Box::pin(async move {
            book_repo
                .add_book(
                    tx,
                    NewBook {
                        title,
                        status,
                        description: None,
                        published_date: None,
                        language: None,
                        series_id: None,
                        series_number: None,
                        publisher_id: None,
                        page_count: None,
                        rating: None,
                        metadata_source: None,
                        cover_path: None,
                    },
                )
                .await
        })
    })
    .await
    .expect("insert_book fixture failed")
}

pub async fn insert_user(repos: &RepositoryService, username: &str) -> User {
    let user_repo = repos.user_repository().clone();
    let new_user = NewUser::new(
        username,
        "password123!",
        format!("{username}@example.com"),
        Default::default(),
        "Test User",
        false,
    )
    .expect("valid new user");
    transaction(&**repos.repository(), |tx| {
        let user_repo = user_repo.clone();
        Box::pin(async move { user_repo.add_user(tx, new_user).await })
    })
    .await
    .expect("insert_user fixture failed")
}

pub async fn insert_author(repos: &RepositoryService, name: &str) -> Author {
    let author_repo = repos.author_repository().clone();
    let name = name.to_owned();
    transaction(&**repos.repository(), |tx| {
        let author_repo = author_repo.clone();
        let name = name.clone();
        Box::pin(async move { author_repo.add_author(tx, NewAuthor { name, bio: None }).await })
    })
    .await
    .expect("insert_author fixture failed")
}

pub async fn link_book_author(repos: &RepositoryService, book_id: BookId, author_id: AuthorId) {
    let book_repo = repos.book_repository().clone();
    transaction(&**repos.repository(), |tx| {
        let book_repo = book_repo.clone();
        Box::pin(async move { book_repo.add_book_author(tx, book_id, author_id, AuthorRole::Author, 0).await })
    })
    .await
    .expect("link_book_author fixture failed");
}

pub async fn insert_import_job(repos: &RepositoryService, file_hash: &str) -> ImportJob {
    let job_repo = repos.import_job_repository().clone();
    let file_hash = file_hash.to_owned();
    transaction(&**repos.repository(), |tx| {
        let job_repo = job_repo.clone();
        let file_hash = file_hash.clone();
        Box::pin(async move {
            job_repo
                .add_job(
                    tx,
                    NewImportJob {
                        file_path: format!("/watch/{file_hash}.epub"),
                        file_hash,
                        file_format: FileFormat::Epub,
                        detected_at: Utc::now(),
                    },
                )
                .await
        })
    })
    .await
    .expect("insert_import_job fixture failed")
}

pub async fn link_job_to_book(repos: &RepositoryService, job: ImportJob, book_id: BookId) -> ImportJob {
    let tx = repos.repository().begin().await.expect("begin tx");
    let result = repos
        .import_job_repository()
        .update_job(
            &*tx,
            ImportJob {
                candidate_book_id: Some(book_id),
                ..job
            },
        )
        .await
        .expect("update_job (link_job_to_book)");
    tx.commit().await.expect("commit tx");
    result
}

pub async fn set_job_status(repos: &RepositoryService, job: ImportJob, status: ImportStatus) -> ImportJob {
    let tx = repos.repository().begin().await.expect("begin tx");
    let result = repos
        .import_job_repository()
        .update_job(&*tx, ImportJob { status, ..job })
        .await
        .expect("update_job (set_job_status)");
    tx.commit().await.expect("commit tx");
    result
}

pub async fn find_job_by_id(repos: &RepositoryService, id: ImportJobId) -> Option<ImportJob> {
    let tx = repos.repository().begin_read_only().await.expect("begin read-only tx");
    repos.import_job_repository().find_by_id(&*tx, id).await.expect("find_by_id")
}
