use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::{
    Error,
    book::{BookToken, FileFormat},
    storage::BookSidecar,
};

#[async_trait]
pub trait LibraryStore: Send + Sync {
    // ── Path derivation (sync, no I/O) ──────────────────────────────────────

    /// Returns the path for an original file in the flat Originals directory:
    /// `{library}/Originals/{original_filename}`.
    fn original_file_path(&self, original_filename: &str) -> PathBuf;

    /// Returns the full path to a book's enriched file:
    /// `{library}/BK_{token}/{slug}.{ext}`.
    fn book_file_path(&self, token: &BookToken, slug: &str, format: FileFormat) -> PathBuf;

    /// Returns the path to a book's cover image:
    /// `{library}/BK_{token}/{filename}`.
    fn cover_path(&self, token: &BookToken, filename: &str) -> PathBuf;

    /// Returns the path to a book's sidecar:
    /// `{library}/BK_{token}/metadata.opf`.
    fn metadata_path(&self, token: &BookToken) -> PathBuf;

    // ── Filesystem I/O (async) ───────────────────────────────────────────────

    /// Moves or copies `source` into `Originals/`, creating the directory if
    /// needed. Tries `original_filename` first; if a file already exists there
    /// with a different hash, falls back to `{stem}_{source_hash_prefix}.{ext}`
    /// using the first 8 chars of `source_hash`.
    /// Returns the filename actually used (may differ from `original_filename`
    /// on collision).
    async fn store_original_file(&self, source_hash: &str, original_filename: &str, source: &Path) -> Result<String, Error>;

    /// Moves or copies the source file into the book's enriched directory.
    async fn store_book_file(&self, token: &BookToken, slug: &str, format: FileFormat, source: &Path) -> Result<(), Error>;

    /// Writes raw bytes as the book's cover image. `filename` determines the
    /// file name within the book's directory (e.g. `"cover.jpg"`).
    async fn store_cover(&self, token: &BookToken, filename: &str, data: &[u8]) -> Result<(), Error>;

    /// Serialises `sidecar` and writes it as `metadata.opf` in the book's
    /// directory.
    async fn store_metadata(&self, token: &BookToken, sidecar: &BookSidecar) -> Result<(), Error>;

    /// Renames all `{old_slug}.*` files in the book's directory to
    /// `{new_slug}.*`.
    async fn rename_book_files(&self, token: &BookToken, old_slug: &str, new_slug: &str) -> Result<(), Error>;

    /// Removes the book's entire directory and all its contents.
    async fn delete_book(&self, token: &BookToken) -> Result<(), Error>;

    /// Removes a file from the flat `Originals/` directory. No-op if the file
    /// does not exist.
    async fn delete_original_file(&self, original_filename: &str) -> Result<(), Error>;
}
