/// ISO 639-1 language codes — North American / European subset.
pub const LANGUAGE_CODES: &[&str] = &[
    "af", "be", "bg", "ca", "cs", "cy", "da", "de", "el", "en", "es", "et", "fi", "fr", "ga", "hr", "hu", "is", "it", "lv", "lt", "mk", "nl", "no", "pl", "pt",
    "ro", "ru", "sk", "sl", "sq", "sr", "sv", "tr", "uk",
];

/// Normalises a BCP 47 language tag to its root subtag and validates it
/// against the known language list.
///
/// Strips any region subtag (`"en-US"` → `"en"`, `"en_US"` → `"en"`),
/// lowercases the result, then returns `None` if the code is not in
/// [`LANGUAGE_CODES`] (e.g. `"und"`, unknown codes, empty strings).
pub fn normalize_language(lang: &str) -> Option<String> {
    let code = lang.split(['-', '_']).next().unwrap_or(lang).to_lowercase();
    if LANGUAGE_CODES.contains(&code.as_str()) { Some(code) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_region_subtag() {
        assert_eq!(normalize_language("en-US"), Some("en".to_string()));
        assert_eq!(normalize_language("en_US"), Some("en".to_string()));
        assert_eq!(normalize_language("pt-BR"), Some("pt".to_string()));
        assert_eq!(normalize_language("en"), Some("en".to_string()));
        assert_eq!(normalize_language("EN"), Some("en".to_string()));
    }

    #[test]
    fn rejects_unknown_codes() {
        assert_eq!(normalize_language("und"), None);
        assert_eq!(normalize_language("xx"), None);
        assert_eq!(normalize_language(""), None);
        assert_eq!(normalize_language("zz-ZZ"), None);
    }
}
