use std::{fmt, str::FromStr};

use bb_utils::{define_token_prefix, token::Token};
use chrono::{DateTime, Utc};

use crate::{
    book::{BookId, FileFormat},
    user::UserId,
};

define_token_prefix!(ImportJobTokenPrefix, "IJ_");
pub type ImportJobId = u64;
pub type ImportJobToken = Token<ImportJobTokenPrefix, ImportJobId, { i64::MAX as u128 }>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportStatus {
    Pending,
    Extracting,
    Identifying,
    NeedsReview,
    Approved,
    Rejected,
    Error,
}

impl ImportStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ImportStatus::Pending => "pending",
            ImportStatus::Extracting => "extracting",
            ImportStatus::Identifying => "identifying",
            ImportStatus::NeedsReview => "needs_review",
            ImportStatus::Approved => "approved",
            ImportStatus::Rejected => "rejected",
            ImportStatus::Error => "error",
        }
    }
}

impl fmt::Display for ImportStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ImportStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(ImportStatus::Pending),
            "extracting" => Ok(ImportStatus::Extracting),
            "identifying" => Ok(ImportStatus::Identifying),
            "needs_review" => Ok(ImportStatus::NeedsReview),
            "approved" => Ok(ImportStatus::Approved),
            "rejected" => Ok(ImportStatus::Rejected),
            "error" => Ok(ImportStatus::Error),
            _ => Err(format!("unknown import status: {s}")),
        }
    }
}

/// Which provider populated the metadata during the import pipeline.
///
/// Distinct from `book::MetadataSource`, which tracks the ongoing canonical
/// source for a book record and includes `Manual` for admin-edited entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportSource {
    /// Metadata extracted from the file itself (EPUB OPF, MOBI headers).
    Embedded,
    Hardcover,
    GoogleBooks,
    OpenLibrary,
}

impl ImportSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            ImportSource::Embedded => "embedded",
            ImportSource::Hardcover => "hardcover",
            ImportSource::GoogleBooks => "google_books",
            ImportSource::OpenLibrary => "open_library",
        }
    }
}

impl fmt::Display for ImportSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ImportSource {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "embedded" => Ok(ImportSource::Embedded),
            "hardcover" => Ok(ImportSource::Hardcover),
            "google_books" => Ok(ImportSource::GoogleBooks),
            "open_library" => Ok(ImportSource::OpenLibrary),
            _ => Err(format!("unknown import source: {s}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImportJob {
    pub id: ImportJobId,
    pub version: u64,
    pub token: ImportJobToken,
    pub file_path: String,
    pub file_hash: String,
    pub file_format: FileFormat,
    pub detected_at: DateTime<Utc>,
    pub status: ImportStatus,
    pub candidate_book_id: Option<BookId>,
    pub metadata_source: Option<ImportSource>,
    pub error_message: Option<String>,
    pub reviewed_by: Option<UserId>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Input for creating a new import job. Status always starts as `Pending`.
#[derive(Debug, Clone)]
pub struct NewImportJob {
    pub file_path: String,
    pub file_hash: String,
    pub file_format: FileFormat,
    pub detected_at: DateTime<Utc>,
}
