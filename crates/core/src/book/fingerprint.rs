use rust_decimal::Decimal;
use serde::Serialize;
use sha2::{Digest, Sha256};

/// Canonical input to the sidecar fingerprint hash.
///
/// Fields are declared in a fixed order — Serde always emits struct fields in
/// declaration order, ensuring the JSON bytes are identical across runs for the
/// same logical inputs.
///
/// All `Vec` fields must be **pre-sorted ascending** by the caller before this
/// struct is constructed.
#[derive(Serialize)]
struct FingerprintData<'a> {
    title: &'a str,
    authors: &'a [&'a str],
    series_name: Option<&'a str>,
    series_number: Option<String>,
    publisher: Option<&'a str>,
    genres: &'a [&'a str],
    tags: &'a [&'a str],
    rating: Option<i16>,
}

/// Compute a stable SHA-256 fingerprint of sidecar-visible book metadata.
///
/// All slice inputs (`author_names`, `genres`, `tags`) **must be sorted
/// ascending** by the caller — the function does not sort them internally so
/// that callers can sort once and reuse the slices.
///
/// Returns a 64-character lowercase hex string.
pub fn compute_sidecar_fingerprint(
    title: &str,
    author_names: &[&str],
    series_name: Option<&str>,
    series_number: Option<&Decimal>,
    publisher: Option<&str>,
    genres: &[&str],
    tags: &[&str],
    rating: Option<i16>,
) -> String {
    let data = FingerprintData {
        title,
        authors: author_names,
        series_name,
        series_number: series_number.map(std::string::ToString::to_string),
        publisher,
        genres,
        tags,
        rating,
    };
    let json = serde_json::to_vec(&data).expect("FingerprintData is always serializable");
    let hash = Sha256::digest(&json);
    hash.iter().fold(String::with_capacity(64), |mut s, b| {
        use std::fmt::Write;
        write!(s, "{b:02x}").unwrap();
        s
    })
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;

    fn base_fp() -> String {
        compute_sidecar_fingerprint(
            "Dune",
            &["Frank Herbert"],
            Some("Dune"),
            Some(&dec!(1.0)),
            Some("Chilton"),
            &["Science Fiction"],
            &["Epic"],
            Some(9),
        )
    }

    #[test]
    fn deterministic() {
        assert_eq!(base_fp(), base_fp());
    }

    #[test]
    fn changing_title_changes_hash() {
        let a = compute_sidecar_fingerprint("Dune", &["Frank Herbert"], None, None, None, &[], &[], None);
        let b = compute_sidecar_fingerprint("Dune Messiah", &["Frank Herbert"], None, None, None, &[], &[], None);
        assert_ne!(a, b);
    }

    #[test]
    fn changing_author_changes_hash() {
        let a = compute_sidecar_fingerprint("Dune", &["Frank Herbert"], None, None, None, &[], &[], None);
        let b = compute_sidecar_fingerprint("Dune", &["Brian Herbert"], None, None, None, &[], &[], None);
        assert_ne!(a, b);
    }

    #[test]
    fn pre_sorted_authors_are_stable() {
        let mut names = ["Zia", "Ada", "Bob"];
        names.sort_unstable();
        let a = compute_sidecar_fingerprint("Book", names.as_ref(), None, None, None, &[], &[], None);
        // Same sorted order again
        let b = compute_sidecar_fingerprint("Book", &["Ada", "Bob", "Zia"], None, None, None, &[], &[], None);
        assert_eq!(a, b);
    }

    #[test]
    fn pre_sorted_genres_are_stable() {
        let a = compute_sidecar_fingerprint("Book", &[], None, None, None, &["Fantasy", "Science Fiction"], &[], None);
        let b = compute_sidecar_fingerprint("Book", &[], None, None, None, &["Fantasy", "Science Fiction"], &[], None);
        assert_eq!(a, b);
    }

    #[test]
    fn changing_genre_changes_hash() {
        let a = compute_sidecar_fingerprint("Book", &[], None, None, None, &["Fantasy"], &[], None);
        let b = compute_sidecar_fingerprint("Book", &[], None, None, None, &["Horror"], &[], None);
        assert_ne!(a, b);
    }

    #[test]
    fn none_publisher_differs_from_empty_string() {
        let a = compute_sidecar_fingerprint("Book", &[], None, None, None, &[], &[], None);
        let b = compute_sidecar_fingerprint("Book", &[], None, None, Some(""), &[], &[], None);
        assert_ne!(a, b);
    }

    #[test]
    fn changing_rating_changes_hash() {
        let a = compute_sidecar_fingerprint("Book", &[], None, None, None, &[], &[], Some(8));
        let b = compute_sidecar_fingerprint("Book", &[], None, None, None, &[], &[], Some(9));
        assert_ne!(a, b);
    }

    #[test]
    fn none_rating_differs_from_some() {
        let a = compute_sidecar_fingerprint("Book", &[], None, None, None, &[], &[], None);
        let b = compute_sidecar_fingerprint("Book", &[], None, None, None, &[], &[], Some(0));
        assert_ne!(a, b);
    }

    #[test]
    fn series_number_included_in_hash() {
        let a = compute_sidecar_fingerprint("Book", &[], Some("Series"), Some(&dec!(1.0)), None, &[], &[], None);
        let b = compute_sidecar_fingerprint("Book", &[], Some("Series"), Some(&dec!(2.0)), None, &[], &[], None);
        assert_ne!(a, b);
    }

    #[test]
    fn output_is_64_hex_chars() {
        let fp = base_fp();
        assert_eq!(fp.len(), 64);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
