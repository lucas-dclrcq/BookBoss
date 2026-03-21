/// Weight applied to the title similarity component of the combined score.
pub const TITLE_WEIGHT: f32 = 0.7;

/// Weight applied to the author similarity component of the combined score.
pub const AUTHOR_WEIGHT: f32 = 0.3;

/// Combine a title and author similarity score into a single value using
/// [`TITLE_WEIGHT`] and [`AUTHOR_WEIGHT`].
///
/// If no author is available for comparison, pass `0.5` as `author_score`
/// to apply a neutral contribution.
#[must_use]
pub fn combined_score(title_score: f32, author_score: f32) -> f32 {
    TITLE_WEIGHT * title_score + AUTHOR_WEIGHT * author_score
}

/// Compute word-level Jaccard similarity between two title strings.
///
/// Normalizes each title by stripping subtitles (text after ` : ` or ` - `),
/// removing punctuation, lowercasing, and dropping leading articles
/// ("the", "a", "an") before comparing word sets.
///
/// Returns a value in `[0.0, 1.0]`; `1.0` means identical word sets.
#[must_use]
pub fn title_similarity(a: &str, b: &str) -> f32 {
    word_jaccard(&normalize_title(a), &normalize_title(b))
}

/// Compute word-level Jaccard similarity between two author name strings.
///
/// Normalizes each name by removing punctuation and lowercasing before
/// comparing word sets. Order-independent, so "Smith, Jane" and "Jane Smith"
/// produce the same score.
///
/// Returns a value in `[0.0, 1.0]`; `1.0` means identical word sets.
#[must_use]
pub fn author_similarity(a: &str, b: &str) -> f32 {
    word_jaccard(&normalize_author(a), &normalize_author(b))
}

fn normalize_title(s: &str) -> String {
    let main = s.split(" : ").next().and_then(|t| t.split(" - ").next()).unwrap_or(s);
    let cleaned: String = main.to_lowercase().chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect();
    cleaned
        .split_whitespace()
        .filter(|w| !matches!(*w, "the" | "a" | "an"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_author(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn word_jaccard(a: &str, b: &str) -> f32 {
    let a_words: std::collections::HashSet<&str> = a.split_whitespace().collect();
    let b_words: std::collections::HashSet<&str> = b.split_whitespace().collect();
    let union_count = a_words.union(&b_words).count();
    if union_count == 0 {
        return 1.0;
    }
    #[expect(clippy::cast_precision_loss, reason = "word counts are small; f32 precision is sufficient")]
    {
        a_words.intersection(&b_words).count() as f32 / union_count as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_titles_score_one() {
        assert!((title_similarity("The Hobbit", "The Hobbit") - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn article_stripping() {
        assert!((title_similarity("The Hobbit", "Hobbit") - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn subtitle_stripping() {
        let score = title_similarity(
            "Fellowship of the Ring",
            "The Fellowship of the Ring : Being the First Part of The Lord of the Rings",
        );
        assert!(score > 0.9, "score was {score}");
    }

    #[test]
    fn unrelated_titles_score_low() {
        let score = title_similarity("The Hobbit", "Pride and Prejudice");
        assert!(score < 0.2, "score was {score}");
    }

    #[test]
    fn author_order_independent() {
        assert!((author_similarity("Smith, Jane", "Jane Smith") - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn combined_score_weights() {
        let s = combined_score(1.0, 0.0);
        assert!((s - TITLE_WEIGHT).abs() < f32::EPSILON);
    }
}
