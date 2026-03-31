//! Shared helpers for server functions.

use dioxus::prelude::ServerFnError;

use crate::server::{AuthSession, AuthUser};

/// Extracts the authenticated `AuthUser` from the session.
///
/// Returns `Err("Not authenticated")` when the session carries no user or the
/// user is the anonymous default (empty username).
pub(crate) fn authenticated_user(auth_session: &AuthSession) -> Result<AuthUser, ServerFnError> {
    auth_session
        .current_user
        .as_ref()
        .filter(|u| !u.username.is_empty())
        .cloned()
        .ok_or_else(|| ServerFnError::new("Not authenticated"))
}

/// Converts any `Display` error into a `ServerFnError`.
///
/// Replaces the `.map_err(|e| ServerFnError::new(e.to_string()))` boilerplate
/// with a point-free `.map_err(to_server_err)`.
pub(crate) fn to_server_err<E: std::fmt::Display>(e: E) -> ServerFnError {
    ServerFnError::new(e.to_string())
}
