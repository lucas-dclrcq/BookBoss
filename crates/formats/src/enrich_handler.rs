use std::sync::Arc;

use bb_core::{
    Error, RepositoryError,
    book::{AuthorRole, FileFormat, FileRole, book_slug},
    jobs::{JobHandler, JobRepositoryExt},
    repository::{RepositoryService, read_only_transaction, transaction},
    storage::{BookSidecar, LibraryStore, SidecarAuthor, SidecarIdentifier, SidecarSeries},
};
use bb_utils::hash::hash_file;

use crate::conversion::{ConvertKepubPayload, EnrichEpubPayload};

pub struct EnrichEpubHandler {
    repository_service: Arc<RepositoryService>,
    library_store: Arc<dyn LibraryStore>,
}

impl EnrichEpubHandler {
    #[must_use]
    pub fn new(repository_service: Arc<RepositoryService>, library_store: Arc<dyn LibraryStore>) -> Self {
        Self {
            repository_service,
            library_store,
        }
    }
}

impl JobHandler for EnrichEpubHandler {
    const JOB_TYPE: &'static str = "enrich_epub";
    type Payload = EnrichEpubPayload;

    async fn handle(&self, payload: EnrichEpubPayload) -> Result<(), Error> {
        let book_id = payload.book_id;

        // ── 1. Load all book data in a single read transaction ────────────────
        let repo = self.repository_service.clone();
        let (book, files, authors, identifiers, genres, tags, series_opt, publisher_opt) =
            read_only_transaction(&**self.repository_service.repository(), |tx| {
                let repo = repo.clone();
                Box::pin(async move {
                    let book_repo = repo.book_repository().clone();
                    let author_repo = repo.author_repository().clone();
                    let series_repo = repo.series_repository().clone();
                    let publisher_repo = repo.publisher_repository().clone();

                    let book = book_repo
                        .find_by_id(tx, book_id)
                        .await?
                        .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;

                    let files = book_repo.files_for_book(tx, book_id).await?;
                    let book_author_links = book_repo.authors_for_book(tx, book_id).await?;
                    let identifiers = book_repo.identifiers_for_book(tx, book_id).await?;
                    let genres = book_repo.genres_for_book(tx, book_id).await?;
                    let tags = book_repo.tags_for_book(tx, book_id).await?;

                    // Resolve full author records to get names
                    let mut authors: Vec<(String, AuthorRole, i32)> = Vec::new();
                    for link in &book_author_links {
                        if let Some(author) = author_repo.find_by_id(tx, link.author_id).await? {
                            authors.push((author.name, link.role.clone(), link.sort_order));
                        }
                    }

                    let series_opt = if let Some(sid) = book.series_id {
                        series_repo.find_by_id(tx, sid).await?
                    } else {
                        None
                    };

                    let publisher_opt = if let Some(pid) = book.publisher_id {
                        publisher_repo.find_by_id(tx, pid).await?
                    } else {
                        None
                    };

                    Ok((book, files, authors, identifiers, genres, tags, series_opt, publisher_opt))
                })
            })
            .await?;

        // ── 2. Find the Original EPUB file record ─────────────────────────────
        let original_file = files
            .iter()
            .find(|f| f.file_role == FileRole::Original && f.format == FileFormat::Epub)
            .ok_or_else(|| Error::Infrastructure(format!("book {book_id}: no original epub file record")))?;

        let source_path = self.library_store.resolve(&original_file.path);

        // ── 3. Load cover bytes (non-fatal if missing) ────────────────────────
        let cover_bytes: Option<Vec<u8>> = if let Some(cover_filename) = &book.cover_path {
            let cover_path = self.library_store.cover_path(book.token, cover_filename);
            match tokio::fs::read(&cover_path).await {
                Ok(data) => Some(data),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    tracing::warn!(book_id, "cover file not found, enriching without cover");
                    None
                }
                Err(e) => return Err(Error::Infrastructure(format!("book {book_id}: failed to read cover: {e}"))),
            }
        } else {
            None
        };

        // ── 4. Build sidecar ──────────────────────────────────────────────────
        let sidecar_authors: Vec<SidecarAuthor> = authors
            .iter()
            .map(|(name, role, sort_order)| SidecarAuthor {
                name: name.clone(),
                role: role.clone(),
                sort_order: *sort_order,
                file_as: None,
            })
            .collect();

        let sidecar_identifiers: Vec<SidecarIdentifier> = identifiers
            .iter()
            .map(|id| SidecarIdentifier {
                identifier_type: id.identifier_type.clone(),
                value: id.value.clone(),
            })
            .collect();

