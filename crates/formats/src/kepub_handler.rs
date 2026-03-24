use std::sync::Arc;

use bb_core::{
    CoreServices, Error, RepositoryError,
    book::{FileFormat, FileRole, book_slug},
    jobs::{JobHandler, JobRepositoryExt},
    repository::{RepositoryService, read_only_transaction, transaction},
};
use bb_utils::hash::hash_file;

use crate::conversion::ConvertKepubPayload;

pub struct ConvertKepubHandler {
    core: Arc<CoreServices>,
}

impl ConvertKepubHandler {
    #[must_use]
    pub fn new(core: Arc<CoreServices>) -> Self {
        Self { core }
    }
}

impl JobHandler for ConvertKepubHandler {
    const JOB_TYPE: &'static str = "convert_kepub";
    type Payload = ConvertKepubPayload;

    async fn handle(&self, payload: ConvertKepubPayload) -> Result<(), Error> {
        let book_id = payload.book_id;

        // ── 1. Load book, files, and first author in a read transaction ──────
        let repo = self.core.repository_service.clone();
        let (book, files, first_author_name) = read_only_transaction(&**self.core.repository_service.repository(), |tx| {
            let repo = repo.clone();
            Box::pin(async move {
                let book_repo = repo.book_repository().clone();
                let author_repo = repo.author_repository().clone();

                let book = book_repo
                    .find_by_id(tx, book_id)
                    .await?
                    .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;
                let files = book_repo.files_for_book(tx, book_id).await?;

                let book_authors = book_repo.authors_for_book(tx, book_id).await?;
                let first_author_name = if let Some(link) = book_authors.first() {
                    author_repo.find_by_id(tx, link.author_id).await?.map(|a| a.name)
                } else {
                    None
                };

                Ok((book, files, first_author_name))
            })
        })
        .await?;

        // ── 2. Locate the Enriched EPUB — the source for KEPUB conversion ─────
        let enriched_epub = files
            .iter()
            .find(|f| f.file_role == FileRole::Enriched && f.format == FileFormat::Epub)
            .ok_or_else(|| Error::Infrastructure(format!("book {book_id}: no enriched epub file record")))?;

        let source_path = self.core.file_store.resolve(&enriched_epub.path);

        // ── 3. Convert in a blocking thread ───────────────────────────────────
        let named_temp = tempfile::NamedTempFile::new().map_err(|e| Error::Infrastructure(format!("temp file: {e}")))?;
        let temp_path = named_temp.path().to_path_buf();
        let source_send = source_path.clone();
        let temp_send = temp_path.clone();

        tokio::task::spawn_blocking(move || crate::kepub_convert::convert_to_kepub(&source_send, &temp_send))
            .await
            .map_err(|e| Error::Infrastructure(format!("kepub conversion task panicked: {e}")))?
            .map_err(|e| Error::Infrastructure(e.to_string()))?;

        // ── 4. Hash and size the output ───────────────────────────────────────
        let file_hash = hash_file(&temp_path).await.map_err(|e| Error::Infrastructure(format!("hash failed: {e}")))?;
        let file_size = tokio::fs::metadata(&temp_path)
            .await
            .map_err(|e| Error::Infrastructure(format!("metadata failed: {e}")))?
            .len() as i64;

        // ── 5. Derive slug ────────────────────────────────────────────────────
        let slug = book_slug(&book.title, first_author_name.as_deref());

        // ── 6. Move converted file into the library ───────────────────────────
        let kepub_path = self.core.file_store.store_book_file(book.token, &slug, FileFormat::Kepub, &temp_path).await?;

        // ── 7. Upsert the Enriched Kepub book_file record ────────────────────
        let book_repo = self.core.repository_service.book_repository().clone();
        transaction(&**self.core.repository_service.repository(), |tx| {
            let book_repo = book_repo.clone();
            let file_hash = file_hash.clone();
            let kepub_path = kepub_path.clone();
            Box::pin(async move {
                book_repo.delete_book_file_by_role(tx, book_id, FileFormat::Kepub, FileRole::Enriched).await?;
                book_repo
                    .add_book_file(tx, book_id, FileFormat::Kepub, FileRole::Enriched, kepub_path, file_size, file_hash)
                    .await?;
                Ok(())
            })
        })
        .await?;

        tracing::info!(book_id, "KEPUB conversion complete");
        Ok(())
    }
}

/// Re-enqueues KEPUB conversion jobs for any books that have an Enriched EPUB
/// but no Enriched KEPUB. Call once at startup to recover from crashed jobs.
pub async fn recover_kepub_conversions(repository_service: &Arc<RepositoryService>) -> Result<(), Error> {
    let book_repo = repository_service.book_repository().clone();
    let job_repo = repository_service.job_repository().clone();

    let book_ids = read_only_transaction(&**repository_service.repository(), |tx| {
        let book_repo = book_repo.clone();
        Box::pin(async move { book_repo.find_book_ids_needing_kepub_conversion(tx).await })
    })
    .await?;

    if book_ids.is_empty() {
        return Ok(());
    }

    tracing::info!(count = book_ids.len(), "recovering books needing KEPUB conversion");

    transaction(&**repository_service.repository(), |tx| {
        let job_repo = job_repo.clone();
        Box::pin(async move {
            for book_id in book_ids {
                job_repo.enqueue(tx, &ConvertKepubPayload { book_id }).await?;
            }
            Ok(())
        })
    })
    .await?;

    Ok(())
}
