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
            Self::Epub => "epub",
            Self::Kepub => "kepub",
            Self::Mobi => "mobi",
            Self::Azw3 => "azw3",
            Self::Pdf => "pdf",
            Self::Cbz => "cbz",
        }
    }

    /// File extension for this format. KEPUB uses `"kepub.epub"`; all others
    /// match the canonical string.
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Kepub => "kepub.epub",
            other => other.as_str(),
        }
    }

    /// MIME content-type for HTTP responses.
    pub fn content_type(&self) -> &'static str {
        match self {
            Self::Epub | Self::Kepub => "application/epub+zip",
            Self::Mobi => "application/x-mobipocket-ebook",
            Self::Azw3 => "application/vnd.amazon.mobi8-ebook",
            Self::Pdf => "application/pdf",
            Self::Cbz => "application/vnd.comicbook+zip",
        }
    }

    /// Short uppercase label for display in the UI (`"EPUB"`, `"KEPUB"`, etc.).
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Epub => "EPUB",
            Self::Kepub => "KEPUB",
            Self::Mobi => "MOBI",
            Self::Azw3 => "AZW3",
            Self::Pdf => "PDF",
            Self::Cbz => "CBZ",
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
            "epub" => Ok(Self::Epub),
            "kepub" => Ok(Self::Kepub),
            "mobi" => Ok(Self::Mobi),
            "azw3" => Ok(Self::Azw3),
            "pdf" => Ok(Self::Pdf),
            "cbz" => Ok(Self::Cbz),
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
            Self::Original => "original",
            Self::Enriched => "enriched",
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
            "original" => Ok(Self::Original),
            "enriched" => Ok(Self::Enriched),
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
