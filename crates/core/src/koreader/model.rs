use chrono::{DateTime, Utc};

use crate::book::BookId;

#[derive(Debug, Clone)]
pub struct KoReaderDocumentHash {
    pub id: u64,
    pub book_id: BookId,
    pub document_hash: String,
    pub created_at: DateTime<Utc>,
}
