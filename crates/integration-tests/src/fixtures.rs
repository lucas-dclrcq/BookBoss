use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use bb_core::{
    Error,
    book::{Author, AuthorId, AuthorRole, Book, BookId, BookStatus, BookToken, FileFormat, NewAuthor, NewBook},
    format::FormatService,
    import::{ImportJob, ImportJobId, ImportStatus, NewImportJob},
    pipeline::{ExtractedMetadata, PipelineServiceImpl},
    repository::{RepositoryService, transaction},
    storage::{BookSidecar, FileStoreService},
    user::{NewUser, User},
};
use chrono::Utc;

// ── Silent library store
// ──────────────────────────────────────────────────────

/// A `FileStoreService` implementation that silently succeeds all operations.
/// Used in integration tests for code paths that touch the store.
pub struct SilentFileStore;

#[async_trait]
impl FileStoreService for SilentFileStore {
    fn resolve(&self, _relative_path: &str) -> PathBuf {
        PathBuf::new()
    }
    fn cover_path(&self, _token: BookToken, _filename: &str) -> PathBuf {
        PathBuf::new()
    }
    fn metadata_path(&self, _token: BookToken) -> PathBuf {
        PathBuf::new()
    }
    async fn store_original_file(&self, _source_hash: &str, original_filename: &str, _source: &Path) -> Result<String, Error> {
        Ok(format!("Originals/{original_filename}"))
    }
    async fn store_book_file(&self, token: BookToken, slug: &str, _format: FileFormat, _source: &Path) -> Result<String, Error> {
        Ok(format!("{token}/{slug}.epub"))
    }
    async fn store_cover(&self, _token: BookToken, _filename: &str, _data: &[u8]) -> Result<(), Error> {
        Ok(())
    }
    async fn rename_book_files(&self, _token: BookToken, _old_slug: &str, _new_slug: &str) -> Result<(), Error> {
        Ok(())
    }
    async fn copy_to_trash(&self, _token: BookToken, _file_name: &str) -> Result<(), Error> {
        Ok(())
    }
    async fn delete_book(&self, _token: BookToken) -> Result<(), Error> {
        Ok(())
    }
    async fn delete_original_file(&self, _relative_path: &str) -> Result<(), Error> {
        Ok(())
    }
}

pub fn silent_file_store() -> Arc<dyn FileStoreService> {
    Arc::new(SilentFileStore)
}

// ── Stub format service
// ──────────────────────────────────────────────────

/// A `FormatService` that returns fixed metadata and silently succeeds all
/// write operations. Used to test the pipeline without real e-book files.
pub struct StubFormatService {
    pub metadata: ExtractedMetadata,
}

#[async_trait]
impl FormatService for StubFormatService {
    fn detect_format(&self, _path: &Path) -> Option<FileFormat> {
        Some(FileFormat::Epub)
    }
    async fn extract_metadata(&self, _path: &Path) -> Result<(FileFormat, ExtractedMetadata), Error> {
        Ok((FileFormat::Epub, self.metadata.clone()))
    }
    async fn enrich(&self, _request: &bb_core::format::EnrichmentRequest) -> Result<(), Error> {
        Ok(())
    }
    async fn write_sidecar(&self, _path: &Path, _sidecar: &BookSidecar) -> Result<(), Error> {
        Ok(())
    }
    async fn read_sidecar(&self, _path: &Path) -> Result<BookSidecar, Error> {
        Ok(BookSidecar {
            title: String::new(),
            authors: vec![],
            description: None,
            publisher: None,
            published_date: None,
            language: None,
            identifiers: vec![],
            series: None,
            genres: vec![],
            tags: vec![],
            page_count: None,
            status: BookStatus::Incoming,
            metadata_source: None,
            files: vec![],
        })
    }
}

// ── Pipeline service factory
// ──────────────────────────────────────────────────

/// Builds a `CoreServices` backed by a real `PipelineServiceImpl` using:
/// - `StubFormatService` (returns provided metadata, no-op file ops)
/// - `SilentFileStore` (no real file I/O)
/// - No metadata providers (extracted metadata is used as-is)
pub fn pipeline_services(ctx: &crate::context::TestContext, metadata: ExtractedMetadata) -> Arc<bb_core::CoreServices> {
    let event_service = bb_core::test_support::nop_event_service();
    let job_service = bb_core::test_support::nop_job_service();
    let format_service: Arc<dyn FormatService> = Arc::new(StubFormatService { metadata });
    let pipeline = Arc::new(PipelineServiceImpl::new(
        ctx.repos.clone(),
        silent_file_store(),
        format_service.clone(),
        vec![],
        job_service.clone(),
        event_service.clone(),
    ));
    bb_core::create_services(
        bb_core::test_support::default_external_services_builder()
            .repository_service(ctx.repos.clone())
            .file_store(silent_file_store())
            .format_service(format_service)
            .pipeline_service(pipeline)
            .job_service(job_service)
            .event_service(event_service)
            .build()
            .unwrap(),
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

pub async fn find_book_by_id(repos: &RepositoryService, id: BookId) -> Option<Book> {
    let tx = repos.repository().begin_read_only().await.expect("begin read-only tx");
    repos.book_repository().find_by_id(&*tx, id).await.expect("find_by_id")
}

pub async fn find_job_by_id(repos: &RepositoryService, id: ImportJobId) -> Option<ImportJob> {
    let tx = repos.repository().begin_read_only().await.expect("begin read-only tx");
    repos.import_job_repository().find_by_id(&*tx, id).await.expect("find_by_id")
}
