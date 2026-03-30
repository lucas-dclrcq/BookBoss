use std::sync::Arc;

use crate::{
    CoreServices, Error, RepositoryError,
    book::{AuthorRole, BookId, compute_sidecar_fingerprint},
    format::handler::EnrichBookFilesPayload,
    jobs::{BookIdSweep, BookSweepPayload, JobHandler, JobServiceExt, run_book_id_sweep},
    repository::read_only_transaction,
};

pub struct ReconcileFingerprintsHandler {
    core: Arc<CoreServices>,
}

impl ReconcileFingerprintsHandler {
    #[must_use]
    pub fn new(core: Arc<CoreServices>) -> Self {
        Self { core }
    }
}

#[async_trait::async_trait]
impl BookIdSweep for ReconcileFingerprintsHandler {
    fn job_type(&self) -> &'static str {
        Self::JOB_TYPE
    }

    // Default `fetch_batch` — all available books, no SQL filtering.

    async fn process_batch(&self, core: &Arc<CoreServices>, ids: Vec<BookId>) -> Result<(), Error> {
        let mut checked: u32 = 0;
        let mut enqueued: u32 = 0;

        for book_id in ids {
            let repo = core.repository_service.clone();
            let (book, authors, genres, tags, series_opt, publisher_opt) = read_only_transaction(&**core.repository_service.repository(), |tx| {
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

                    let book_author_links = book_repo.authors_for_book(tx, book_id).await?;
                    let genres = book_repo.genres_for_book(tx, book_id).await?;
                    let tags = book_repo.tags_for_book(tx, book_id).await?;

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

                    Ok((book, authors, genres, tags, series_opt, publisher_opt))
                })
            })
            .await?;

            let mut author_names: Vec<&str> = authors.iter().map(|(name, _, _)| name.as_str()).collect();
            author_names.sort_unstable();
            let mut genre_names: Vec<&str> = genres.iter().map(|g| g.name.as_str()).collect();
            genre_names.sort_unstable();
            let mut tag_names: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();
            tag_names.sort_unstable();

            let expected = compute_sidecar_fingerprint(
                &book.title,
                &author_names,
                series_opt.as_ref().map(|s| s.name.as_str()),
                book.series_number.as_ref(),
                publisher_opt.as_ref().map(|p| p.name.as_str()),
                &genre_names,
                &tag_names,
                book.rating,
            );

            checked += 1;
            if book.sidecar_fingerprint.as_deref() != Some(expected.as_str()) {
                core.job_service.enqueue(&EnrichBookFilesPayload { book_id }).await?;
                enqueued += 1;
            }
        }

        tracing::info!(checked, enqueued, "fingerprint reconciliation batch");
        Ok(())
    }
}

impl JobHandler for ReconcileFingerprintsHandler {
    const JOB_TYPE: &'static str = "health.reconcile_fingerprints";
    type Payload = BookSweepPayload;

    async fn handle(&self, payload: BookSweepPayload) -> Result<(), Error> {
        run_book_id_sweep(self, &self.core, payload).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        book::{Book, BookStatus, BookToken, MetadataSource, repository::book::MockBookRepository},
        jobs::repository::MockJobRepository,
        repository::testing::default_repository_service_builder,
        test_support::*,
    };

