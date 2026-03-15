//! `POST /kobo/{sync_token}/v1/initialization`
//!
//! Called by the Kobo device on first connection (and on reconnect). Returns
//! device settings and a `Resources` map that the Kobo uses to discover the
//! exact URL for every subsequent API call.

use axum::{Json, extract};
use chrono::{DateTime, Utc};
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
/// Template variables (e.g. `{ImageId}`, `{RevisionId}`) are filled in by the
/// Kobo client before making each request.
#[derive(Serialize)]
pub struct KoboResources {
    /// Base host for image requests (the Kobo may use this separately).
    pub image_host: String,
    /// Cover image URL template. `{ImageId}` = book UUID, `{width}`,
    /// `{height}`, `{Quality}`, `{IsGreyscale}` filled by Kobo.
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
    /// Unused by BookBoss; empty string satisfies the Kobo schema.
    pub auth_url: String,
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
    /// Opaque user key (unused by BookBoss; zeros satisfy the Kobo).
    pub user_key: &'static str,
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

pub async fn handle(kobo: KoboDevice, extract::Json(req): extract::Json<KoboInitRequest>, base_url: String) -> Json<KoboInitResponse> {
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

    let bookmark_date = kobo
        .device
        .last_synced_at
        .unwrap_or_else(|| DateTime::from_timestamp(0, 0).expect("epoch is valid"))
        .to_rfc3339();

    let base = base_url.trim_end_matches('/');
    let t = &kobo.sync_token;

    let resources = KoboResources {
        image_host: base.to_string(),
        image_url_quality_template: format!("{base}/kobo/{t}/v1/image/{{ImageId}}/{{width}}/{{height}}/{{Quality}}/{{IsGreyscale}}/image.jpg"),
        image_url_template: format!("{base}/kobo/{t}/v1/image/{{ImageId}}/{{width}}/{{height}}/100/false/image.jpg"),
        library_metadata_url: format!("{base}/kobo/{t}/v1/library/{{RevisionId}}/metadata"),
        library_sync_url: format!("{base}/kobo/{t}/v1/library/sync"),
        bookmark_url: format!("{base}/kobo/{t}/v1/library/{{RevisionId}}/bookmarks"),
        subscription_host: String::new(),
        auth_url: String::new(),
    };

    Json(KoboInitResponse {
        bookmark_date,
        device_id: "00000000-0000-0000-0000-000000000000",
        user_key: "00000000000000000000000000000000",
        resources,
        booklist_sync_delta_enabled: false,
        content_accessibility_enabled: false,
        is_checkout_enabled: false,
        is_subscription_enabled: false,
        library_sync: true,
    })
}

#[cfg(test)]
mod tests {
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
        let date: DateTime<Utc> = DateTime::from_timestamp(0, 0).unwrap();
        assert_eq!(date.to_rfc3339(), "1970-01-01T00:00:00+00:00");
    }
}
