use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;

use crate::{
    Error,
    book::{Book, BookId, BookToken, FileFormat, IdentifierType},
    conversion::ConversionService,
    filter::BookFilter,
    import::{ImportJob, ImportJobToken, ImportScanner, scanner::MockImportScanner},
    library::{LibraryService, LibraryStats},
    pipeline::{BookEdit, PipelineService, ProviderBook},
    storage::{BookSidecar, LibraryStore},
};

pub struct NopConversionService;

#[async_trait]
impl ConversionService for NopConversionService {
    async fn queue_enrich_epub(&self, _book_id: BookId) -> Result<(), Error> {
        unimplemented!("NopConversionService")
    }
    async fn queue_convert_kepub(&self, _book_id: BookId) -> Result<(), Error> {
        unimplemented!("NopConversionService")
    }
    async fn count_pending(&self) -> Result<u32, Error> {
        unimplemented!("NopConversionService")
    }
}

#[must_use]
pub fn nop_conversion_service() -> Arc<dyn ConversionService> {
    Arc::new(NopConversionService)
}

/// No-op `LibraryStore` for use in tests and as a placeholder until
/// `LocalLibraryStore` is wired in during M3.8.
pub struct NopLibraryStore;

#[async_trait]
impl LibraryStore for NopLibraryStore {
    fn resolve(&self, _relative_path: &str) -> PathBuf {
        unimplemented!("NopLibraryStore")
    }
    fn cover_path(&self, _token: &BookToken, _filename: &str) -> PathBuf {
        unimplemented!("NopLibraryStore")
    }
    fn metadata_path(&self, _token: &BookToken) -> PathBuf {
        unimplemented!("NopLibraryStore")
    }
    async fn store_original_file(&self, _source_hash: &str, _original_filename: &str, _source: &Path) -> Result<String, Error> {
        unimplemented!("NopLibraryStore")
    }
    async fn store_book_file(&self, _token: &BookToken, _slug: &str, _format: FileFormat, _source: &Path) -> Result<String, Error> {
        unimplemented!("NopLibraryStore")
    }
    async fn store_cover(&self, _token: &BookToken, _filename: &str, _data: &[u8]) -> Result<(), Error> {
        unimplemented!("NopLibraryStore")
    }
    async fn store_metadata(&self, _token: &BookToken, _sidecar: &BookSidecar) -> Result<(), Error> {
        unimplemented!("NopLibraryStore")
    }
    async fn rename_book_files(&self, _token: &BookToken, _old_slug: &str, _new_slug: &str) -> Result<(), Error> {
        unimplemented!("NopLibraryStore")
    }
    async fn delete_book(&self, _token: &BookToken) -> Result<(), Error> {
        unimplemented!("NopLibraryStore")
    }
    async fn delete_original_file(&self, _relative_path: &str) -> Result<(), Error> {
        unimplemented!("NopLibraryStore")
    }
}

#[must_use]
pub fn nop_library_store() -> Arc<dyn LibraryStore> {
    Arc::new(NopLibraryStore)
}

pub struct NopPipelineService;

#[async_trait]
impl PipelineService for NopPipelineService {
    async fn process_job(&self, _job: ImportJob) -> Result<ImportJob, Error> {
        unimplemented!("NopPipelineService")
    }
    async fn reject_job(&self, _job_token: ImportJobToken) -> Result<(), Error> {
        unimplemented!("NopPipelineService")
    }
    fn list_provider_names(&self) -> Vec<&'static str> {
        unimplemented!("NopPipelineService")
    }
    async fn fetch_from_provider(
        &self,
        _provider_name: &str,
        _title: Option<String>,
        _identifiers: Vec<(IdentifierType, String)>,
        _cover_key: &str,
        _temp_dir: &std::path::Path,
    ) -> Result<Option<ProviderBook>, Error> {
        unimplemented!("NopPipelineService")
    }
    async fn approve_job(&self, _job_token: ImportJobToken, _edit: BookEdit, _temp_dir: &std::path::Path) -> Result<(), Error> {
        unimplemented!("NopPipelineService")
    }
    async fn edit_book(&self, _book_token: &BookToken, _edit: BookEdit, _cover_key: &str, _temp_dir: &std::path::Path) -> Result<(), Error> {
        unimplemented!("NopPipelineService")
    }
}

#[must_use]
pub fn nop_pipeline_service() -> Arc<dyn PipelineService> {
    Arc::new(NopPipelineService)
}

pub struct NopLibraryService;

#[async_trait]
impl LibraryService for NopLibraryService {
    async fn library_stats(&self) -> Result<LibraryStats, Error> {
        unimplemented!("NopLibraryService")
    }
    async fn search_books(&self, _filter: &BookFilter, _start_id: Option<BookId>, _page_size: Option<u64>) -> Result<Vec<Book>, Error> {
        unimplemented!("NopLibraryService")
    }
    async fn delete_book(&self, _token: &BookToken) -> Result<(), Error> {
        unimplemented!("NopLibraryService")
    }
}

#[must_use]
pub fn nop_library_service() -> Arc<dyn LibraryService> {
    Arc::new(NopLibraryService)
}

#[must_use]
pub fn nop_import_scanner() -> Arc<dyn ImportScanner> {
    let mut mock = MockImportScanner::new();
    mock.expect_trigger_scan().returning(|| Box::pin(async {}));
    Arc::new(mock)
}