        let sidecar = BookSidecar {
            title: book.title.clone(),
            authors: sidecar_authors,
            description: book.description.clone(),
            publisher: publisher_opt.map(|p| p.name),
            published_date: book.published_date,
            language: book.language.clone(),
            identifiers: sidecar_identifiers,
            series: series_opt.map(|s| SidecarSeries {
                name: s.name,
                number: book.series_number,
            }),
            genres: genres.iter().map(|g| g.name.clone()).collect(),
            tags: tags.iter().map(|t| t.name.clone()).collect(),
            page_count: book.page_count,
            status: book.status.clone(),
            metadata_source: book.metadata_source.clone(),
            files: vec![],
        };

        // ── 5. Derive slug ────────────────────────────────────────────────────
        let first_author_name = authors.first().map(|(name, _, _)| name.as_str());
        let slug = book_slug(&book.title, first_author_name);

        // ── 6. Enrich EPUB in a blocking thread ───────────────────────────────
        let named_temp = tempfile::NamedTempFile::new().map_err(|e| Error::Infrastructure(format!("temp file: {e}")))?;
        let temp_path = named_temp.path().to_path_buf();

        let source_send = source_path.clone();
        let temp_send = temp_path.clone();
        let sidecar_send = sidecar.clone();
        let cover_send = cover_bytes.clone();

        tokio::task::spawn_blocking(move || crate::enrich_epub(&source_send, &temp_send, &sidecar_send, cover_send.as_deref()))
            .await
            .map_err(|e| Error::Infrastructure(format!("enrichment task panicked: {e}")))?
            .map_err(|e| Error::Infrastructure(e.to_string()))?;

        // ── 7. Hash and size the enriched file ────────────────────────────────
        let file_hash = hash_file(&temp_path).await.map_err(|e| Error::Infrastructure(format!("hash failed: {e}")))?;
        let file_size = tokio::fs::metadata(&temp_path)
            .await
            .map_err(|e| Error::Infrastructure(format!("metadata failed: {e}")))?
            .len() as i64;

        // ── 8. Move enriched file into the library ────────────────────────────
        let enriched_path = self.library_store.store_book_file(book.token, &slug, FileFormat::Epub, &temp_path).await?;

        // ── 9. Upsert the Enriched book_file record ───────────────────────────
        let book_repo = self.repository_service.book_repository().clone();
        transaction(&**self.repository_service.repository(), |tx| {
            let book_repo = book_repo.clone();
            let file_hash = file_hash.clone();
            let enriched_path = enriched_path.clone();
            Box::pin(async move {
                book_repo.delete_book_file_by_role(tx, book_id, FileFormat::Epub, FileRole::Enriched).await?;
                book_repo
                    .add_book_file(tx, book_id, FileFormat::Epub, FileRole::Enriched, enriched_path, file_size, file_hash)
                    .await?;
                Ok(())
            })
        })
        .await?;

        // ── 10. Enqueue KEPUB conversion as the next step in the chain ────────
        let job_repo = self.repository_service.job_repository().clone();
        transaction(&**self.repository_service.repository(), |tx| {
            let job_repo = job_repo.clone();
            Box::pin(async move {
                job_repo.enqueue(tx, &ConvertKepubPayload { book_id }).await?;
                Ok(())
            })
        })
        .await?;

        tracing::info!(book_id, "EPUB enrichment complete; KEPUB conversion enqueued");
        Ok(())
    }
}

/// Re-enqueues enrichment jobs for any books that have an Original EPUB but
/// no Enriched EPUB. Call once at startup to recover from crashed enrichment
/// jobs.
pub async fn recover_enrichments(repository_service: &Arc<RepositoryService>) -> Result<(), Error> {
    let book_repo = repository_service.book_repository().clone();
    let job_repo = repository_service.job_repository().clone();

    let book_ids = read_only_transaction(&**repository_service.repository(), |tx| {
        let book_repo = book_repo.clone();
        Box::pin(async move { book_repo.find_book_ids_needing_enrichment(tx).await })
    })
    .await?;

    if book_ids.is_empty() {
        return Ok(());
    }

    tracing::info!(count = book_ids.len(), "recovering books needing EPUB enrichment");

    transaction(&**repository_service.repository(), |tx| {
        let job_repo = job_repo.clone();
        Box::pin(async move {
            for book_id in book_ids {
                job_repo.enqueue(tx, &EnrichEpubPayload { book_id }).await?;
            }
            Ok(())
        })
    })
    .await?;

    Ok(())
}
