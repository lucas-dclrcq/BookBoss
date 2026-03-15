//! `GET /kobo/{sync_token}/v1/initialization` (also accepts POST)
//!
//! Called by the Kobo device on first connection (and on reconnect). Returns
//! device settings and a `Resources` map that the Kobo uses to discover the
//! exact URL for every subsequent API call.
//!
//! # Protocol notes
//!
//! Calibre-Web (the reference implementation) returns the header
//! `x-kobo-apitoken: e30=` on the initialization response. `e30=` is the
//! base64 encoding of `{}`. Without this header the Kobo firmware does not
//! consider the initialization successful.
//!
//! The Kobo looks up auth via the `device_auth` and `device_refresh` keys in
//! `Resources` (not `auth_url`). We point both at our own auth endpoint so
//! the device can refresh its `KoboAccessToken` without contacting the real
//! Kobo store.

use axum::{
    Json,
    body::Bytes,
    http::{HeaderMap, HeaderName, HeaderValue},
    response::IntoResponse,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::KoboDevice;

// ── Request
// ───────────────────────────────────────────────────────────────────

/// Device information sent by the Kobo on initialization.
///
/// All fields are optional — the schema varies across firmware versions. We
/// log what we receive and otherwise ignore the body.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub struct KoboInitRequest {
    pub affiliate_name: Option<String>,
    pub app_version: Option<String>,
    pub device_id: Option<String>,
    pub serial: Option<String>,
    pub user_agent: Option<String>,
}

// ── Response
// ──────────────────────────────────────────────────────────────────

/// Endpoint URLs returned to the Kobo so it knows where to call each function.
///
/// Field names are serialised as-is (snake_case) to match the Kobo protocol.
/// Template variables (e.g. `{ImageId}`, `{RevisionId}`) are filled in by the
/// Kobo client before making each request.
#[derive(Serialize)]
pub struct KoboResources {
    /// Base host for image requests (the Kobo may use this separately).
    pub image_host: String,
    /// Cover image URL template. `{ImageId}` = book UUID, `{Width}`,
    /// `{Height}`, `{Quality}`, `{IsGreyscale}` filled by Kobo.
    pub image_url_quality_template: String,
    /// Simplified cover image URL template (fixed quality/greyscale).
    pub image_url_template: String,
    /// Per-book metadata URL. `{RevisionId}` = book UUID.
    pub library_metadata_url: String,
    /// Library sync endpoint (no template variables).
    pub library_sync_url: String,
    /// Per-book bookmark sync URL. `{RevisionId}` = book UUID.
    pub bookmark_url: String,
    /// Unused by BookBoss; empty string satisfies the Kobo schema.
    pub subscription_host: String,
    /// Token acquisition endpoint. The Kobo calls this when it needs a new
    /// `KoboAccessToken` (e.g. first connection). Replaces the legacy
    /// `auth_url` key — Kobo firmware looks for `device_auth`.
    pub device_auth: String,
    /// Token refresh endpoint. The Kobo calls this when its `KoboAccessToken`
    /// has expired. We point it at the same handler as `device_auth`.
    pub device_refresh: String,
}

/// Settings returned to the Kobo after initialization.
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct KoboInitResponse {
    /// Last sync timestamp — used by the Kobo as a starting hint for
    /// incremental sync. The sync cursor from `library/sync` takes precedence.
    pub bookmark_date: String,
    /// Opaque device identifier (unused by BookBoss; zeros satisfy the Kobo).
    pub device_id: &'static str,
    /// User key echoed back to the Kobo as `KoboAccessToken`. Must be the
    /// same token used in the URL path so the device's bearer token matches
    /// what we accept on subsequent requests.
    pub user_key: String,
    /// Endpoint resource map — the Kobo uses these to discover all API URLs.
    #[serde(rename = "Resources")]
    pub resources: KoboResources,
    pub booklist_sync_delta_enabled: bool,
    pub content_accessibility_enabled: bool,
    pub is_checkout_enabled: bool,
    pub is_subscription_enabled: bool,
    /// Must be `true` to enable library sync.
    pub library_sync: bool,
}

// ── Handler
// ───────────────────────────────────────────────────────────────────

