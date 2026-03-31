/// Normalize a name string: trim edges and collapse interior whitespace runs
/// to a single space. Ensures `"A  B"` and `"A B"` resolve identically.
pub fn normalize_name(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::normalize_name;

    #[test]
    fn normalize_name_already_normal() {
        assert_eq!(normalize_name("J.K. Rowling"), "J.K. Rowling");
    }

    #[test]
    fn normalize_name_collapses_interior_spaces() {
        assert_eq!(normalize_name("A  B"), "A B");
    }

    #[test]
    fn normalize_name_trims_leading_and_trailing() {
        assert_eq!(normalize_name("  Foo  "), "Foo");
    }

    #[test]
    fn normalize_name_empty_string() {
        assert_eq!(normalize_name(""), "");
    }
}
