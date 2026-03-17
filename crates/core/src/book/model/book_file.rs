use std::{fmt, str::FromStr};

use crate::book::BookId;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FileFormat {
    Epub,
    Kepub,
    Mobi,
    Azw3,
    Pdf,
    Cbz,
}

impl FileFormat {
    /// Canonical database/wire string (`"epub"`, `"kepub"`, etc.).
    pub fn as_str(&self) -> &'static str {
        match self {
            FileFormat::Epub => "epub",
            FileFormat::Kepub => "kepub",
            FileFormat::Mobi => "mobi",
            FileFormat::Azw3 => "azw3",
            FileFormat::Pdf => "pdf",
            FileFormat::Cbz => "cbz",
        }
    }

    /// File extension for this format. KEPUB uses `"kepub.epub"`; all others
    /// match the canonical string.
    pub fn extension(&self) -> &'static str {
        match self {
            FileFormat::Kepub => "kepub.epub",
            other => other.as_str(),
        }
    }

    /// MIME content-type for HTTP responses.
    pub fn content_type(&self) -> &'static str {
        match self {
            FileFormat::Epub | FileFormat::Kepub => "application/epub+zip",
            FileFormat::Mobi => "application/x-mobipocket-ebook",
            FileFormat::Azw3 => "application/vnd.amazon.mobi8-ebook",
            FileFormat::Pdf => "application/pdf",
            FileFormat::Cbz => "application/vnd.comicbook+zip",
        }
    }

    /// Short uppercase label for display in the UI (`"EPUB"`, `"KEPUB"`, etc.).
    pub fn display_name(&self) -> &'static str {
        match self {
            FileFormat::Epub => "EPUB",
            FileFormat::Kepub => "KEPUB",
            FileFormat::Mobi => "MOBI",
            FileFormat::Azw3 => "AZW3",
            FileFormat::Pdf => "PDF",
            FileFormat::Cbz => "CBZ",
        }
    }
}

impl fmt::Display for FileFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for FileFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "epub" => Ok(FileFormat::Epub),
            "kepub" => Ok(FileFormat::Kepub),
            "mobi" => Ok(FileFormat::Mobi),
            "azw3" => Ok(FileFormat::Azw3),
            "pdf" => Ok(FileFormat::Pdf),
            "cbz" => Ok(FileFormat::Cbz),
            _ => Err(format!("unknown file format: {s}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FileRole {
    Original,
    Enriched,
}

impl FileRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            FileRole::Original => "original",
            FileRole::Enriched => "enriched",
        }
    }
}

impl fmt::Display for FileRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for FileRole {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "original" => Ok(FileRole::Original),
            "enriched" => Ok(FileRole::Enriched),
            _ => Err(format!("unknown file role: {s}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BookFile {
    pub book_id: BookId,
    pub format: FileFormat,
    pub file_role: FileRole,
    pub path: String,
    pub file_size: i64,
    pub file_hash: String,
}

impl BookFile {
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn fake(book_id: BookId, format: &str) -> Self {
        let format = format.parse::<FileFormat>().unwrap_or(FileFormat::Epub);
        Self {
            book_id,
            format,
            file_role: FileRole::Original,
            path: String::new(),
            file_size: 0,
            file_hash: String::new(),
        }
    }
}
