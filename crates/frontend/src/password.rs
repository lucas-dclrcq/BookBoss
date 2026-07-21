//! Password validation helpers shared across registration and login flows.

pub(crate) const SPECIAL_CHARS: &str = "!@#$%^&*()_+-=[]{}|;:,.<>?";
pub(crate) const MIN_PASSWORD_LEN: usize = 12;

pub(crate) fn password_requirements(pw: &str) -> Vec<(String, bool)> {
    vec![
        (format!("At least {MIN_PASSWORD_LEN} characters"), pw.len() >= MIN_PASSWORD_LEN),
        ("One uppercase letter (A–Z)".to_string(), pw.chars().any(char::is_uppercase)),
        ("One lowercase letter (a–z)".to_string(), pw.chars().any(char::is_lowercase)),
        ("One digit (0–9)".to_string(), pw.chars().any(|c| c.is_ascii_digit())),
        ("One special character (!@#$%^&*…)".to_string(), pw.chars().any(|c| SPECIAL_CHARS.contains(c))),
    ]
}

pub(crate) fn password_is_valid(pw: &str) -> bool {
    password_requirements(pw).iter().all(|(_, ok)| *ok)
}

/// Generates a random password that satisfies [`password_is_valid`]: one of
/// each required character class plus filler, shuffled. Used both for
/// admin-created accounts and for auto-provisioned SSO accounts (which never
/// use it, but the `users` table requires a hash).
#[cfg(feature = "server")]
pub(crate) fn make_password() -> String {
    use rand::RngExt;

    const UPPER: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    const LOWER: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
    const DIGITS: &[u8] = b"0123456789";
    const ALL: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*()_+-=[]{}|;:,.<>?";

    let mut rng = rand::rng();
    // Guarantee one of each required character class.
    let mut pw: Vec<u8> = vec![
        UPPER[rng.random_range(0..UPPER.len())],
        LOWER[rng.random_range(0..LOWER.len())],
        DIGITS[rng.random_range(0..DIGITS.len())],
        SPECIAL_CHARS.as_bytes()[rng.random_range(0..SPECIAL_CHARS.len())],
    ];
    for _ in pw.len()..16 {
        pw.push(ALL[rng.random_range(0..ALL.len())]);
    }
    // Fisher-Yates shuffle so the guaranteed classes aren't always in front.
    for i in (1..pw.len()).rev() {
        let j = rng.random_range(0..=i);
        pw.swap(i, j);
    }
    String::from_utf8(pw).expect("all bytes are valid ASCII")
}

/// Server-side password strength validation. Returns `Err` with a user-facing
/// message if the password does not satisfy all requirements.
#[cfg(feature = "server")]
pub(crate) fn validate_password_strength(password: &str) -> Result<(), dioxus::prelude::ServerFnError> {
    if password.len() < MIN_PASSWORD_LEN {
        return Err(dioxus::prelude::ServerFnError::new(format!(
            "Password must be at least {MIN_PASSWORD_LEN} characters"
        )));
    }
    if !password.chars().any(char::is_uppercase) {
        return Err(dioxus::prelude::ServerFnError::new("Password must contain at least one uppercase letter"));
    }
    if !password.chars().any(char::is_lowercase) {
        return Err(dioxus::prelude::ServerFnError::new("Password must contain at least one lowercase letter"));
    }
    if !password.chars().any(|c| c.is_ascii_digit()) {
        return Err(dioxus::prelude::ServerFnError::new("Password must contain at least one digit"));
    }
    if !password.chars().any(|c| SPECIAL_CHARS.contains(c)) {
        return Err(dioxus::prelude::ServerFnError::new("Password must contain at least one special character"));
    }
    Ok(())
}
