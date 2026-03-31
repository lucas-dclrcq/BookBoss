use dioxus::prelude::*;

use crate::routes::books_page::BookSummary;

/// Global search text signal — persists across route changes regardless of
/// component mounting/unmounting (survives fullstack re-hydration).
pub(crate) static SEARCH_TEXT: GlobalSignal<String> = Signal::global(String::new);

// ---------------------------------------------------------------------------
// Search token types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum SearchField {
    Title,
    Author,
    Series,
    Genre,
    Tag,
    Status,
}

#[derive(Debug, Clone, PartialEq)]
enum SearchToken {
    /// Bare word — matches if it appears in at least one field.
    Any(String),
    /// Field-directed — matches only the specified field.
    Field(SearchField, String),
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parses a search query string into tokens.
///
/// Supports:
/// - Bare words: `thor` → `Any("thor")`
/// - Field-directed: `author:thor` → `Field(Author, "thor")`
/// - Quoted values: `author:"Brad Thor"` → `Field(Author, "brad thor")`
///
/// All values are lowercased for case-insensitive matching.
///
/// If the last word being typed is a prefix of a known field name (e.g.
/// "auth", "autho") it is ignored rather than treated as a bare word. This
/// avoids jarring intermediate filtering while the user is typing a field
/// prefix.
fn parse_search_query(query: &str) -> Vec<SearchToken> {
    let mut tokens = Vec::new();
    let mut chars = query.chars().peekable();

    while chars.peek().is_some() {
        // Skip whitespace.
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }

        if chars.peek().is_none() {
            break;
        }

        // Collect a word (non-whitespace run).
        let mut word = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                break;
            }
            // Stop at colon to check for field prefix.
            if c == ':' && !word.is_empty() {
                if let Some(field) = parse_field_prefix(&word) {
                    chars.next(); // consume ':'
                    let value = collect_value(&mut chars);
                    if !value.is_empty() {
                        tokens.push(SearchToken::Field(field, value.to_lowercase()));
                    }
                    word.clear();
                    break;
                }
            }
            word.push(c);
            chars.next();
        }

        if !word.is_empty() {
            // If the word is at the very end of input (no trailing chars at
            // all) and looks like a field name or prefix, skip it — the user
            // is still typing. A trailing space commits the word.
            let at_end = chars.peek().is_none();
            if at_end && is_field_prefix_start(&word) {
                continue;
            }
            tokens.push(SearchToken::Any(word.to_lowercase()));
        }
    }

    tokens
}

fn parse_field_prefix(prefix: &str) -> Option<SearchField> {
    match prefix.to_lowercase().as_str() {
        "title" => Some(SearchField::Title),
        "author" => Some(SearchField::Author),
        "series" => Some(SearchField::Series),
        "genre" => Some(SearchField::Genre),
        "tag" => Some(SearchField::Tag),
        "status" => Some(SearchField::Status),
        _ => None,
    }
}

const FIELD_NAMES: [&str; 6] = ["title", "author", "series", "genre", "tag", "status"];

pub(crate) const PLACEHOLDER_TIPS: [&str; 6] = [
    "Try: author:\"Brad Thor\"",
    "Try: status:unread",
    "Try: genre:fantasy",
    "Try: series:dune",
    "Try: tag:upnext",
    "Try: author:tolkien fantasy",
];

/// Returns `true` if `word` is a known field name or a prefix of one. Used to
/// suppress intermediate bare-word filtering while the user is typing a field
/// prefix. The word is only committed as a bare search token once the user
/// presses space (making it no longer the last word).
fn is_field_prefix_start(word: &str) -> bool {
    let lower = word.to_lowercase();
    FIELD_NAMES.iter().any(|name| name.starts_with(&lower))
}

/// Returns the ghost-text suffix to display for the last token in `input`.
///
/// The suffix is the remaining characters of the matched field name plus a
/// colon (e.g., `"aut"` + cycle 0 → `"hor:"`). Returns an empty string when:
/// - `input` is empty or ends with whitespace (nothing being typed)
/// - The last token already contains a colon (value entry, not field name)
/// - No field name starts with the last token
///
/// `cycle_idx` selects among multiple matches (alphabetical order) and wraps.
pub(crate) fn compute_completion(input: &str, cycle_idx: usize) -> String {
    if input.is_empty() || input.ends_with(|c: char| c.is_whitespace()) {
        return String::new();
    }

    let last_token = input.split_whitespace().last().unwrap_or("");
    if last_token.contains(':') {
        return String::new();
    }

    let lower = last_token.to_lowercase();
    let field_matches: Vec<&str> = FIELD_NAMES.iter().copied().filter(|name| name.starts_with(lower.as_str())).collect();

    if field_matches.is_empty() {
        return String::new();
    }

    let matched = field_matches[cycle_idx % field_matches.len()];
    format!("{}:", &matched[lower.len()..])
}

