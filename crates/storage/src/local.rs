use std::path::{Path, PathBuf};

use async_trait::async_trait;
use bb_core::{
    Error,
    book::{BookToken, FileFormat},
    storage::FileStoreService,
};
use bb_utils::hash::hash_file;
use image::{ImageReader, codecs::jpeg::JpegEncoder, imageops::FilterType};

pub struct LocalFileStore {
    library_path: PathBuf,
}

impl LocalFileStore {
    #[must_use]
    pub fn new(library_path: PathBuf) -> Self {
        Self { library_path }
    }

    fn originals_dir(&self) -> PathBuf {
        self.library_path.join("Originals")
    }

    fn trash_dir(&self) -> PathBuf {
        self.library_path.join("Trash")
    }

    fn book_dir(&self, token: BookToken) -> PathBuf {
        self.library_path.join(token.to_string())
    }

    fn book_file_path(&self, token: BookToken, slug: &str, format: &FileFormat) -> PathBuf {
        self.book_dir(token).join(format!("{slug}.{}", format.extension()))
    }

    /// Returns the library-root-relative path for a book file
    /// (e.g. `"BK_XXXXX/slug.epub"`).
    fn book_file_rel_path(token: BookToken, slug: &str, format: &FileFormat) -> String {
        format!("{}/{}.{}", token, slug, format.extension())
    }
}

#[allow(clippy::needless_pass_by_value, reason = "owned error needed for map_err ergonomics")]
fn io_err(e: impl ToString) -> Error {
    Error::Infrastructure(e.to_string())
}

/// Converts any recognized image format to JPEG, resizing to fit within
/// 1024×1536 if needed, and re-encodes at quality 85.
///
/// Re-encoding strips all embedded metadata (EXIF, XMP, IPTC, ICC profiles)
/// and keeps covers small enough for Kobo firmware to decode reliably.
/// Falls back to the original bytes if decoding fails.
fn normalize_to_jpeg(data: &[u8]) -> Vec<u8> {
    const MAX_W: u32 = 1024;
    const MAX_H: u32 = 1536;
    const QUALITY: u8 = 85;

    let Ok(reader) = ImageReader::new(std::io::Cursor::new(data)).with_guessed_format() else {
        return data.to_vec();
    };
    let Ok(img) = reader.decode() else {
        return data.to_vec();
    };

    let img = if img.width() > MAX_W || img.height() > MAX_H {
        img.resize(MAX_W, MAX_H, FilterType::Lanczos3)
    } else {
        img
    };

    let mut out = Vec::new();
    if JpegEncoder::new_with_quality(&mut out, QUALITY).encode_image(&img).is_err() {
        return data.to_vec();
    }
    out
}

#[async_trait]
impl FileStoreService for LocalFileStore {
    fn resolve(&self, relative_path: &str) -> PathBuf {
        self.library_path.join(relative_path)
    }

    fn cover_path(&self, token: BookToken, filename: &str) -> PathBuf {
        self.book_dir(token).join(filename)
    }

    fn metadata_path(&self, token: BookToken) -> PathBuf {
        self.book_dir(token).join("metadata.opf")
    }

    async fn store_original_file(&self, source_hash: &str, original_filename: &str, source: &Path) -> Result<String, Error> {
        let originals_dir = self.originals_dir();
        tokio::fs::create_dir_all(&originals_dir).await.map_err(io_err)?;

        let preferred = originals_dir.join(original_filename);

        // Check for collision with an existing file.
        if tokio::fs::try_exists(&preferred).await.map_err(io_err)? {
            let existing_hash = hash_file(&preferred).await.map_err(|e| io_err(e.to_string()))?;
            if existing_hash == source_hash {
                // Same content already stored — idempotent, nothing to do.
                return Ok(format!("Originals/{original_filename}"));
            }
            // Different content: fall back to a hash-prefixed name.
            let hash_prefix = &source_hash[..8.min(source_hash.len())];
            let filename = {
                let path = std::path::Path::new(original_filename);
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(original_filename);
                let ext = path.extension().and_then(|s| s.to_str());
                match ext {
                    Some(e) => format!("{stem}_{hash_prefix}.{e}"),
                    None => format!("{stem}_{hash_prefix}"),
                }
            };
            let dest = originals_dir.join(&filename);
            tokio::fs::copy(source, &dest).await.map_err(io_err)?;
            return Ok(format!("Originals/{filename}"));
        }

        // No collision — copy to the preferred path.
        tokio::fs::copy(source, &preferred).await.map_err(io_err)?;
        Ok(format!("Originals/{original_filename}"))
    }

