use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::{
    Error,
    book::{BookToken, FileFormat},
};

#[async_trait]
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
#[allow(unused_lifetimes, reason = "async_trait + mockall expansion emits a spurious 'life0 parameter")]
pub trait FileStoreService: Send + Sync {
    // ── Path resolution (sync, no I/O) ──────────────────────────────────────

    /// Resolves a library-root-relative path to an absolute path.
    /// Use this to open files whose relative path is stored in `BookFile.path`.
    fn resolve(&self, relative_path: &str) -> PathBuf;

    /// Returns the path to a book's cover image:
    /// `{library}/BK_{token}/{filename}`.
    fn cover_path(&self, token: BookToken, filename: &str) -> PathBuf;

    /// Returns the path to a book's sidecar:
    /// `{library}/BK_{token}/metadata.opf`.
    fn metadata_path(&self, token: BookToken) -> PathBuf;

    // ── Filesystem I/O (async) ───────────────────────────────────────────────

    /// Moves or copies `source` into `Originals/`, creating the directory if
    /// needed. Tries `original_filename` first; if a file already exists there
    /// with a different hash, falls back to `{stem}_{source_hash_prefix}.{ext}`
    /// using the first 8 chars of `source_hash`.
    /// Returns the library-root-relative path actually used
    /// (e.g. `"Originals/Black Ice.epub"` or `"Originals/Black
    /// Ice_1a2b3c4d.epub"`).
    async fn store_original_file(&self, source_hash: &str, original_filename: &str, source: &Path) -> Result<String, Error>;

    /// Moves or copies the source file into the book's enriched directory.
    /// Returns the library-root-relative path of the stored file
    /// (e.g. `"BK_XXXXX/black-ice-brad-thor.epub"`).
    async fn store_book_file(&self, token: BookToken, slug: &str, format: FileFormat, source: &Path) -> Result<String, Error>;

    /// Writes raw bytes as the book's cover image. `filename` determines the
    /// file name within the book's directory (e.g. `"cover.jpg"`).
    async fn store_cover(&self, token: BookToken, filename: &str, data: &[u8]) -> Result<(), Error>;

    /// Renames all `{old_slug}.*` files in the book's directory to
    /// `{new_slug}.*`.
    async fn rename_book_files(&self, token: BookToken, old_slug: &str, new_slug: &str) -> Result<(), Error>;

    /// Removes the book's entire directory and all its contents.
    async fn delete_book(&self, token: BookToken) -> Result<(), Error>;

    /// Copies a single file from the book's directory to `Trash/`, creating
    /// the directory if needed. Overwrites any existing file with the same
    /// name.
    async fn copy_to_trash(&self, token: BookToken, file_name: &str) -> Result<(), Error>;

    /// Copies `source` (an absolute path, typically a bookdrop file) into
    /// `Trash/Bookdrop/`, creating the directory if needed. Overwrites any
    /// existing file with the same name.
    async fn copy_to_bookdrop_trash(&self, source: &Path) -> Result<(), Error>;

    /// Removes a file by its library-root-relative path. No-op if the file
    /// does not exist.
    async fn delete_original_file(&self, relative_path: &str) -> Result<(), Error>;

    /// Returns the paths of all regular files in `path` (non-recursive).
    /// Directories, symlinks, and other non-file entries are excluded.
    async fn list_files(&self, path: &Path) -> Result<Vec<PathBuf>, Error>;
}
