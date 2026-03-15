//! `POST /kobo/{sync_token}/v1/auth/device` — Kobo token acquisition/refresh.
//!
//! The Kobo firmware calls this URL (returned as `device_auth` and
//! `device_refresh` in the initialization Resources map) when it needs to
//! acquire or refresh its `KoboAccessToken`.
//!
//! # Protocol notes
//!
//! Calibre-Web returns `AccessToken`, `RefreshToken`, `TokenType`,
//! `TrackingId`, and `UserKey` (all PascalCase). The Kobo firmware parses
//! the `AccessToken` and stores it as `KoboAccessToken`. An `ExpiresIn`
//! field (seconds) tells the Kobo when to next refresh; without it the device
//! may set a very short default expiry, causing repeated auth failures.

use axum::{Json, response::IntoResponse};
use serde::{Deserialize, Serialize};

use super::KoboDevice;

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct AuthRequest {
    /// The Kobo echoes back the UserKey it received during initialization.
    user_key: String,
}

/// Response shape matches Calibre-Web's `make_calibre_web_auth_response()`.
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct AuthResponse {
    /// The new access token the Kobo stores as `KoboAccessToken`.
    access_token: String,
    /// Refresh token (not used by BookBoss, but the Kobo expects the field).
    refresh_token: String,
    token_type: &'static str,
    /// Request tracking identifier (static — the Kobo only logs this).
    tracking_id: &'static str,
    /// Echoed back from the request (or falls back to the URL sync token).
    user_key: String,
    /// Token lifetime in seconds. 1 year prevents constant re-auth cycles.
    expires_in: u32,
}

pub async fn handle(kobo: KoboDevice, body: axum::body::Bytes) -> impl IntoResponse {
    let req: AuthRequest = serde_json::from_slice(&body).unwrap_or_default();
    let user_key = if req.user_key.is_empty() { kobo.sync_token.clone() } else { req.user_key };

    Json(AuthResponse {
        access_token: kobo.sync_token.clone(),
        refresh_token: kobo.sync_token,
        token_type: "Bearer",
        tracking_id: "00000000-0000-0000-0000-000000000000",
        user_key,
        expires_in: 31_536_000,
    })
}
