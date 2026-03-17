use std::{fmt, str::FromStr};

use crate::book::BookId;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum IdentifierType {
    Isbn10,
    Isbn13,
    Asin,
    GoogleBooks,
    OpenLibrary,
    Hardcover,
}

impl IdentifierType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Isbn10 => "isbn10",
            Self::Isbn13 => "isbn13",
            Self::Asin => "asin",
            Self::GoogleBooks => "google_books",
            Self::OpenLibrary => "open_library",
            Self::Hardcover => "hardcover",
        }
    }

    /// Human-readable label for display in the UI.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Isbn10 => "ISBN-10",
            Self::Isbn13 => "ISBN-13",
            Self::Asin => "ASIN",
            Self::GoogleBooks => "Google Books",
            Self::OpenLibrary => "Open Library",
            Self::Hardcover => "Hardcover",
        }
    }

    /// OPF scheme string for Dublin Core identifier metadata.
    pub fn opf_scheme(&self) -> &'static str {
        match self {
            Self::Isbn10 | Self::Isbn13 => "ISBN",
            Self::Asin => "ASIN",
            Self::GoogleBooks => "GoogleBooks",
            Self::OpenLibrary => "OpenLibrary",
            Self::Hardcover => "Hardcover",
        }
    }

    /// PascalCase key used in the review/edit form identifier map.
    pub fn form_key(&self) -> &'static str {
        match self {
            Self::Isbn13 => "Isbn13",
            Self::Isbn10 => "Isbn10",
            Self::Asin => "Asin",
            Self::GoogleBooks => "GoogleBooks",
            Self::OpenLibrary => "OpenLibrary",
            Self::Hardcover => "Hardcover",
        }
    }

    /// Parse from the PascalCase form key used in the review/edit form.
    pub fn from_form_key(key: &str) -> Option<Self> {
        match key {
            "Isbn13" => Some(Self::Isbn13),
            "Isbn10" => Some(Self::Isbn10),
            "Asin" => Some(Self::Asin),
            "GoogleBooks" => Some(Self::GoogleBooks),
            "OpenLibrary" => Some(Self::OpenLibrary),
            "Hardcover" => Some(Self::Hardcover),
            _ => None,
        }
    }
}

impl fmt::Display for IdentifierType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for IdentifierType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "isbn10" => Ok(Self::Isbn10),
            "isbn13" => Ok(Self::Isbn13),
            "asin" => Ok(Self::Asin),
            "google_books" => Ok(Self::GoogleBooks),
            "open_library" => Ok(Self::OpenLibrary),
            "hardcover" => Ok(Self::Hardcover),
            _ => Err(format!("unknown identifier type: {s}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BookIdentifier {
    pub book_id: BookId,
    pub identifier_type: IdentifierType,
    pub value: String,
}

impl BookIdentifier {
    #[cfg(any(test, feature = "test-support"))]
    pub fn fake(book_id: BookId, identifier_type: &str, value: impl Into<String>) -> Self {
        let identifier_type = identifier_type.parse::<IdentifierType>().unwrap_or(IdentifierType::Isbn13);
        Self {
            book_id,
            identifier_type,
            value: value.into(),
        }
    }
}
