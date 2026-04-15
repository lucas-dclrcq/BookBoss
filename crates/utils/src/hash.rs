use std::path::Path;

use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error)]
pub enum HashError {
    #[error("I/O error hashing file: {0}")]
    Io(#[from] std::io::Error),
    #[error("Task join error: {0}")]
    Join(#[from] tokio::task::JoinError),
}

/// Computes a lowercase SHA-256 hex digest of the file at `path`.
///
/// Runs on a blocking thread via `spawn_blocking` so the async runtime
/// is not stalled while reading large files.
pub async fn hash_file(path: &Path) -> Result<String, HashError> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut file = std::fs::File::open(&path)?;
        let mut hasher = Sha256::new();
        let mut buf = [0u8; 8192];
        loop {
            let n = std::io::Read::read(&mut file, &mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        let hash = hasher.finalize();
        Ok(hash.iter().fold(String::with_capacity(64), |mut s, b| {
            use std::fmt::Write;
            write!(s, "{b:02x}").unwrap();
            s
        }))
    })
    .await?
}

/// Computes a lowercase SHA-256 hex digest of the given bytes.
pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let hash = hasher.finalize();
    hash.iter().fold(String::with_capacity(64), |mut s, b| {
        use std::fmt::Write;
        write!(s, "{b:02x}").unwrap();
        s
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::{hash_bytes, hash_file};

    #[tokio::test]
    async fn known_digest() {
        // echo -n "hello" | sha256sum → 2cf24dba...
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(b"hello").unwrap();
        let digest = hash_file(f.path()).await.unwrap();
        assert_eq!(digest, "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
    }

    #[tokio::test]
    async fn deterministic() {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(b"bookboss test data").unwrap();
        let h1 = hash_file(f.path()).await.unwrap();
        let h2 = hash_file(f.path()).await.unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_bytes_matches_known_digest() {
        // echo -n "hello" | sha256sum
        let digest = hash_bytes(b"hello");
        assert_eq!(digest, "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
    }
}