    async fn store_book_file(&self, token: BookToken, slug: &str, format: FileFormat, source: &Path) -> Result<String, Error> {
        let book_dir = self.book_dir(token);
        tokio::fs::create_dir_all(&book_dir).await.map_err(io_err)?;
        let dest = self.book_file_path(token, slug, &format);
        // Try rename first (fast, same filesystem)
        if tokio::fs::rename(source, &dest).await.is_err() {
            // Fall back to copy then remove source
            tokio::fs::copy(source, &dest).await.map_err(io_err)?;
            let _ = tokio::fs::remove_file(source).await;
        }
        Ok(Self::book_file_rel_path(token, slug, &format))
    }

    async fn store_cover(&self, token: BookToken, filename: &str, data: &[u8]) -> Result<(), Error> {
        let book_dir = self.book_dir(token);
        tokio::fs::create_dir_all(&book_dir).await.map_err(io_err)?;
        let cover_path = self.cover_path(token, filename);

        // Normalize all recognized image formats to JPEG: resize to fit within
        // 1024×1536 if needed, re-encode at quality 85, strip all metadata
        // (EXIF, XMP, IPTC, ICC). Non-image data (unrecognized magic bytes) is
        // stored as-is. Falls back to original bytes on any decode error.
        let is_recognized_image = data.starts_with(&[0xFF, 0xD8])
            || data.starts_with(&[0x89, 0x50, 0x4E, 0x47])
            || data.starts_with(&[0x47, 0x49, 0x46])
            || (data.len() >= 12 && data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP"));

        let bytes = if is_recognized_image { normalize_to_jpeg(data) } else { data.to_vec() };

        tokio::fs::write(cover_path, bytes).await.map_err(io_err)?;
        Ok(())
    }

    async fn rename_book_files(&self, token: BookToken, old_slug: &str, new_slug: &str) -> Result<(), Error> {
        let book_dir = self.book_dir(token);
        let prefix = format!("{old_slug}.");
        let mut entries = tokio::fs::read_dir(&book_dir).await.map_err(io_err)?;
        while let Some(entry) = entries.next_entry().await.map_err(io_err)? {
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();
            if let Some(ext) = name.strip_prefix(&prefix) {
                let new_name = format!("{new_slug}.{ext}");
                tokio::fs::rename(entry.path(), book_dir.join(new_name)).await.map_err(io_err)?;
            }
        }
        Ok(())
    }

    async fn copy_to_trash(&self, token: BookToken, file_name: &str) -> Result<(), Error> {
        let source = self.book_dir(token).join(file_name);
        let trash_dir = self.trash_dir();
        tokio::fs::create_dir_all(&trash_dir).await.map_err(io_err)?;
        let dest = trash_dir.join(file_name);
        tokio::fs::copy(&source, &dest).await.map_err(io_err)?;
        Ok(())
    }

    async fn delete_book(&self, token: BookToken) -> Result<(), Error> {
        let book_dir = self.book_dir(token);
        match tokio::fs::remove_dir_all(&book_dir).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io_err(e)),
        }
    }

    async fn delete_original_file(&self, relative_path: &str) -> Result<(), Error> {
        let path = self.resolve(relative_path);
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io_err(e)),
        }
    }

    async fn list_files(&self, path: &Path) -> Result<Vec<PathBuf>, Error> {
        let mut entries = tokio::fs::read_dir(path).await.map_err(io_err)?;
        let mut files = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(io_err)? {
            if entry.file_type().await.map_err(io_err)?.is_file() {
                files.push(entry.path());
            }
        }
        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use bb_core::{
        book::{BookToken, FileFormat},
        storage::FileStoreService,
    };
    use tempfile::tempdir;

    use super::LocalFileStore;

    fn test_store(library_path: std::path::PathBuf) -> LocalFileStore {
        LocalFileStore::new(library_path)
    }

    fn test_token() -> BookToken {
        BookToken::new(1)
    }

    #[tokio::test]
    async fn store_book_file_creates_at_expected_path() {
        let dir = tempdir().unwrap();
        let store = test_store(dir.path().to_path_buf());
        let token = test_token();

        // Create a source file to move
        let source = dir.path().join("source.epub");
        tokio::fs::write(&source, b"epub content").await.unwrap();

        let rel_path = store.store_book_file(token, "my-book", FileFormat::Epub, &source).await.unwrap();

        let expected = store.resolve(&rel_path);
        assert!(expected.exists(), "book file should exist at {expected:?}");
        let contents = tokio::fs::read(&expected).await.unwrap();
        assert_eq!(contents, b"epub content");
    }

    #[tokio::test]
    async fn store_book_file_returns_correct_relative_path() {
        let dir = tempdir().unwrap();
        let store = test_store(dir.path().to_path_buf());
        let token = test_token();

        let source = dir.path().join("source.epub");
        tokio::fs::write(&source, b"epub content").await.unwrap();

        let rel_path = store.store_book_file(token, "my-book", FileFormat::Epub, &source).await.unwrap();

        assert_eq!(rel_path, format!("{token}/my-book.epub"));
    }

    #[tokio::test]
    async fn store_original_file_returns_relative_path() {
        let dir = tempdir().unwrap();
        let store = test_store(dir.path().to_path_buf());

        let source = dir.path().join("original.epub");
        tokio::fs::write(&source, b"epub content").await.unwrap();

        let rel_path = store.store_original_file("fakehash", "original.epub", &source).await.unwrap();

        assert_eq!(rel_path, "Originals/original.epub");
        assert!(store.resolve(&rel_path).exists());
    }

    #[tokio::test]
    async fn store_cover_writes_to_cover_jpg() {
        let dir = tempdir().unwrap();
        let store = test_store(dir.path().to_path_buf());
        let token = test_token();

        let data = b"fake jpeg bytes";
        store.store_cover(token, "cover.jpg", data).await.unwrap();

        let cover = store.cover_path(token, "cover.jpg");
        assert!(cover.exists(), "cover.jpg should exist");
        let contents = tokio::fs::read(&cover).await.unwrap();
        assert_eq!(contents, data);
    }

    #[tokio::test]
    async fn rename_book_files_renames_correctly() {
        let dir = tempdir().unwrap();
        let store = test_store(dir.path().to_path_buf());
        let token = test_token();

        // Create book dir and some files
        let book_dir = dir.path().join(token.to_string());
        tokio::fs::create_dir_all(&book_dir).await.unwrap();
        tokio::fs::write(book_dir.join("old-slug.epub"), b"epub").await.unwrap();
        tokio::fs::write(book_dir.join("old-slug.pdf"), b"pdf").await.unwrap();
        tokio::fs::write(book_dir.join("cover.jpg"), b"cover").await.unwrap();

        store.rename_book_files(token, "old-slug", "new-slug").await.unwrap();

        assert!(book_dir.join("new-slug.epub").exists(), "epub should be renamed");
        assert!(book_dir.join("new-slug.pdf").exists(), "pdf should be renamed");
        assert!(!book_dir.join("old-slug.epub").exists(), "old epub should not exist");
        assert!(!book_dir.join("old-slug.pdf").exists(), "old pdf should not exist");
        // Non-matching file unchanged
        assert!(book_dir.join("cover.jpg").exists(), "cover.jpg should be untouched");
    }

    #[tokio::test]
    async fn copy_to_trash_copies_file() {
        let dir = tempdir().unwrap();
        let store = test_store(dir.path().to_path_buf());
        let token = test_token();

        let book_dir = dir.path().join(token.to_string());
        tokio::fs::create_dir_all(&book_dir).await.unwrap();
        tokio::fs::write(book_dir.join("my-book.epub"), b"enriched epub").await.unwrap();

        store.copy_to_trash(token, "my-book.epub").await.unwrap();

        let trash_file = dir.path().join("Trash").join("my-book.epub");
        assert!(trash_file.exists(), "file should exist in Trash/");
        let contents = tokio::fs::read(&trash_file).await.unwrap();
        assert_eq!(contents, b"enriched epub");
    }

    #[tokio::test]
    async fn copy_to_trash_overwrites_existing() {
        let dir = tempdir().unwrap();
        let store = test_store(dir.path().to_path_buf());
        let token = test_token();

        let book_dir = dir.path().join(token.to_string());
        tokio::fs::create_dir_all(&book_dir).await.unwrap();

        // Write an old version in Trash
        let trash_dir = dir.path().join("Trash");
        tokio::fs::create_dir_all(&trash_dir).await.unwrap();
        tokio::fs::write(trash_dir.join("my-book.epub"), b"old version").await.unwrap();

        // Write the new version in the book dir
        tokio::fs::write(book_dir.join("my-book.epub"), b"new version").await.unwrap();

        store.copy_to_trash(token, "my-book.epub").await.unwrap();

        let contents = tokio::fs::read(trash_dir.join("my-book.epub")).await.unwrap();
        assert_eq!(contents, b"new version");
    }

    #[tokio::test]
    async fn list_files_returns_only_files() {
        let dir = tempdir().unwrap();
        let store = test_store(dir.path().to_path_buf());

        let scan_dir = dir.path().join("scan");
        tokio::fs::create_dir_all(&scan_dir).await.unwrap();
        tokio::fs::write(scan_dir.join("book.epub"), b"epub").await.unwrap();
        tokio::fs::write(scan_dir.join("book.pdf"), b"pdf").await.unwrap();
        tokio::fs::create_dir_all(scan_dir.join("subdir")).await.unwrap();

        let mut files = store.list_files(&scan_dir).await.unwrap();
        files.sort();

        assert_eq!(files.len(), 2);
        assert!(files[0].ends_with("book.epub"));
        assert!(files[1].ends_with("book.pdf"));
    }

    #[tokio::test]
    async fn list_files_empty_directory() {
        let dir = tempdir().unwrap();
        let store = test_store(dir.path().to_path_buf());

        let scan_dir = dir.path().join("empty");
        tokio::fs::create_dir_all(&scan_dir).await.unwrap();

        let files = store.list_files(&scan_dir).await.unwrap();
        assert!(files.is_empty());
    }

    #[tokio::test]
    async fn list_files_nonexistent_directory_returns_error() {
        let dir = tempdir().unwrap();
        let store = test_store(dir.path().to_path_buf());

        let result = store.list_files(&dir.path().join("nope")).await;
        result.unwrap_err();
    }

    #[tokio::test]
    async fn delete_book_removes_directory() {
        let dir = tempdir().unwrap();
        let store = test_store(dir.path().to_path_buf());
        let token = test_token();

        let book_dir = dir.path().join(token.to_string());
        tokio::fs::create_dir_all(&book_dir).await.unwrap();
        tokio::fs::write(book_dir.join("test.epub"), b"data").await.unwrap();

        store.delete_book(token).await.unwrap();
        assert!(!book_dir.exists(), "book dir should be removed");

        // Second call is a no-op
        store.delete_book(token).await.unwrap();
    }
}
