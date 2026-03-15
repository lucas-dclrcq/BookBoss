//! Kobo sync cursor encoding/decoding.
//!
//! The cursor is sent as the `x-kobo-sync-token` response header and echoed
//! back by the Kobo on subsequent requests. It encodes both the incremental
//! sync timestamp and the keyset pagination bookmark so that a multi-page sync
//! can be resumed after a disconnection.
//!
//! # Format
//!
//! `BB1:{timestamp_ms}:{last_book_id}`
//!
//! | Field           | Value        | Meaning                         |
//! |-----------------|------------- |---------------------------------|
//! | prefix          | `BB1`        | version sentinel                |
//! | `timestamp_ms`  | `0`          | full sync (no `since` filter)   |
//! | `timestamp_ms`  | unix ms      | incremental — `since` timestamp |
//! | `last_book_id`  | `0`          | first page (no keyset cursor)   |
//! | `last_book_id`  | non-zero u64 | keyset cursor for next page     |

use bb_core::book::BookId;
use chrono::{DateTime, Utc};

const PREFIX: &str = "BB1";

/// Encodes a sync cursor from its constituent parts.
///
/// A `timestamp_ms` of `0` (produced when `since` is `None`) signals a full
/// initial sync. A `last_book_id` of `0` (produced when `after_book_id` is
/// `None`) indicates the first page.
pub fn encode(since: Option<DateTime<Utc>>, after_book_id: Option<BookId>) -> String {
    let ts = since.map(|t| t.timestamp_millis()).unwrap_or(0);
    let id = after_book_id.unwrap_or(0);
    format!("{PREFIX}:{ts}:{id}")
}

/// Decodes a sync cursor string.
///
/// Returns `(since, after_book_id)`. On any parse failure or unknown prefix
/// the function returns `(None, None)`, which is equivalent to a fresh full
/// sync from the first page — a safe fallback.
pub fn decode(s: &str) -> (Option<DateTime<Utc>>, Option<BookId>) {
    let parts: Vec<&str> = s.splitn(3, ':').collect();
    if parts.len() != 3 || parts[0] != PREFIX {
        return (None, None);
    }

    let ts: i64 = match parts[1].parse() {
        Ok(v) => v,
        Err(_) => return (None, None),
    };
    let id: u64 = match parts[2].parse() {
        Ok(v) => v,
        Err(_) => return (None, None),
    };

    let since = if ts == 0 { None } else { DateTime::from_timestamp_millis(ts) };

    let after_book_id = if id == 0 { None } else { Some(id) };

    (since, after_book_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_initial_sync() {
        assert_eq!(encode(None, None), "BB1:0:0");
    }

    #[test]
    fn encode_incremental_first_page() {
        let ts = DateTime::from_timestamp(1_000, 0).unwrap();
        assert_eq!(encode(Some(ts), None), "BB1:1000000:0");
    }

    #[test]
    fn encode_keyset_page() {
        let ts = DateTime::from_timestamp(2_000, 0).unwrap();
        assert_eq!(encode(Some(ts), Some(42)), "BB1:2000000:42");
    }

    #[test]
    fn round_trip_initial() {
        let (since, after) = decode(&encode(None, None));
        assert!(since.is_none());
        assert!(after.is_none());
    }

    #[test]
    fn round_trip_incremental_first_page() {
        let ts = DateTime::from_timestamp_millis(1_234_567_890).unwrap();
        let (since, after) = decode(&encode(Some(ts), None));
        assert_eq!(since, Some(ts));
        assert!(after.is_none());
    }

    #[test]
    fn round_trip_keyset_page() {
        let ts = DateTime::from_timestamp_millis(9_999_999).unwrap();
        let (since, after) = decode(&encode(Some(ts), Some(99)));
        assert_eq!(since, Some(ts));
        assert_eq!(after, Some(99));
    }

    #[test]
    fn decode_unknown_prefix_returns_full_sync_defaults() {
        let (since, after) = decode("V2:1000:5");
        assert!(since.is_none());
        assert!(after.is_none());
    }

    #[test]
    fn decode_garbage_returns_full_sync_defaults() {
        let (since, after) = decode("not-a-cursor");
        assert!(since.is_none());
        assert!(after.is_none());
    }

    #[test]
    fn decode_bad_timestamp_returns_full_sync_defaults() {
        let (since, after) = decode("BB1:notanumber:0");
        assert!(since.is_none());
        assert!(after.is_none());
    }

    #[test]
    fn decode_bad_book_id_returns_full_sync_defaults() {
        let (since, after) = decode("BB1:1000:notanumber");
        assert!(since.is_none());
        assert!(after.is_none());
    }
}
