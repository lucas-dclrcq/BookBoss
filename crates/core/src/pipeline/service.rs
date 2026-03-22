use std::{path::PathBuf, sync::Arc};

use bb_utils::similarity::{author_similarity, combined_score, title_similarity};

use crate::{
    Error, RepositoryError,
    book::{
        AuthorRole, BookStatus, BookToken, FileRole, IdentifierType, MetadataSource, NewAuthor, NewBook, NewGenre, NewPublisher, NewSeries, NewTag, book_slug,
    },
    conversion::ConversionService,
    event::EventService,
    import::{ImportJob, ImportJobToken, ImportSource, ImportStatus},
    pipeline::{
        MetadataExtractor, MetadataProvider, ProviderBook,
        model::{BookEdit, ExtractedIdentifier, ExtractedMetadata},
    },
    repository::{RepositoryService, read_only_transaction, transaction},
    storage::{BookSidecar, LibraryStore, SidecarAuthor, SidecarFile, SidecarIdentifier, SidecarSeries},
};

#[async_trait::async_trait]
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
#[allow(unused_lifetimes, reason = "async_trait + mockall expansion emits a spurious 'life0 parameter")]
pub trait PipelineService: Send + Sync {
    /// Processes an import job through the full acquisition pipeline:
    /// dedup → extract → enrich → create book → stage files → write sidecar.
    ///
    /// Returns the updated import job with `NeedsReview` status and
    /// `candidate_book_id` set, or `Rejected` if the file is a duplicate.
    async fn process_job(&self, job: ImportJob) -> Result<ImportJob, Error>;

    /// Rejects a `NeedsReview` import job, cleaning up all associated
    /// artifacts: removes the library directory, deletes the candidate book
    /// record, and deletes the import job record so the file can be
    /// re-imported if dropped again.
    async fn reject_job(&self, job_token: ImportJobToken) -> Result<(), Error>;

    /// Returns the human-readable names of all configured metadata providers,
    /// in priority order.
    fn list_provider_names(&self) -> Vec<&'static str>;

    /// Fetches metadata from a named provider.
    ///
    /// `title` and `identifiers` are used as search context. Returns `None`
    /// when the provider finds no match or has insufficient data to query.
    /// Returns an error if the provider name is unknown.
    ///
    /// If the result includes cover bytes they are written to the temp cover
    /// store keyed by `cover_key` for later retrieval (e.g. during
    /// `approve_job`).
    async fn fetch_from_provider(
        &self,
        provider_name: &str,
        title: Option<String>,
        identifiers: Vec<(IdentifierType, String)>,
        cover_key: &str,
        temp_dir: &std::path::Path,
    ) -> Result<Option<crate::pipeline::ProviderBook>, Error>;

    /// Approves a `NeedsReview` import job, committing the reviewer's edits to
    /// the database, transitioning the book to `Available`, and marking the
    /// import job as `Approved`.
    ///
    /// File renames (when title or primary author changed) and sidecar rewrites
    /// are performed as part of this operation.
    async fn approve_job(&self, job_token: ImportJobToken, edit: BookEdit, temp_dir: &std::path::Path) -> Result<(), Error>;

    /// Saves edits directly to an existing library book without going through
    /// the import pipeline. Intended for editing books already in the library.
    ///
    /// `cover_key` is the key used to look up any fetched cover bytes in the
    /// temp store (written there by `fetch_from_provider`). File renames and
    /// sidecar rewrites are performed when the title or primary author changes.
    async fn edit_book(&self, book_token: BookToken, edit: BookEdit, cover_key: &str, temp_dir: &std::path::Path) -> Result<(), Error>;
}

pub struct PipelineServiceImpl {
    repository_service: Arc<RepositoryService>,
    library_store: Arc<dyn LibraryStore>,
    extractor: Arc<dyn MetadataExtractor>,
    providers: Vec<Arc<dyn MetadataProvider>>,
    conversion_service: Arc<dyn ConversionService>,
    event_service: Arc<dyn EventService>,
}

impl PipelineServiceImpl {
    pub fn new(
        repository_service: Arc<RepositoryService>,
        library_store: Arc<dyn LibraryStore>,
        extractor: Arc<dyn MetadataExtractor>,
        providers: Vec<Arc<dyn MetadataProvider>>,
        conversion_service: Arc<dyn ConversionService>,
        event_service: Arc<dyn EventService>,
    ) -> Self {
        Self {
            repository_service,
            library_store,
            extractor,
            providers,
            conversion_service,
            event_service,
        }
    }
}

/// Minimum title similarity score [0.0, 1.0] required for a provider result
/// to be accepted. Results below this threshold are discarded and embedded
/// metadata is used instead.
const MATCH_THRESHOLD: f32 = 0.5;

/// Parse image dimensions from raw bytes by inspecting format-specific headers.
/// Supports PNG, GIF, WebP (VP8/VP8L/VP8X), and JPEG (SOF markers).
/// Returns `None` if the format is unrecognised or the header is truncated.
#[must_use]
pub fn image_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    // PNG: 8-byte signature followed by IHDR (width at 16, height at 20)
    if data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) && data.len() >= 24 {
        let w = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        let h = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
        return Some((w, h));
    }
    // GIF: "GIF87a" / "GIF89a" + 2-byte LE width + 2-byte LE height at offset 6
    if (data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a")) && data.len() >= 10 {
        let w = u32::from(u16::from_le_bytes([data[6], data[7]]));
        let h = u32::from(u16::from_le_bytes([data[8], data[9]]));
        return Some((w, h));
    }
    // WebP: RIFF....WEBP
    if data.len() >= 30 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        match &data[12..16] {
            b"VP8 " => {
                // Lossy: 14-bit LE width/height at offset 26/28 (mask 0x3FFF)
                let w = u32::from(u16::from_le_bytes([data[26], data[27]]) & 0x3FFF);
                let h = u32::from(u16::from_le_bytes([data[28], data[29]]) & 0x3FFF);
                return Some((w, h));
            }
            b"VP8L" if data.len() >= 25 => {
                // Lossless: packed bits at offset 21
                let bits = u32::from_le_bytes([data[21], data[22], data[23], data[24]]);
                let w = (bits & 0x3FFF) + 1;
                let h = ((bits >> 14) & 0x3FFF) + 1;
                return Some((w, h));
            }
            b"VP8X" => {
                // Extended: canvas width-1 (3 bytes LE) at 24, height-1 at 27
                let w = u32::from_le_bytes([data[24], data[25], data[26], 0]) + 1;
                let h = u32::from_le_bytes([data[27], data[28], data[29], 0]) + 1;
                return Some((w, h));
            }
            _ => {}
        }
    }
    // JPEG: scan for SOF marker (0xFF 0xCx, excluding DHT/DAC/RST variants)
    if data.starts_with(&[0xFF, 0xD8]) {
        let mut i = 2usize;
        while i + 3 < data.len() {
            if data[i] != 0xFF {
                break;
            }
            let marker = data[i + 1];
            if matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) && i + 8 < data.len() {
                let h = u32::from(u16::from_be_bytes([data[i + 5], data[i + 6]]));
                let w = u32::from(u16::from_be_bytes([data[i + 7], data[i + 8]]));
                return Some((w, h));
            }
            if i + 3 >= data.len() {
                break;
            }
            let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
            if len < 2 {
                break;
            }
            i += 2 + len;
        }
    }
    None
}

