use std::{fmt, str::FromStr};

/// Canonical source for a book's metadata, used to decide whether
/// and where to re-fetch.
///
/// Distinct from `import::ImportSource`, which records which provider
/// was used during the import pipeline.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MetadataSource {
    Hardcover,
    GoogleBooks,
    OpenLibrary,
    /// Metadata was hand-entered or edited by an admin. Do not auto-re-fetch.
    Manual,
}

impl MetadataSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            MetadataSource::Hardcover => "hardcover",
            MetadataSource::GoogleBooks => "google_books",
            MetadataSource::OpenLibrary => "open_library",
            MetadataSource::Manual => "manual",
        }
    }
}

impl fmt::Display for MetadataSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for MetadataSource {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "hardcover" => Ok(MetadataSource::Hardcover),
            "google_books" => Ok(MetadataSource::GoogleBooks),
            "open_library" => Ok(MetadataSource::OpenLibrary),
            "manual" => Ok(MetadataSource::Manual),
            _ => Err(format!("unknown metadata source: {s}")),
        }
    }
}