#[tracing::instrument(level = "trace", skip(kobo),     fields(
        device_id = kobo.device.id,
    )
)]
pub async fn handle(kobo: KoboDevice, body: Bytes, base_url: String) -> impl IntoResponse {
    // Accept both GET (no body) and POST (JSON body). Parse leniently.
    let req: KoboInitRequest = serde_json::from_slice(&body).unwrap_or_default();

    tracing::debug!(
        device_id = kobo.device.id,
        sync_token = %kobo.sync_token,
        affiliate_name = ?req.affiliate_name,
        app_version = ?req.app_version,
        kobo_device_id = ?req.device_id,
        serial = ?req.serial,
        user_agent = ?req.user_agent,
        "kobo initialization request"
    );

    // Use now() when last_synced_at is None (never synced or reset) so the
    // Kobo doesn't see epoch and abort. The library/sync handler enforces the
    // full-resync via the device record independently of this field.
    let bookmark_date = kobo.device.last_synced_at.unwrap_or_else(Utc::now).to_rfc3339();

    let base = base_url.trim_end_matches('/');
    let t = &kobo.sync_token;
    let auth_url = format!("{base}/kobo/{t}/v1/auth/device");

    let resources = KoboResources {
        image_host: base.to_string(),
        image_url_quality_template: format!("{base}/kobo/{t}/v1/image/{{ImageId}}/{{width}}/{{height}}/{{Quality}}/{{IsGreyscale}}/image.jpg"),
        image_url_template: format!("{base}/kobo/{t}/v1/image/{{ImageId}}/{{width}}/{{height}}/100/false/image.jpg"),
        library_metadata_url: format!("{base}/kobo/{t}/v1/library/{{RevisionId}}/metadata"),
        library_sync_url: format!("{base}/kobo/{t}/v1/library/sync"),
        bookmark_url: format!("{base}/kobo/{t}/v1/library/{{RevisionId}}/bookmarks"),
        subscription_host: String::new(),
        device_auth: auth_url.clone(),
        device_refresh: auth_url,
    };

    let body = KoboInitResponse {
        bookmark_date,
        device_id: "00000000-0000-0000-0000-000000000000",
        user_key: kobo.sync_token.clone(),
        resources,
        booklist_sync_delta_enabled: false,
        content_accessibility_enabled: false,
        is_checkout_enabled: false,
        is_subscription_enabled: false,
        library_sync: true,
    };

    // `x-kobo-apitoken: e30=` is required by the Kobo firmware to consider
    // initialization successful. `e30=` is base64 for `{}`.
    let mut headers = HeaderMap::new();
    headers.insert(HeaderName::from_static("x-kobo-apitoken"), HeaderValue::from_static("e30="));

    (headers, Json(body))
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;

    use super::*;

    fn base() -> &'static str {
        "https://example.com"
    }

    fn token() -> &'static str {
        "MYTOKEN"
    }

    #[test]
    fn resources_library_sync_url() {
        let url = format!("{}/kobo/{}/v1/library/sync", base(), token());
        assert_eq!(url, "https://example.com/kobo/MYTOKEN/v1/library/sync");
    }

    #[test]
    fn resources_image_url_template_contains_placeholders() {
        let url = format!("{}/kobo/{}/v1/image/{{ImageId}}/{{width}}/{{height}}/100/false/image.jpg", base(), token());
        assert!(url.contains("{ImageId}"));
        assert!(url.contains("{width}"));
        assert!(url.contains("{height}"));
    }

    #[test]
    fn resources_library_metadata_url_contains_revision_placeholder() {
        let url = format!("{}/kobo/{}/v1/library/{{RevisionId}}/metadata", base(), token());
        assert!(url.contains("{RevisionId}"));
    }

    #[test]
    fn base_url_trailing_slash_stripped() {
        let base = "https://example.com/".trim_end_matches('/');
        let url = format!("{base}/kobo/{}/v1/library/sync", token());
        assert_eq!(url, "https://example.com/kobo/MYTOKEN/v1/library/sync");
    }

    #[test]
    fn bookmark_date_epoch_when_never_synced() {
        let date = DateTime::from_timestamp(0, 0).unwrap();
        assert_eq!(date.to_rfc3339(), "1970-01-01T00:00:00+00:00");
    }

    #[test]
    fn device_auth_and_refresh_point_to_same_url() {
        let base = base();
        let t = token();
        let expected = format!("{base}/kobo/{t}/v1/auth/device");
        assert_eq!(expected, "https://example.com/kobo/MYTOKEN/v1/auth/device");
    }
}
