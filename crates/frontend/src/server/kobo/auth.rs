//! `POST /kobo/{sync_token}/v1/auth/device` — Kobo token refresh endpoint.
//!
//! The Kobo firmware calls this URL (returned as `auth_url` in the
//! initialization response) when it needs to refresh its `KoboAccessToken`.
//! We echo the sync token back as the access token with a year-long expiry so
//! the device considers itself authorized without a real token exchange.

use axum::{Json, response::IntoResponse};
use serde::Serialize;

use super::KoboDevice;

#[derive(Serialize)]
struct TokenResponse {
    access_token: String,
    token_type: &'static str,
    /// Expiry in seconds — 1 year keeps the Kobo from re-requesting frequently.
    expires_in: u32,
}

pub async fn handle(kobo: KoboDevice) -> impl IntoResponse {
    Json(TokenResponse {
        access_token: kobo.sync_token,
        token_type: "Bearer",
        expires_in: 31_536_000,
    })
}