/// Applies a completion suffix to the last token of `input`.
///
/// `suffix` is the return value of `compute_completion` (e.g., `"hor:"`).
/// The result is the original input with the suffix appended to the last token,
/// so `apply_completion("aut", "hor:")` → `"author:"`.
pub(crate) fn apply_completion(input: &str, suffix: &str) -> String {
    let last_space = input.rfind(|c: char| c.is_whitespace()).map_or(0, |i| i + 1);
    format!("{}{}{}", &input[..last_space], &input[last_space..], suffix)
}

/// Produces the next cycled input string when Tab is pressed after a
/// completion has already been applied (shell-style cycling).
///
/// `current_input` is the full input after the previous Tab (e.g. `"series:"`).
/// `cycle_prefix` is the original prefix before any Tab (e.g. `"s"`).
/// `cycle_idx` is the (already-incremented) cycle counter.
///
/// Returns `None` when there is only one match for `cycle_prefix` (no cycling
/// possible) or when `cycle_prefix` has no matches.
pub(crate) fn next_cycle_input(current_input: &str, cycle_prefix: &str, cycle_idx: usize) -> Option<String> {
    let lower = cycle_prefix.to_lowercase();
    let field_matches: Vec<&str> = FIELD_NAMES.iter().copied().filter(|name| name.starts_with(lower.as_str())).collect();

    if field_matches.len() < 2 {
        return None;
    }

    let next_field = field_matches[cycle_idx % field_matches.len()];
    let last_space = current_input.rfind(|c: char| c.is_whitespace()).map_or(0, |i| i + 1);
    let head = &current_input[..last_space];
    Some(format!("{head}{next_field}:"))
}

/// Collects the value after a field prefix colon.
/// Handles quoted values (`"Brad Thor"`) and unquoted single words.
fn collect_value(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    // Skip leading whitespace.
    while chars.peek().is_some_and(|c| c.is_whitespace()) {
        chars.next();
    }

    if chars.peek() == Some(&'"') {
        chars.next(); // consume opening quote
        let mut value = String::new();
        for c in chars.by_ref() {
            if c == '"' {
                break;
            }
            value.push(c);
        }
        value
    } else {
        let mut value = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                break;
            }
            value.push(c);
            chars.next();
        }
        value
    }
}

// ---------------------------------------------------------------------------
// Filtering
// ---------------------------------------------------------------------------

/// Filters books by a search query string.
///
/// Returns all books if the query is empty or whitespace-only.
pub(crate) fn filter_books_by_search(books: Vec<BookSummary>, query: &str) -> Vec<BookSummary> {
    let tokens = parse_search_query(query);

    if tokens.is_empty() {
        return books;
    }

    books.into_iter().filter(|book| book_matches(book, &tokens)).collect()
}