    fn fake_job() -> crate::jobs::Job {
        crate::jobs::Job {
            id: 1,
            job_type: String::new(),
            payload: serde_json::json!({}),
            status: crate::jobs::JobStatus::Pending,
            priority: 0,
            attempt: 0,
            max_attempts: 3,
            version: 0,
            scheduled_at: chrono::Utc::now(),
            started_at: None,
            completed_at: None,
            error_message: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn base_book(id: BookId, fingerprint: Option<String>) -> Book {
        Book {
            id,
            version: 1,
            token: BookToken::generate(),
            title: "Dune".into(),
            status: BookStatus::Available,
            description: None,
            published_date: None,
            language: None,
            series_id: None,
            series_number: None,
            publisher_id: None,
            page_count: None,
            rating: None,
            metadata_source: Some(MetadataSource::Hardcover),
            cover_path: None,
            sidecar_fingerprint: fingerprint,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn fingerprint_for(book: &Book) -> String {
        compute_sidecar_fingerprint(&book.title, &[], None, None, None, &[], &[], book.rating)
    }

    #[tokio::test]
    async fn matching_fingerprint_no_enqueue() {
        let mut book_repo = MockBookRepository::new();

        let book = base_book(1, Some(fingerprint_for(&base_book(1, None))));
        let b = book.clone();
        book_repo.expect_find_by_id().returning(move |_, _| {
            let b = b.clone();
            Box::pin(async move { Ok(Some(b)) })
        });
        book_repo.expect_authors_for_book().returning(|_, _| Box::pin(async { Ok(vec![]) }));
        book_repo.expect_genres_for_book().returning(|_, _| Box::pin(async { Ok(vec![]) }));
        book_repo.expect_tags_for_book().returning(|_, _| Box::pin(async { Ok(vec![]) }));
        book_repo
            .expect_find_available_books_for_sweep()
            .returning(|_, _, _| Box::pin(std::future::ready(Ok(vec![1]))));

        let repo_service = Arc::new(default_repository_service_builder().book_repository(Arc::new(book_repo)).build().unwrap());
        let core = crate::create_services(
            default_external_services_builder().repository_service(repo_service).build().unwrap(),
            "test-secret",
        )
        .unwrap();

        let handler = ReconcileFingerprintsHandler::new(core);
        handler.handle(BookSweepPayload::default()).await.unwrap();
    }

    #[tokio::test]
    async fn mismatched_fingerprint_enqueues_enrichment() {
        let mut book_repo = MockBookRepository::new();
        let mut job_repo = MockJobRepository::new();

        let book = base_book(2, Some("wrong_hash".into()));
        let b = book.clone();
        book_repo.expect_find_by_id().returning(move |_, _| {
            let b = b.clone();
            Box::pin(async move { Ok(Some(b)) })
        });
        book_repo.expect_authors_for_book().returning(|_, _| Box::pin(async { Ok(vec![]) }));
        book_repo.expect_genres_for_book().returning(|_, _| Box::pin(async { Ok(vec![]) }));
        book_repo.expect_tags_for_book().returning(|_, _| Box::pin(async { Ok(vec![]) }));
        book_repo
            .expect_find_available_books_for_sweep()
            .returning(|_, _, _| Box::pin(std::future::ready(Ok(vec![2]))));

        job_repo
            .expect_enqueue_raw()
            .once()
            .returning(|_, _, _, _| Box::pin(std::future::ready(Ok(fake_job()))));

        let repo_service = Arc::new(
            default_repository_service_builder()
                .book_repository(Arc::new(book_repo))
                .job_repository(Arc::new(job_repo))
                .build()
                .unwrap(),
        );
        let core = crate::create_services(
            default_external_services_builder().repository_service(repo_service).build().unwrap(),
            "test-secret",
        )
        .unwrap();

        let handler = ReconcileFingerprintsHandler::new(core);
        handler.handle(BookSweepPayload::default()).await.unwrap();
    }

    #[tokio::test]
    async fn null_fingerprint_enqueues_enrichment() {
        let mut book_repo = MockBookRepository::new();
        let mut job_repo = MockJobRepository::new();

        let book = base_book(3, None);
        let b = book.clone();
        book_repo.expect_find_by_id().returning(move |_, _| {
            let b = b.clone();
            Box::pin(async move { Ok(Some(b)) })
        });
        book_repo.expect_authors_for_book().returning(|_, _| Box::pin(async { Ok(vec![]) }));
        book_repo.expect_genres_for_book().returning(|_, _| Box::pin(async { Ok(vec![]) }));
        book_repo.expect_tags_for_book().returning(|_, _| Box::pin(async { Ok(vec![]) }));
        book_repo
            .expect_find_available_books_for_sweep()
            .returning(|_, _, _| Box::pin(std::future::ready(Ok(vec![3]))));

        job_repo
            .expect_enqueue_raw()
            .once()
            .returning(|_, _, _, _| Box::pin(std::future::ready(Ok(fake_job()))));

        let repo_service = Arc::new(
            default_repository_service_builder()
                .book_repository(Arc::new(book_repo))
                .job_repository(Arc::new(job_repo))
                .build()
                .unwrap(),
        );
        let core = crate::create_services(
            default_external_services_builder().repository_service(repo_service).build().unwrap(),
            "test-secret",
        )
        .unwrap();

        let handler = ReconcileFingerprintsHandler::new(core);
        handler.handle(BookSweepPayload::default()).await.unwrap();
    }
}
