use crate::{Error, book::BookId};

#[async_trait::async_trait]
pub trait ConversionService: Send + Sync {
    /// Enqueue an `enrich_epub` job for the given book. Always enqueues a new
    /// job — if a job for this book is already pending or running, both will
    /// execute sequentially; the later one overwrites the earlier result.
    async fn queue_enrich_epub(&self, book_id: BookId) -> Result<(), Error>;

    /// Enqueue a `convert_kepub` job for the given book. Runs after
    /// `enrich_epub` completes — converts the enriched EPUB to KEPUB format.
    async fn queue_convert_kepub(&self, book_id: BookId) -> Result<(), Error>;

    /// Returns the total number of pending or running conversion jobs
    /// (`enrich_epub` + `convert_kepub` combined).
    async fn count_pending(&self) -> Result<u32, Error>;
}