fn book_matches(book: &BookSummary, tokens: &[SearchToken]) -> bool {
    let title = book.title.to_lowercase();
    let authors_combined: String = book.authors.iter().map(|a| a.name.to_lowercase()).collect::<Vec<_>>().join(" ");
    let series = book.series_name.as_ref().map(|s| s.to_lowercase());
    let genres_combined: String = book.genres.iter().map(|g| g.to_lowercase()).collect::<Vec<_>>().join(" ");
    let tags_combined: String = book.tags.iter().map(|t| t.to_lowercase()).collect::<Vec<_>>().join(" ");

    tokens.iter().all(|token| match token {
        SearchToken::Any(word) => {
            title.contains(word.as_str())
                || authors_combined.contains(word.as_str())
                || series.as_ref().is_some_and(|s| s.contains(word.as_str()))
                || genres_combined.contains(word.as_str())
                || tags_combined.contains(word.as_str())
        }
        SearchToken::Field(field, value) => match field {
            SearchField::Title => title.contains(value.as_str()),
            SearchField::Author => authors_combined.contains(value.as_str()),
            SearchField::Series => series.as_ref().is_some_and(|s| s.contains(value.as_str())),
            SearchField::Genre => genres_combined.contains(value.as_str()),
            SearchField::Tag => tags_combined.contains(value.as_str()),
            SearchField::Status => {
                let book_status = book.reading_state.as_ref().map_or_else(|| "unread".to_string(), |s| s.status.to_lowercase());
                book_status == value.as_str()
            }
        },
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::books_page::AuthorLink;

    fn make_book(title: &str, authors: &[&str], series: Option<&str>, genres: &[&str], tags: &[&str]) -> BookSummary {
        BookSummary {
            token: "tok".into(),
            title: title.into(),
            cover_path: None,
            authors: authors
                .iter()
                .map(|name| AuthorLink {
                    token: "a".into(),
                    name: name.to_string(),
                })
                .collect(),
            series_token: series.map(|_| "s".into()),
            series_name: series.map(Into::into),
            series_number: None,
            genres: genres.iter().map(std::string::ToString::to_string).collect(),
            tags: tags.iter().map(std::string::ToString::to_string).collect(),
            reading_state: None,
            created_at: String::new(),
        }
    }

    // -- Parser tests --

    #[test]
    fn parse_empty_query() {
        assert!(parse_search_query("").is_empty());
        assert!(parse_search_query("   ").is_empty());
    }

    #[test]
    fn parse_bare_words() {
        let tokens = parse_search_query("brad thor");
        assert_eq!(tokens, vec![SearchToken::Any("brad".into()), SearchToken::Any("thor".into())]);
    }

    #[test]
    fn parse_field_directed() {
        let tokens = parse_search_query("author:thor");
        assert_eq!(tokens, vec![SearchToken::Field(SearchField::Author, "thor".into())]);
    }

    #[test]
    fn parse_field_quoted() {
        let tokens = parse_search_query("author:\"Brad Thor\"");
        assert_eq!(tokens, vec![SearchToken::Field(SearchField::Author, "brad thor".into())]);
    }

    #[test]
    fn parse_mixed() {
        let tokens = parse_search_query("author:thor backlash");
        assert_eq!(
            tokens,
            vec![SearchToken::Field(SearchField::Author, "thor".into()), SearchToken::Any("backlash".into())]
        );
    }

    #[test]
    fn parse_all_field_prefixes() {
        for (prefix, expected) in [
            ("title", SearchField::Title),
            ("author", SearchField::Author),
            ("series", SearchField::Series),
            ("genre", SearchField::Genre),
            ("tag", SearchField::Tag),
            ("status", SearchField::Status),
        ] {
            let tokens = parse_search_query(&format!("{prefix}:test"));
            assert_eq!(tokens, vec![SearchToken::Field(expected, "test".into())]);
        }
    }

    #[test]
    fn parse_unknown_prefix_treated_as_bare_word() {
        let tokens = parse_search_query("foo:bar");
        assert_eq!(tokens, vec![SearchToken::Any("foo:bar".into())]);
    }

    #[test]
    fn parse_case_insensitive() {
        let tokens = parse_search_query("Author:Thor");
        assert_eq!(tokens, vec![SearchToken::Field(SearchField::Author, "thor".into())]);
    }

    #[test]
    fn parse_field_name_or_prefix_ignored_when_last() {
        // Partial prefixes of field names are skipped when last
        assert!(parse_search_query("auth").is_empty());
        assert!(parse_search_query("autho").is_empty());
        assert!(parse_search_query("se").is_empty()); // prefix of "series"
        assert!(parse_search_query("ta").is_empty()); // prefix of "tag"

        // Complete field names are also skipped when last (user is about to type ":")
        assert!(parse_search_query("author").is_empty());
        assert!(parse_search_query("tag").is_empty());
        assert!(parse_search_query("title").is_empty());
    }

    #[test]
    fn parse_field_name_committed_when_followed_by_space() {
        // A trailing space commits the word as a bare token
        let tokens = parse_search_query("author ");
        assert_eq!(tokens, vec![SearchToken::Any("author".into())]);
    }

    #[test]
    fn parse_field_prefix_only_affects_last_word() {
        // Field-name-like words mid-query (followed by more words) are committed
        let tokens = parse_search_query("auth dune");
        assert_eq!(tokens, vec![SearchToken::Any("auth".into()), SearchToken::Any("dune".into())]);
        let tokens = parse_search_query("author dune");
        assert_eq!(tokens, vec![SearchToken::Any("author".into()), SearchToken::Any("dune".into())]);
    }

    // -- Filter tests --

    #[test]
    fn empty_query_returns_all() {
        let books = vec![make_book("Dune", &["Frank Herbert"], None, &[], &[])];
        let result = filter_books_by_search(books.clone(), "");
        assert_eq!(result.len(), 1);

        let result = filter_books_by_search(books, "   ");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn bare_word_matches_title() {
        let books = vec![
            make_book("Dune", &["Frank Herbert"], None, &[], &[]),
            make_book("Neuromancer", &["William Gibson"], None, &[], &[]),
        ];
        let result = filter_books_by_search(books, "dune");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Dune");
    }

    #[test]
    fn bare_word_matches_author() {
        let books = vec![
            make_book("Dune", &["Frank Herbert"], None, &[], &[]),
            make_book("Neuromancer", &["William Gibson"], None, &[], &[]),
        ];
        let result = filter_books_by_search(books, "gibson");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Neuromancer");
    }

    #[test]
    fn bare_word_matches_series() {
        let books = vec![
            make_book("Dune", &["Frank Herbert"], Some("Dune Chronicles"), &[], &[]),
            make_book("Neuromancer", &["William Gibson"], None, &[], &[]),
        ];
        let result = filter_books_by_search(books, "chronicles");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Dune");
    }

    #[test]
    fn bare_word_matches_genre() {
        let books = vec![
            make_book("Dune", &["Frank Herbert"], None, &["Science Fiction"], &[]),
            make_book("The Hobbit", &["J.R.R. Tolkien"], None, &["Fantasy"], &[]),
        ];
        let result = filter_books_by_search(books, "fantasy");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "The Hobbit");
    }

    #[test]
    fn bare_word_matches_tag() {
        let books = vec![
            make_book("Dune", &["Frank Herbert"], None, &[], &["UpNext"]),
            make_book("Neuromancer", &["William Gibson"], None, &[], &[]),
        ];
        let result = filter_books_by_search(books, "upnext");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Dune");
    }

    #[test]
    fn multiple_bare_words_require_all() {
        let books = vec![
            make_book("Backlash", &["Brad Thor"], None, &[], &[]),
            make_book("The Last Patriot", &["Brad Thor"], None, &[], &[]),
            make_book("Neuromancer", &["William Gibson"], None, &[], &[]),
        ];
        let result = filter_books_by_search(books, "brad backlash");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Backlash");
    }

    #[test]
    fn field_directed_author() {
        let books = vec![
            make_book("Thor", &["Someone Else"], None, &[], &[]),
            make_book("Backlash", &["Brad Thor"], None, &[], &[]),
        ];
        let result = filter_books_by_search(books, "author:thor");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Backlash");
    }

    #[test]
    fn field_directed_tag() {
        let books = vec![
            make_book("Dune", &["Frank Herbert"], None, &[], &["UpNext"]),
            make_book("Neuromancer", &["William Gibson"], None, &[], &["Finished"]),
        ];
        let result = filter_books_by_search(books, "tag:upnext");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Dune");
    }

    #[test]
    fn quoted_field_value() {
        let books = vec![
            make_book("Backlash", &["Brad Thor"], None, &[], &[]),
            make_book("Dune", &["Frank Herbert"], None, &[], &[]),
        ];
        let result = filter_books_by_search(books, "author:\"Brad Thor\"");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Backlash");
    }

    #[test]
    fn mixed_field_and_bare() {
        let books = vec![
            make_book("Backlash", &["Brad Thor"], None, &[], &[]),
            make_book("The Last Patriot", &["Brad Thor"], None, &[], &[]),
        ];
        // author must contain "thor" AND at least one field must contain "backlash"
        let result = filter_books_by_search(books, "author:thor backlash");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Backlash");
    }

    #[test]
    fn case_insensitive_matching() {
        let books = vec![make_book("Dune", &["Frank Herbert"], None, &[], &[])];
        let result = filter_books_by_search(books, "DUNE");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn no_match_returns_empty() {
        let books = vec![make_book("Dune", &["Frank Herbert"], None, &[], &[])];
        let result = filter_books_by_search(books, "xyz");
        assert!(result.is_empty());
    }

    fn make_book_with_status(title: &str, status: Option<&str>) -> BookSummary {
        use crate::routes::book_detail_page::ReadingStateDto;
        let mut book = make_book(title, &[], None, &[], &[]);
        book.reading_state = status.map(|s| ReadingStateDto {
            status: s.to_string(),
            progress_pct: None,
            personal_rating: None,
            times_read: 0,
            notes: None,
        });
        book
    }

    #[test]
    fn status_unread_matches_no_reading_state() {
        let books = vec![make_book_with_status("Dune", None), make_book_with_status("Neuromancer", Some("Reading"))];
        let result = filter_books_by_search(books, "status:unread");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Dune");
    }

    #[test]
    fn status_reading_matches_reading_state() {
        let books = vec![make_book_with_status("Dune", None), make_book_with_status("Neuromancer", Some("Reading"))];
        let result = filter_books_by_search(books, "status:reading");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Neuromancer");
    }

    #[test]
    fn status_all_variants_match() {
        for status in ["Reading", "Paused", "Rereading", "Read", "Abandoned"] {
            let books = vec![
                make_book_with_status("Book A", Some(status)),
                make_book_with_status("Book B", None), // Unread — won't match any named status
            ];
            let result = filter_books_by_search(books, &format!("status:{}", status.to_lowercase()));
            assert_eq!(result.len(), 1, "status:{status} should match exactly one book");
            assert_eq!(result[0].title, "Book A");
        }
    }

    #[test]
    fn status_case_insensitive() {
        let books = vec![make_book_with_status("Dune", Some("Reading"))];
        let result = filter_books_by_search(books, "status:READING");
        assert_eq!(result.len(), 1);
    }

    // ── Tab-completion helpers ──

    #[test]
    fn completion_empty_input() {
        assert_eq!(compute_completion("", 0), "");
    }

    #[test]
    fn completion_no_match() {
        assert_eq!(compute_completion("xyz", 0), "");
        assert_eq!(compute_completion("foo:bar", 0), ""); // last token has colon
    }

    #[test]
    fn completion_partial_prefix() {
        assert_eq!(compute_completion("aut", 0), "hor:");
        assert_eq!(compute_completion("autho", 0), "r:");
        assert_eq!(compute_completion("ge", 0), "nre:");
        assert_eq!(compute_completion("ta", 0), "g:");
        assert_eq!(compute_completion("ti", 0), "tle:");
    }

    #[test]
    fn completion_exact_field_name() {
        assert_eq!(compute_completion("author", 0), ":");
        assert_eq!(compute_completion("tag", 0), ":");
    }

    #[test]
    fn completion_after_colon_suppressed() {
        assert_eq!(compute_completion("author:", 0), "");
        assert_eq!(compute_completion("author:brad", 0), "");
    }

    #[test]
    fn completion_after_space_uses_last_word() {
        assert_eq!(compute_completion("author:thor s", 0), "eries:");
        assert_eq!(compute_completion("author:thor s", 1), "tatus:");
    }

    #[test]
    fn completion_trailing_space_no_match() {
        assert_eq!(compute_completion("author ", 0), "");
    }

    #[test]
    fn completion_ambiguous_prefix_cycles() {
        assert_eq!(compute_completion("s", 0), "eries:");
        assert_eq!(compute_completion("s", 1), "tatus:");
        assert_eq!(compute_completion("s", 2), "eries:"); // wraps
    }

    #[test]
    fn apply_completion_simple() {
        assert_eq!(apply_completion("aut", "hor:"), "author:");
        assert_eq!(apply_completion("ge", "nre:"), "genre:");
        assert_eq!(apply_completion("author", ":"), "author:");
    }

    #[test]
    fn apply_completion_preserves_prefix() {
        assert_eq!(apply_completion("author:thor s", "eries:"), "author:thor series:");
    }

    #[test]
    fn next_cycle_input_replaces_completed_field() {
        assert_eq!(next_cycle_input("series:", "s", 1), Some("status:".to_string()));
        assert_eq!(next_cycle_input("status:", "s", 2), Some("series:".to_string()));
    }

    #[test]
    fn next_cycle_input_single_match_returns_none() {
        assert_eq!(next_cycle_input("genre:", "ge", 1), None);
    }

    #[test]
    fn apply_completion_empty_suffix() {
        // Empty suffix returns input unchanged
        assert_eq!(apply_completion("author:thor", ""), "author:thor");
        assert_eq!(apply_completion("", ""), "");
    }

    #[test]
    fn next_cycle_input_multi_token_input() {
        // Cycling works correctly when there are tokens before the completed field
        assert_eq!(next_cycle_input("author:thor series:", "s", 1), Some("author:thor status:".to_string()));
        assert_eq!(next_cycle_input("author:thor status:", "s", 2), Some("author:thor series:".to_string()));
    }
}