/// Returns the minimum side of an image's dimensions, used for quality
/// comparison.
fn cover_min_side(data: &[u8]) -> u32 {
    image_dimensions(data).map_or(0, |(w, h)| w.min(h))
}

/// Detect a cover image filename from leading magic bytes.
fn detect_cover_filename(data: &[u8]) -> &'static str {
    if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        "cover.png"
    } else if data.starts_with(&[0x47, 0x49, 0x46]) {
        "cover.gif"
    } else if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        "cover.webp"
    } else {
        "cover.jpg"
    }
}

/// Priority order for breaking ties between providers with equal scores.
/// Lower value = preferred. Unknown providers fall back to `u8::MAX`.
fn provider_priority(name: &str) -> u8 {
    match name {
        "Hardcover" => 0,
        "Google Books" => 1,
        "Open Library" => 2,
        _ => u8::MAX,
    }
}

/// Normalize a name string: trim edges and collapse interior whitespace runs
/// to a single space. Ensures "A  B" and "A B" resolve to the same author.
fn normalize_name(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[async_trait::async_trait]
impl PipelineService for PipelineServiceImpl {
    #[tracing::instrument(level = "trace", skip(self, job), fields(jobToken = %job.token))]
    async fn process_job(&self, mut job: ImportJob) -> Result<ImportJob, Error> {
        // Guard: only process jobs in Pending state. A duplicate queue entry
        // (e.g. from startup re-enqueue racing with a reset job) must not
        // overwrite a job that is already mid-flight or complete.
        if job.status != ImportStatus::Pending {
            tracing::debug!(import_job_id = job.id, status = ?job.status, "skipping import job not in pending state");
            return Ok(job);
        }

        // ── 1. Hash dedup: reject if file is already in the library ───────────
        {
            let book_repo = self.repository_service.book_repository().clone();
            let file_hash = job.file_hash.clone();
            let existing = read_only_transaction(&**self.repository_service.repository(), |tx| {
                Box::pin(async move { book_repo.find_file_by_hash(tx, &file_hash).await })
            })
            .await?;

            if existing.is_some() {
                let import_job_repo = self.repository_service.import_job_repository().clone();
                job.status = ImportStatus::Rejected;
                job.error_message = Some("File already exists in library".to_string());
                let j = job;
                return transaction(&**self.repository_service.repository(), |tx| {
                    Box::pin(async move { import_job_repo.update_job(tx, j).await })
                })
                .await;
            }
        }

        // ── 2. Mark Extracting ────────────────────────────────────────────────
        job = {
            let import_job_repo = self.repository_service.import_job_repository().clone();
            job.status = ImportStatus::Extracting;
            let j = job;
            transaction(&**self.repository_service.repository(), |tx| {
                Box::pin(async move { import_job_repo.update_job(tx, j).await })
            })
            .await?
        };

        // ── 3. Extract metadata from the e-book file ──────────────────────────
        let path: PathBuf = job.file_path.clone().into();
        let extracted = self.extractor.extract(&path, job.file_format.clone()).await?;

        // ── 4. Mark Identifying ───────────────────────────────────────────────
        job = {
            let import_job_repo = self.repository_service.import_job_repository().clone();
            job.status = ImportStatus::Identifying;
            let j = job;
            transaction(&**self.repository_service.repository(), |tx| {
                Box::pin(async move { import_job_repo.update_job(tx, j).await })
            })
            .await?
        };

        // ── 5. Enrich: query all providers concurrently, score results by title
        //    similarity, and pick the highest-scoring match above the threshold.
        //
        // If the file already contains spinnaker:metadata it was previously
        // enriched by BookBoss. Treat the embedded metadata as authoritative
        // and skip all external providers.
        //
        // Cover selection: pick the largest image from any provider result
        // (subtask 5 will refine this to only consider confident providers).
        let (final_meta, cover_bytes, job_source) = {
            let embedded_cover = extracted.cover_bytes.clone();

            if extracted.has_spinnaker_metadata {
                (extracted, embedded_cover, ImportSource::Embedded)
            } else {
                // Spawn one task per provider and run all concurrently.
                let mut join_set = tokio::task::JoinSet::new();
                for provider in &self.providers {
                    let provider = Arc::clone(provider);
                    let ex = extracted.clone();
                    join_set.spawn(async move {
                        let name = provider.name();
                        let result = provider.enrich(&ex).await;
                        (name, result)
                    });
                }

                // Collect successful results; log and discard errors.
                let mut provider_results: Vec<(&'static str, ProviderBook)> = Vec::new();
                while let Some(join_result) = join_set.join_next().await {
                    match join_result {
                        Ok((name, Ok(Some(pb)))) => provider_results.push((name, pb)),
                        Ok((name, Ok(None))) => tracing::debug!(provider = name, "provider returned no result"),
                        Ok((name, Err(e))) => tracing::warn!(provider = name, error = %e, "provider error during enrichment"),
                        Err(e) => tracing::warn!(error = %e, "provider task panicked during enrichment"),
                    }
                }

                // Score each result against the embedded title and primary author.
                // Scores are stored in a parallel vec so they can be reused for
                // both metadata selection and cover selection below.
                let extracted_title = extracted.title.as_deref();
                let extracted_author = extracted
                    .authors
                    .as_deref()
                    .and_then(|a| a.iter().min_by_key(|a| a.sort_order))
                    .map(|a| a.name.as_str());
                let scores: Vec<f32> = provider_results
                    .iter()
                    .map(|(name, pb)| {
                        let t_score = match (extracted_title, pb.metadata.title.as_deref()) {
                            (Some(ext), Some(prov)) => title_similarity(ext, prov),
                            _ => 0.0,
                        };
                        let a_score = match (
                            extracted_author,
                            pb.metadata.authors.as_deref().and_then(|a| a.iter().min_by_key(|a| a.sort_order)),
                        ) {
                            (Some(ext), Some(prov)) => author_similarity(ext, &prov.name),
                            _ => 0.5,
                        };
                        let score = combined_score(t_score, a_score);
                        tracing::debug!(provider = name, title_score = t_score, author_score = a_score, score, "provider result scored");
                        score
                    })
                    .collect();

                // Pick the highest-scoring result above MATCH_THRESHOLD,
                // breaking ties by provider priority.
                let mut best_idx: Option<usize> = None;
                let mut best_score: f32 = -1.0;
                let mut best_priority = u8::MAX;
                for (i, (name, _)) in provider_results.iter().enumerate() {
                    let score = scores[i];
                    if score >= MATCH_THRESHOLD {
                        let priority = provider_priority(name);
                        #[allow(
                            clippy::float_cmp,
                            reason = "scores are computed by the same deterministic formula; exact equality is intentional"
                        )]
                        let is_better = score > best_score || (score == best_score && priority < best_priority);
                        if is_better {
                            best_score = score;
                            best_priority = priority;
                            best_idx = Some(i);
                        }
                    }
                }

                // Cover: pick the largest image from the embedded EPUB cover and
                // any provider that scored above MATCH_THRESHOLD. Covers from
                // providers that failed to match are excluded.
                let mut best_cover = embedded_cover;
                let mut best_min_side = best_cover.as_deref().map_or(0, cover_min_side);
                let mut cover_source: Option<&str> = None;
                for ((name, pb), &score) in provider_results.iter().zip(scores.iter()) {
                    if score >= MATCH_THRESHOLD {
                        if let Some(cover) = &pb.cover_bytes {
                            let min_side = cover_min_side(cover);
                            if min_side > best_min_side {
                                best_min_side = min_side;
                                best_cover = Some(cover.clone());
                                cover_source = Some(name);
                            }
                        }
                    }
                }
                tracing::debug!(provider = cover_source, min_side = best_min_side, "cover selected");

                if let Some(idx) = best_idx {
                    let (source_name, pb) = provider_results.swap_remove(idx);
                    tracing::debug!(provider = source_name, score = best_score, "selected provider result");
                    let source = pb.source;
                    let mut metadata = pb.metadata;
                    // Preserve file-embedded fields not returned by the provider.
                    if let Some(extracted_ids) = &extracted.identifiers {
                        let provider_ids = metadata.identifiers.get_or_insert_with(Vec::new);
                        let existing_types: std::collections::HashSet<IdentifierType> = provider_ids.iter().map(|id| id.identifier_type.clone()).collect();
                        for id in extracted_ids {
                            if !existing_types.contains(&id.identifier_type) {
                                provider_ids.push(id.clone());
                            }
                        }
                    }
                    if metadata.publisher.is_none() {
                        metadata.publisher.clone_from(&extracted.publisher);
                    }
                    if metadata.language.is_none() {
                        metadata.language.clone_from(&extracted.language);
                    }
                    if metadata.genres.is_empty() {
                        metadata.genres.clone_from(&extracted.genres);
                    }
                    if metadata.tags.is_empty() {
                        metadata.tags.clone_from(&extracted.tags);
                    }
                    if metadata.page_count.is_none() {
                        metadata.page_count = extracted.page_count;
                    }
                    (metadata, best_cover, source)
                } else {
                    tracing::debug!("no provider result met match threshold; using embedded metadata");
                    (extracted, best_cover, ImportSource::Embedded)
                }
            }
        };
        let job_source = Some(job_source);

        // ── 6. Resolve cover filename from magic bytes ─────────────────────────
        let cover_filename: Option<String> = cover_bytes.as_deref().map(|b| detect_cover_filename(b).to_string());

        // ── 7. Capture file size before the file is moved ─────────────────────
        #[expect(
            clippy::cast_possible_wrap,
            reason = "file size stored as i64 in DB; files in practice will not exceed i64::MAX bytes"
        )]
        let file_size = tokio::fs::metadata(&path).await.map_or(0, |m| m.len() as i64);

        // ── 7a. Store original file in Originals/ before the transaction ───────
        // Must happen before the DB transaction so the actual filename (possibly
        // collision-resolved) is available to pass into add_book_file.
        // Uses copy semantics — source file is preserved for store_book_file.
        let intended_filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
        let actual_original_filename = self.library_store.store_original_file(&job.file_hash, &intended_filename, &path).await?;

        // ── 8. Determine title (fall back to filename stem) ───────────────────
        let title = normalize_name(
            &final_meta
                .title
                .clone()
                .unwrap_or_else(|| path.file_stem().and_then(|s| s.to_str()).unwrap_or("Unknown").to_string()),
        );

        // ── 9. Map ImportSource → MetadataSource for the Book record ──────────
        let book_metadata_source: Option<MetadataSource> = job_source.as_ref().map(|s| match s {
            ImportSource::Embedded => MetadataSource::Manual,
            ImportSource::OpenLibrary => MetadataSource::OpenLibrary,
            ImportSource::Hardcover => MetadataSource::Hardcover,
            ImportSource::GoogleBooks => MetadataSource::GoogleBooks,
        });

        // ── 10. Pre-build sidecar sub-structures from final_meta ──────────────
        let sidecar_authors: Vec<SidecarAuthor> = final_meta
            .authors
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|a| SidecarAuthor {
                name: a.name.clone(),
                role: a.role.clone().unwrap_or(AuthorRole::Author),
                sort_order: a.sort_order,
                file_as: None,
            })
            .collect();

        let sidecar_identifiers: Vec<SidecarIdentifier> = final_meta
            .identifiers
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|i| SidecarIdentifier {
                identifier_type: i.identifier_type.clone(),
                value: i.value.clone(),
            })
            .collect();

        // ── 11. DB writes in a single transaction ──────────────────────────────
        let book_repo = self.repository_service.book_repository().clone();
        let author_repo = self.repository_service.author_repository().clone();
        let series_repo = self.repository_service.series_repository().clone();
        let publisher_repo = self.repository_service.publisher_repository().clone();
        let genre_repo = self.repository_service.genre_repository().clone();
        let tag_repo = self.repository_service.tag_repository().clone();
        let import_job_repo = self.repository_service.import_job_repository().clone();

        let fm = final_meta.clone();
        let bms = book_metadata_source.clone();
        let cover_fn = cover_filename.clone();
        let js = job_source.clone();
        let file_hash = job.file_hash.clone();
        let file_format = job.file_format.clone();
        let original_filename = actual_original_filename.clone();
        let title_c = title.clone();
        let mut job_c = job;

        let (book, updated_job) = transaction(&**self.repository_service.repository(), |tx| {
            Box::pin(async move {
                // Find or create publisher
                let publisher_id = match &fm.publisher {
                    Some(name) => {
                        let name = normalize_name(name);
                        match publisher_repo.find_by_name(tx, &name).await? {
                            Some(p) => Some(p.id),
                            None => Some(publisher_repo.add_publisher(tx, NewPublisher { name }).await?.id),
                        }
                    }
                    None => None,
                };

                // Find or create series
                let (series_id, series_number) = match &fm.series_name {
                    Some(name) => {
                        let name = normalize_name(name);
                        let s = match series_repo.find_by_name(tx, &name).await? {
                            Some(s) => s,
                            None => series_repo.add_series(tx, NewSeries { name, description: None }).await?,
                        };
                        (Some(s.id), fm.series_number)
                    }
                    None => (None, None),
                };

                // Create the candidate book record
                let book = book_repo
                    .add_book(
                        tx,
                        NewBook {
                            title: title_c,
                            status: BookStatus::Incoming,
                            description: fm.description.clone(),
                            published_date: fm.published_date,
                            language: fm.language.clone(),
                            series_id,
                            series_number,
                            publisher_id,
                            page_count: fm.page_count,
                            rating: None,
                            metadata_source: bms,
                            cover_path: cover_fn,
                        },
                    )
                    .await?;

                // Record the book file — path set to the library-relative location
                // returned by store_original_file (updated to full relative path in M9.3).
                book_repo
                    .add_book_file(tx, book.id, file_format, FileRole::Original, original_filename, file_size, file_hash)
                    .await?;

                // Find or create each author, then link to book
                for a in fm.authors.as_deref().unwrap_or(&[]) {
                    let name = normalize_name(&a.name);
                    let author = match author_repo.find_by_name(tx, &name).await? {
                        Some(ex) => ex,
                        None => author_repo.add_author(tx, NewAuthor { name, bio: None }).await?,
                    };
                    let role = a.role.clone().unwrap_or(AuthorRole::Author);
                    book_repo.add_book_author(tx, book.id, author.id, role, a.sort_order).await?;
                }

                // Add identifiers, deduplicating by type (keep first occurrence)
                let mut seen_types = std::collections::HashSet::new();
                for id in fm.identifiers.as_deref().unwrap_or(&[]) {
                    if seen_types.insert(id.identifier_type.clone()) {
                        book_repo.add_book_identifier(tx, book.id, id.identifier_type.clone(), id.value.clone()).await?;
                    }
                }

                // Add genres
                for name in &fm.genres {
                    let name = normalize_name(name);
                    if name.is_empty() {
                        continue;
                    }
                    let genre = match genre_repo.find_by_name(tx, &name).await? {
                        Some(g) => g,
                        None => genre_repo.add_genre(tx, NewGenre { name }).await?,
                    };
                    book_repo.add_book_genre(tx, book.id, genre.id).await?;
                }

                // Add tags
                for name in &fm.tags {
                    let name = normalize_name(name);
                    if name.is_empty() {
                        continue;
                    }
                    let tag = match tag_repo.find_by_name(tx, &name).await? {
                        Some(t) => t,
                        None => tag_repo.add_tag(tx, NewTag { name }).await?,
                    };
                    book_repo.add_book_tag(tx, book.id, tag.id).await?;
                }

                // Advance import job to NeedsReview with candidate book linked
                job_c.status = ImportStatus::NeedsReview;
                job_c.candidate_book_id = Some(book.id);
                job_c.metadata_source = js;
                let updated_job = import_job_repo.update_job(tx, job_c).await?;

                Ok((book, updated_job))
            })
        })
        .await?;

        // ── 12. Store book file (moves it into the library directory) ──────────
        let slug = {
            let first_author = final_meta.authors.as_deref().and_then(|a| a.first()).map(|a| a.name.as_str());
            book_slug(&book.title, first_author)
        };
        self.library_store
            .store_book_file(book.token, &slug, updated_job.file_format.clone(), &path)
            .await?;

        // ── 13. Store cover image ──────────────────────────────────────────────
        if let (Some(filename), Some(data)) = (&cover_filename, &cover_bytes) {
            self.library_store.store_cover(book.token, filename, data).await?;
        }

        // ── 14. Write metadata sidecar ────────────────────────────────────────
        let sidecar = BookSidecar {
            title: book.title.clone(),
            authors: sidecar_authors,
            description: final_meta.description,
            publisher: final_meta.publisher,
            published_date: final_meta.published_date,
            language: final_meta.language,
            identifiers: sidecar_identifiers,
            series: final_meta.series_name.map(|name| SidecarSeries {
                name,
                number: final_meta.series_number,
            }),
            genres: final_meta.genres.clone(),
            tags: final_meta.tags.clone(),
            page_count: final_meta.page_count,
            status: BookStatus::Incoming,
            metadata_source: book_metadata_source,
            files: vec![SidecarFile {
                format: updated_job.file_format.clone(),
                hash: updated_job.file_hash.clone(),
            }],
        };
        self.library_store.store_metadata(book.token, &sidecar).await?;

        self.event_service.notify_incoming_changed();

        Ok(updated_job)
    }

    fn list_provider_names(&self) -> Vec<&'static str> {
        self.providers.iter().map(|p| p.name()).collect()
    }

    #[tracing::instrument(level = "trace", skip(self, identifiers, temp_dir), fields(coverKey = cover_key, provider = provider_name))]
    async fn fetch_from_provider(
        &self,
        provider_name: &str,
        title: Option<String>,
        identifiers: Vec<(IdentifierType, String)>,
        cover_key: &str,
        temp_dir: &std::path::Path,
    ) -> Result<Option<crate::pipeline::ProviderBook>, Error> {
        let provider = self
            .providers
            .iter()
            .find(|p| p.name() == provider_name)
            .ok_or_else(|| Error::Validation(format!("unknown provider: {provider_name}")))?
            .clone();

        let extracted = ExtractedMetadata {
            title,
            identifiers: if identifiers.is_empty() {
                None
            } else {
                Some(
                    identifiers
                        .into_iter()
                        .map(|(identifier_type, value)| ExtractedIdentifier { identifier_type, value })
                        .collect(),
                )
            },
            ..Default::default()
        };

        let result = provider.enrich(&extracted).await?;

        // Persist cover bytes to temp store keyed by cover_key so the caller
        // (approve_job or save_book_metadata) can retrieve them without a
        // round-trip through the frontend.
        if let Some(pb) = &result {
            if let Some(cover) = &pb.cover_bytes {
                let cover_dir = temp_dir.join("bookboss-covers");
                tokio::fs::create_dir_all(&cover_dir)
                    .await
                    .map_err(|e| Error::Infrastructure(format!("failed to create temp cover dir: {e}")))?;
                let cover_path = cover_dir.join(cover_key);
                tokio::fs::write(&cover_path, cover)
                    .await
                    .map_err(|e| Error::Infrastructure(format!("failed to write temp cover: {e}")))?;
            }
        }

        Ok(result)
    }

    #[tracing::instrument(level = "trace", skip(self, edit, temp_dir), fields(jobToken = %job_token))]
    async fn approve_job(&self, job_token: ImportJobToken, edit: BookEdit, temp_dir: &std::path::Path) -> Result<(), Error> {
        // ── 1. Load job and guard status ──────────────────────────────────────
        let import_job_repo = self.repository_service.import_job_repository().clone();
        let job = read_only_transaction(&**self.repository_service.repository(), |tx| {
            Box::pin(async move { import_job_repo.find_by_token(tx, job_token).await })
        })
        .await?
        .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;

        if job.status != ImportStatus::NeedsReview {
            return Err(Error::Validation(format!("cannot approve job with status {:?}", job.status)));
        }

        let book_id = job
            .candidate_book_id
            .ok_or_else(|| Error::Validation("import job has no candidate book".into()))?;

        // ── 2. Load current book and first author for old-slug computation ─────
        let book_repo = self.repository_service.book_repository().clone();
        let (book, old_authors) = read_only_transaction(&**self.repository_service.repository(), |tx| {
            let br = book_repo.clone();
            Box::pin(async move {
                let book = br.find_by_id(tx, book_id).await?.ok_or(Error::RepositoryError(RepositoryError::NotFound))?;
                let authors = br.authors_for_book(tx, book_id).await?;
                Ok::<_, Error>((book, authors))
            })
        })
        .await?;

        let old_first_author_id = old_authors.iter().min_by_key(|a| a.sort_order).map(|a| a.author_id);

        let old_first_author_name = if let Some(author_id) = old_first_author_id {
            let author_repo = self.repository_service.author_repository().clone();
            read_only_transaction(&**self.repository_service.repository(), |tx| {
                Box::pin(async move { author_repo.find_by_id(tx, author_id).await.map(|a| a.map(|a| a.name)) })
            })
            .await?
        } else {
            None
        };

        let old_slug = book_slug(&book.title, old_first_author_name.as_deref());

        // ── 3. Read temp cover if requested ───────────────────────────────────
        let cover_data: Option<(Vec<u8>, String)> = if edit.use_fetched_cover {
            let cover_path = temp_dir.join("bookboss-covers").join(job_token.to_string());
            match tokio::fs::read(&cover_path).await {
                Ok(data) => {
                    let filename = detect_cover_filename(&data).to_string();
                    Some((data, filename))
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    tracing::warn!(
                        job_token = %job_token,
                        "use_fetched_cover requested but no temp cover file found; skipping"
                    );
                    None
                }
                Err(e) => return Err(Error::Infrastructure(format!("failed to read temp cover: {e}"))),
            }
        } else {
            None
        };

        // ── 4. DB transaction: update book + approve job ──────────────────────
        let book_repo2 = self.repository_service.book_repository().clone();
        let author_repo = self.repository_service.author_repository().clone();
        let series_repo = self.repository_service.series_repository().clone();
        let publisher_repo = self.repository_service.publisher_repository().clone();
        let genre_repo = self.repository_service.genre_repository().clone();
        let tag_repo = self.repository_service.tag_repository().clone();
        let import_job_repo2 = self.repository_service.import_job_repository().clone();
        let job_id = job.id;
        let cover_filename = cover_data.as_ref().map(|(_, f)| f.clone());
        let edit_c = edit.clone();
        let book_version = book.version;

        transaction(&**self.repository_service.repository(), |tx| {
            Box::pin(async move {
                // Resolve series
                let (series_id, series_number) = match &edit_c.series_name {
                    Some(name) if !name.is_empty() => {
                        let name = normalize_name(name);
                        let s = match series_repo.find_by_name(tx, &name).await? {
                            Some(s) => s,
                            None => series_repo.add_series(tx, NewSeries { name, description: None }).await?,
                        };
                        (Some(s.id), edit_c.series_number)
                    }
                    _ => (None, None),
                };

                // Resolve publisher
                let publisher_id = match &edit_c.publisher_name {
                    Some(name) if !name.is_empty() => {
                        let name = normalize_name(name);
                        match publisher_repo.find_by_name(tx, &name).await? {
                            Some(p) => Some(p.id),
                            None => Some(publisher_repo.add_publisher(tx, NewPublisher { name }).await?.id),
                        }
                    }
                    _ => None,
                };

                // Update book record
                let mut updated_book = book_repo2
                    .find_by_id(tx, book_id)
                    .await?
                    .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;
                if updated_book.version != book_version {
                    return Err(Error::RepositoryError(RepositoryError::Conflict));
                }
                updated_book.title = normalize_name(&edit_c.title);
                updated_book.description = edit_c.description;
                updated_book.published_date = edit_c.published_date;
                updated_book.language = edit_c.language;
                updated_book.series_id = series_id;
                updated_book.series_number = series_number;
                updated_book.publisher_id = publisher_id;
                updated_book.page_count = edit_c.page_count;
                updated_book.status = BookStatus::Available;
                if let Some(filename) = cover_filename {
                    updated_book.cover_path = Some(filename);
                }
                book_repo2.update_book(tx, updated_book).await?;

                // Replace authors
                book_repo2.delete_book_authors(tx, book_id).await?;
                for (sort_order, name) in edit_c.authors.iter().enumerate() {
                    let name = normalize_name(name);
                    if name.is_empty() {
                        continue;
                    }
                    let author = match author_repo.find_by_name(tx, &name).await? {
                        Some(a) => a,
                        None => author_repo.add_author(tx, NewAuthor { name, bio: None }).await?,
                    };
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_possible_wrap,
                        reason = "author list index; books have far fewer authors than i32::MAX"
                    )]
                    let sort_order = sort_order as i32;
                    book_repo2.add_book_author(tx, book_id, author.id, AuthorRole::Author, sort_order).await?;
                }

                // Replace identifiers (deduplicate by type, keep first)
                book_repo2.delete_book_identifiers(tx, book_id).await?;
                let mut seen_types = std::collections::HashSet::new();
                for (id_type, value) in &edit_c.identifiers {
                    if value.is_empty() {
                        continue;
                    }
                    if seen_types.insert(id_type.clone()) {
                        book_repo2.add_book_identifier(tx, book_id, id_type.clone(), value.clone()).await?;
                    }
                }

                // Replace genres
                book_repo2.delete_book_genres(tx, book_id).await?;
                for name in &edit_c.genres {
                    let name = name.trim();
                    if name.is_empty() {
                        continue;
                    }
                    let genre = match genre_repo.find_by_name(tx, name).await? {
                        Some(g) => g,
                        None => genre_repo.add_genre(tx, NewGenre { name: name.to_string() }).await?,
                    };
                    book_repo2.add_book_genre(tx, book_id, genre.id).await?;
                }

                // Replace tags
                book_repo2.delete_book_tags(tx, book_id).await?;
                for name in &edit_c.tags {
                    let name = name.trim();
                    if name.is_empty() {
                        continue;
                    }
                    let tag = match tag_repo.find_by_name(tx, name).await? {
                        Some(t) => t,
                        None => tag_repo.add_tag(tx, NewTag { name: name.to_string() }).await?,
                    };
                    book_repo2.add_book_tag(tx, book_id, tag.id).await?;
                }

                // Mark import job approved
                import_job_repo2.approve_job(tx, job_id).await?;

                Ok(())
            })
        })
        .await?;

        // ── 5. Store fetched cover ────────────────────────────────────────────
        if let Some((cover_bytes, cover_filename)) = cover_data {
            self.library_store.store_cover(book.token, &cover_filename, &cover_bytes).await?;
            // Clean up temp file
            let cover_path = temp_dir.join("bookboss-covers").join(job_token.to_string());
            let _ = tokio::fs::remove_file(&cover_path).await;
        }

        // ── 6. Rename book files if slug changed ──────────────────────────────
        let new_first_author = edit.authors.first().map(|a| normalize_name(a));
        let new_slug = book_slug(&normalize_name(&edit.title), new_first_author.as_deref());

        if old_slug != new_slug {
            self.library_store.rename_book_files(book.token, &old_slug, &new_slug).await?;

            // Update DB paths for enriched files to match the new slug.
            let book_repo_rename = self.repository_service.book_repository().clone();
            let old_slug_c = old_slug.clone();
            let new_slug_c = new_slug.clone();
            transaction(&**self.repository_service.repository(), |tx| {
                let br = book_repo_rename.clone();
                let old = old_slug_c.clone();
                let new = new_slug_c.clone();
                Box::pin(async move { br.update_enriched_paths(tx, book_id, &old, &new).await })
            })
            .await?;
        }

        // ── 7. Rewrite metadata sidecar ───────────────────────────────────────
        let sidecar_authors: Vec<SidecarAuthor> = edit
            .authors
            .iter()
            .filter(|n| !n.is_empty())
            .enumerate()
            .map(|(i, name)| {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_possible_wrap,
                    reason = "author list index; books have far fewer authors than i32::MAX"
                )]
                let sort_order = i as i32;
                SidecarAuthor {
                    name: normalize_name(name),
                    role: AuthorRole::Author,
                    sort_order,
                    file_as: None,
                }
            })
            .collect();

        let sidecar_identifiers: Vec<SidecarIdentifier> = {
            let mut seen = std::collections::HashSet::new();
            edit.identifiers
                .iter()
                .filter(|(t, v)| !v.is_empty() && seen.insert(t.clone()))
                .map(|(t, v)| SidecarIdentifier {
                    identifier_type: t.clone(),
                    value: v.clone(),
                })
                .collect()
        };

        let book_file = {
            let book_repo3 = self.repository_service.book_repository().clone();
            read_only_transaction(&**self.repository_service.repository(), |tx| {
                Box::pin(async move { book_repo3.files_for_book(tx, book_id).await })
            })
            .await?
            .into_iter()
            .next()
        };

        let sidecar = BookSidecar {
            title: normalize_name(&edit.title),
            authors: sidecar_authors,
            description: edit.description,
            publisher: edit.publisher_name,
            published_date: edit.published_date,
            language: edit.language,
            identifiers: sidecar_identifiers,
            series: edit.series_name.filter(|n| !n.is_empty()).map(|name| SidecarSeries {
                name,
                number: edit.series_number,
            }),
            genres: edit.genres.iter().map(|g| g.trim().to_string()).filter(|g| !g.is_empty()).collect(),
            tags: edit.tags.iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect(),
            page_count: edit.page_count,
            status: BookStatus::Available,
            metadata_source: None,
            files: book_file
                .map(|f| {
                    vec![SidecarFile {
                        format: f.format,
                        hash: f.file_hash,
                    }]
                })
                .unwrap_or_default(),
        };
        self.library_store.store_metadata(book.token, &sidecar).await?;

        // ── 8. Enqueue EPUB enrichment ────────────────────────────────────────
        self.conversion_service.queue_enrich_epub(book_id).await?;

        self.event_service.notify_incoming_changed();
        self.event_service.notify_jobs_changed();

        Ok(())
    }

    #[tracing::instrument(level = "trace", skip(self, edit, temp_dir), fields(bookToken = %book_token))]
    async fn edit_book(&self, book_token: BookToken, edit: BookEdit, cover_key: &str, temp_dir: &std::path::Path) -> Result<(), Error> {
        // ── 1. Load book and compute old slug for rename detection ─────────────
        let book_repo = self.repository_service.book_repository().clone();
        let (book, old_authors) = read_only_transaction(&**self.repository_service.repository(), |tx| {
            let br = book_repo.clone();
            Box::pin(async move {
                let book = br
                    .find_by_token(tx, book_token)
                    .await?
                    .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;
                let book_id = book.id;
                let authors = br.authors_for_book(tx, book_id).await?;
                Ok::<_, Error>((book, authors))
            })
        })
        .await?;

        let book_id = book.id;

        let old_first_author_id = old_authors.iter().min_by_key(|a| a.sort_order).map(|a| a.author_id);
        let old_first_author_name = if let Some(author_id) = old_first_author_id {
            let author_repo = self.repository_service.author_repository().clone();
            read_only_transaction(&**self.repository_service.repository(), |tx| {
                Box::pin(async move { author_repo.find_by_id(tx, author_id).await.map(|a| a.map(|a| a.name)) })
            })
            .await?
        } else {
            None
        };

        let old_slug = book_slug(&book.title, old_first_author_name.as_deref());

        // ── 2. Read temp cover if requested ───────────────────────────────────
        let cover_data: Option<(Vec<u8>, String)> = if edit.use_fetched_cover {
            let cover_path = temp_dir.join("bookboss-covers").join(cover_key);
            match tokio::fs::read(&cover_path).await {
                Ok(data) => {
                    let filename = detect_cover_filename(&data).to_string();
                    Some((data, filename))
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    tracing::warn!(cover_key, "use_fetched_cover requested but no temp cover file found; skipping");
                    None
                }
                Err(e) => return Err(Error::Infrastructure(format!("failed to read temp cover: {e}"))),
            }
        } else {
            None
        };

        // ── 3. DB transaction: update book ────────────────────────────────────
        let book_repo2 = self.repository_service.book_repository().clone();
        let author_repo = self.repository_service.author_repository().clone();
        let series_repo = self.repository_service.series_repository().clone();
        let publisher_repo = self.repository_service.publisher_repository().clone();
        let genre_repo = self.repository_service.genre_repository().clone();
        let tag_repo = self.repository_service.tag_repository().clone();
        let cover_filename = cover_data.as_ref().map(|(_, f)| f.clone());
        let edit_c = edit.clone();
        let book_version = book.version;

        transaction(&**self.repository_service.repository(), |tx| {
            Box::pin(async move {
                // Resolve series
                let (series_id, series_number) = match &edit_c.series_name {
                    Some(name) if !name.is_empty() => {
                        let name = normalize_name(name);
                        let s = match series_repo.find_by_name(tx, &name).await? {
                            Some(s) => s,
                            None => series_repo.add_series(tx, NewSeries { name, description: None }).await?,
                        };
                        (Some(s.id), edit_c.series_number)
                    }
                    _ => (None, None),
                };

                // Resolve publisher
                let publisher_id = match &edit_c.publisher_name {
                    Some(name) if !name.is_empty() => {
                        let name = normalize_name(name);
                        match publisher_repo.find_by_name(tx, &name).await? {
                            Some(p) => Some(p.id),
                            None => Some(publisher_repo.add_publisher(tx, NewPublisher { name }).await?.id),
                        }
                    }
                    _ => None,
                };

                // Update book record
                let mut updated_book = book_repo2
                    .find_by_id(tx, book_id)
                    .await?
                    .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;
                if updated_book.version != book_version {
                    return Err(Error::RepositoryError(RepositoryError::Conflict));
                }
                updated_book.title = normalize_name(&edit_c.title);
                updated_book.description = edit_c.description;
                updated_book.published_date = edit_c.published_date;
                updated_book.language = edit_c.language;
                updated_book.series_id = series_id;
                updated_book.series_number = series_number;
                updated_book.publisher_id = publisher_id;
                updated_book.page_count = edit_c.page_count;
                if let Some(filename) = cover_filename {
                    updated_book.cover_path = Some(filename);
                }
                book_repo2.update_book(tx, updated_book).await?;

                // Replace authors
                book_repo2.delete_book_authors(tx, book_id).await?;
                for (sort_order, name) in edit_c.authors.iter().enumerate() {
                    let name = normalize_name(name);
                    if name.is_empty() {
                        continue;
                    }
                    let author = match author_repo.find_by_name(tx, &name).await? {
                        Some(a) => a,
                        None => author_repo.add_author(tx, NewAuthor { name, bio: None }).await?,
                    };
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_possible_wrap,
                        reason = "author list index; books have far fewer authors than i32::MAX"
                    )]
                    let sort_order = sort_order as i32;
                    book_repo2.add_book_author(tx, book_id, author.id, AuthorRole::Author, sort_order).await?;
                }

                // Replace identifiers (deduplicate by type, keep first)
                book_repo2.delete_book_identifiers(tx, book_id).await?;
                let mut seen_types = std::collections::HashSet::new();
                for (id_type, value) in &edit_c.identifiers {
                    if value.is_empty() {
                        continue;
                    }
                    if seen_types.insert(id_type.clone()) {
                        book_repo2.add_book_identifier(tx, book_id, id_type.clone(), value.clone()).await?;
                    }
                }

                // Replace genres
                book_repo2.delete_book_genres(tx, book_id).await?;
                for name in &edit_c.genres {
                    let name = name.trim();
                    if name.is_empty() {
                        continue;
                    }
                    let genre = match genre_repo.find_by_name(tx, name).await? {
                        Some(g) => g,
                        None => genre_repo.add_genre(tx, NewGenre { name: name.to_string() }).await?,
                    };
                    book_repo2.add_book_genre(tx, book_id, genre.id).await?;
                }

                // Replace tags
                book_repo2.delete_book_tags(tx, book_id).await?;
                for name in &edit_c.tags {
                    let name = name.trim();
                    if name.is_empty() {
                        continue;
                    }
                    let tag = match tag_repo.find_by_name(tx, name).await? {
                        Some(t) => t,
                        None => tag_repo.add_tag(tx, NewTag { name: name.to_string() }).await?,
                    };
                    book_repo2.add_book_tag(tx, book_id, tag.id).await?;
                }

                Ok(())
            })
        })
        .await?;

        // ── 4. Store fetched cover ─────────────────────────────────────────────
        if let Some((cover_bytes, cover_filename)) = cover_data {
            self.library_store.store_cover(book.token, &cover_filename, &cover_bytes).await?;
            let cover_path = temp_dir.join("bookboss-covers").join(cover_key);
            let _ = tokio::fs::remove_file(&cover_path).await;
        }

        // ── 5. Rename book files if slug changed ──────────────────────────────
        let new_first_author = edit.authors.first().map(|a| normalize_name(a));
        let new_slug = book_slug(&normalize_name(&edit.title), new_first_author.as_deref());

        if old_slug != new_slug {
            self.library_store.rename_book_files(book.token, &old_slug, &new_slug).await?;

            // Update DB paths for enriched files to match the new slug.
            let book_repo_rename = self.repository_service.book_repository().clone();
            let old_slug_c = old_slug.clone();
            let new_slug_c = new_slug.clone();
            transaction(&**self.repository_service.repository(), |tx| {
                let br = book_repo_rename.clone();
                let old = old_slug_c.clone();
                let new = new_slug_c.clone();
                Box::pin(async move { br.update_enriched_paths(tx, book_id, &old, &new).await })
            })
            .await?;
        }

        // ── 6. Rewrite metadata sidecar ───────────────────────────────────────
        let sidecar_authors: Vec<SidecarAuthor> = edit
            .authors
            .iter()
            .filter(|n| !n.is_empty())
            .enumerate()
            .map(|(i, name)| {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_possible_wrap,
                    reason = "author list index; books have far fewer authors than i32::MAX"
                )]
                let sort_order = i as i32;
                SidecarAuthor {
                    name: normalize_name(name),
                    role: AuthorRole::Author,
                    sort_order,
                    file_as: None,
                }
            })
            .collect();

        let sidecar_identifiers: Vec<SidecarIdentifier> = {
            let mut seen = std::collections::HashSet::new();
            edit.identifiers
                .iter()
                .filter(|(t, v)| !v.is_empty() && seen.insert(t.clone()))
                .map(|(t, v)| SidecarIdentifier {
                    identifier_type: t.clone(),
                    value: v.clone(),
                })
                .collect()
        };

        let book_file = {
            let book_repo3 = self.repository_service.book_repository().clone();
            read_only_transaction(&**self.repository_service.repository(), |tx| {
                Box::pin(async move { book_repo3.files_for_book(tx, book_id).await })
            })
            .await?
            .into_iter()
            .next()
        };

        let sidecar = BookSidecar {
            title: normalize_name(&edit.title),
            authors: sidecar_authors,
            description: edit.description,
            publisher: edit.publisher_name,
            published_date: edit.published_date,
            language: edit.language,
            identifiers: sidecar_identifiers,
            series: edit.series_name.filter(|n| !n.is_empty()).map(|name| SidecarSeries {
                name,
                number: edit.series_number,
            }),
            genres: edit.genres.iter().map(|g| g.trim().to_string()).filter(|g| !g.is_empty()).collect(),
            tags: edit.tags.iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect(),
            page_count: edit.page_count,
            status: BookStatus::Available,
            metadata_source: None,
            files: book_file
                .map(|f| {
                    vec![SidecarFile {
                        format: f.format,
                        hash: f.file_hash,
                    }]
                })
                .unwrap_or_default(),
        };
        self.library_store.store_metadata(book.token, &sidecar).await?;

        // ── 7. Enqueue EPUB enrichment ────────────────────────────────────────
        self.conversion_service.queue_enrich_epub(book_id).await?;

        self.event_service.notify_jobs_changed();

        Ok(())
    }

    #[tracing::instrument(level = "trace", skip(self), fields(jobToken = %job_token))]
    async fn reject_job(&self, job_token: ImportJobToken) -> Result<(), Error> {
        let import_job_repo = self.repository_service.import_job_repository().clone();
        let job = read_only_transaction(&**self.repository_service.repository(), |tx| {
            Box::pin(async move { import_job_repo.find_by_token(tx, job_token).await })
        })
        .await?
        .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;

        if job.status != ImportStatus::NeedsReview {
            return Err(Error::Validation(format!("cannot reject job with status {:?}", job.status)));
        }

        // Collect cleanup targets, then delete DB records in one transaction.
        let (book_token, original_filenames) = if let Some(book_id) = job.candidate_book_id {
            let book_repo = self.repository_service.book_repository().clone();
            let author_repo = self.repository_service.author_repository().clone();

            transaction(&**self.repository_service.repository(), |tx| {
                Box::pin(async move {
                    let Some(book) = book_repo.find_by_id(tx, book_id).await? else {
                        return Ok((None, vec![]));
                    };

                    let author_links = book_repo.authors_for_book(tx, book.id).await?;
                    let author_ids: Vec<u64> = author_links.iter().map(|a| a.author_id).collect();

                    // Collect original file paths before the records are deleted.
                    let original_filenames: Vec<String> = book_repo
                        .files_for_book(tx, book.id)
                        .await?
                        .into_iter()
                        .filter(|f| f.file_role == FileRole::Original)
                        .map(|f| f.path)
                        .collect();

                    book_repo.delete_book_genres(tx, book.id).await?;
                    book_repo.delete_book_tags(tx, book.id).await?;
                    book_repo.delete_book_authors(tx, book.id).await?;
                    book_repo.delete_book_identifiers(tx, book.id).await?;
                    book_repo.delete_book(tx, book.id).await?;

                    // Orphan author cleanup.
                    for author_id in author_ids {
                        if book_repo.count_books_for_author(tx, author_id).await? == 0 {
                            author_repo.delete_author(tx, author_id).await?;
                        }
                    }

                    Ok((Some(book.token), original_filenames))
                })
            })
            .await?
        } else {
            (None, vec![])
        };

        // Delete the import job — candidate_book_id was SET NULL by FK when the
        // book record was deleted above, so the job is still findable by ID.
        let import_job_repo = self.repository_service.import_job_repository().clone();
        let job_id = job.id;
        transaction(&**self.repository_service.repository(), |tx| {
            Box::pin(async move { import_job_repo.delete_job(tx, job_id).await })
        })
        .await?;

        // Delete library files after all DB work is done.
        if let Some(token) = book_token {
            self.library_store.delete_book(token).await?;
        }
        for filename in original_filenames {
            self.library_store.delete_original_file(&filename).await?;
        }

        self.event_service.notify_incoming_changed();

        Ok(())
    }
}
