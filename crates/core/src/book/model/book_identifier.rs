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
            IdentifierType::Isbn10 => "isbn10",
            IdentifierType::Isbn13 => "isbn13",
            IdentifierType::Asin => "asin",
            IdentifierType::GoogleBooks => "google_books",
            IdentifierType::OpenLibrary => "open_library",
            IdentifierType::Hardcover => "hardcover",
        }
    }

    /// Human-readable label for display in the UI.
    pub fn display_name(&self) -> &'static str {
        match self {
            IdentifierType::Isbn10 => "ISBN-10",
            IdentifierType::Isbn13 => "ISBN-13",
            IdentifierType::Asin => "ASIN",
            IdentifierType::GoogleBooks => "Google Books",
            IdentifierType::OpenLibrary => "Open Library",
            IdentifierType::Hardcover => "Hardcover",
        }
    }

    /// OPF scheme string for Dublin Core identifier metadata.
    pub fn opf_scheme(&self) -> &'static str {
        match self {
            IdentifierType::Isbn10 | IdentifierType::Isbn13 => "ISBN",
            IdentifierType::Asin => "ASIN",
            IdentifierType::GoogleBooks => "GoogleBooks",
            IdentifierType::OpenLibrary => "OpenLibrary",
            IdentifierType::Hardcover => "Hardcover",
        }
    }

    /// PascalCase key used in the review/edit form identifier map.
    pub fn form_key(&self) -> &'static str {
        match self {
            IdentifierType::Isbn13 => "Isbn13",
            IdentifierType::Isbn10 => "Isbn10",
            IdentifierType::Asin => "Asin",
            IdentifierType::GoogleBooks => "GoogleBooks",
            IdentifierType::OpenLibrary => "OpenLibrary",
            IdentifierType::Hardcover => "Hardcover",
        }
    }

    /// Parse from the PascalCase form key used in the review/edit form.
    pub fn from_form_key(key: &str) -> Option<Self> {
        match key {
            "Isbn13" => Some(IdentifierType::Isbn13),
            "Isbn10" => Some(IdentifierType::Isbn10),
            "Asin" => Some(IdentifierType::Asin),
            "GoogleBooks" => Some(IdentifierType::GoogleBooks),
            "OpenLibrary" => Some(IdentifierType::OpenLibrary),
            "Hardcover" => Some(IdentifierType::Hardcover),
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
            "isbn10" => Ok(IdentifierType::Isbn10),
            "isbn13" => Ok(IdentifierType::Isbn13),
            "asin" => Ok(IdentifierType::Asin),
            "google_books" => Ok(IdentifierType::GoogleBooks),
            "open_library" => Ok(IdentifierType::OpenLibrary),
            "hardcover" => Ok(IdentifierType::Hardcover),
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
