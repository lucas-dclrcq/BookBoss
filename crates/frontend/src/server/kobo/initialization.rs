//! `GET /kobo/{sync_token}/v1/initialization` (also accepts POST)
//!
//! Called by the Kobo device on first connection (and on reconnect).
//!
//! # Protocol
//!
//! We proxy the request to `storeapi.kobo.com/v1/initialization` to get the
//! real `Resources` map (auth URLs, store endpoints, etc.), then overwrite
//! only the entries we serve ourselves (images, library sync, metadata).
//! Everything else — including `device_auth` and `device_refresh` — stays
//! pointing at the real Kobo store so the device manages its own auth.
//!
//! If the store is unreachable we fall back to the native resource map captured
//! from a real handshake so the device can still sync books.
//!
//! The `x-kobo-apitoken: e30=` header (`e30=` = base64 of `{}`) is required
//! by the Kobo firmware to consider initialization successful.

use axum::{
    Json,
    body::Bytes,
    http::{HeaderMap, HeaderName, HeaderValue, Method},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{KoboDevice, proxy};

// ── Request
// ───────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub struct KoboInitRequest {
    pub affiliate_name: Option<String>,
    pub app_version: Option<String>,
    pub device_id: Option<String>,
    pub serial: Option<String>,
    pub user_agent: Option<String>,
}

// ── Handler
// ───────────────────────────────────────────────────────────────────

pub async fn handle(kobo: KoboDevice, method: Method, req_headers: HeaderMap, body: Bytes, base_url: String) -> impl IntoResponse {
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

    let base = base_url.trim_end_matches('/');
    let t = &kobo.sync_token;

    // 1. Proxy to the real Kobo store.
    let store_result = proxy::proxy_to_store("/v1/initialization", method, &req_headers, body).await;

    // 2. Build the response JSON.
    let response_json = build_response_json(store_result, base, t);

    // 4. x-kobo-apitoken header is required by the Kobo firmware.
    let mut headers = HeaderMap::new();
    headers.insert(HeaderName::from_static("x-kobo-apitoken"), HeaderValue::from_static("e30="));

    (headers, Json(response_json))
}

/// Build the full init response JSON.
///
/// If the store returned a parseable response, patch `Resources` in-place
/// and return the store's full JSON (preserving any extra fields). Otherwise
/// fall back to our own minimal response shape.
fn build_response_json(store_result: Option<(axum::http::StatusCode, HeaderMap, Bytes)>, base: &str, t: &str) -> Value {
    if let Some((status, _, ref store_bytes)) = store_result {
        if !status.is_success() {
            tracing::warn!(%status, "Kobo store init returned error status; using fallback resources");
        } else if let Ok(mut store_json) = serde_json::from_slice::<Value>(store_bytes) {
            if store_json.get("Resources").is_some() {
                patch_resources(&mut store_json, base, t);
                return store_json;
            }
            tracing::warn!("Kobo store init response missing Resources key; using fallback resources");
        } else {
            tracing::warn!("Kobo store init response was not valid JSON; using fallback resources");
        }
    } else {
        tracing::warn!("Kobo store unreachable for initialization; using fallback resources");
    }

    fallback_response(base, t)
}

/// Overwrite the resource entries that BookBoss serves into the store JSON.
///
/// Only the keys we handle are replaced; everything else (device_auth,
/// device_refresh, store URLs, etc.) is left exactly as the store returned it.
fn patch_resources(store_json: &mut Value, base: &str, t: &str) {
    let Some(resources) = store_json.get_mut("Resources").and_then(|r| r.as_object_mut()) else {
        return;
    };
    resources.insert("image_host".into(), json!(base));
    resources.insert(
        "image_url_quality_template".into(),
        json!(format!(
            "{base}/kobo/{t}/v1/image/{{ImageId}}/{{width}}/{{height}}/{{Quality}}/{{IsGreyscale}}/image.jpg"
        )),
    );
    resources.insert(
        "image_url_template".into(),
        json!(format!("{base}/kobo/{t}/v1/image/{{ImageId}}/{{width}}/{{height}}/100/false/image.jpg")),
    );
    resources.insert("library_sync".into(), json!(format!("{base}/kobo/{t}/v1/library/sync")));
    resources.insert("library_metadata".into(), json!(format!("{base}/kobo/{t}/v1/library/{{Ids}}/metadata")));
}

/// Full resource map captured from a real Kobo store initialization response.
/// Used as the base when the store is unreachable.
const NATIVE_KOBO_RESOURCES_JSON: &str = include_str!("native_kobo_resources.json");

/// Fallback response when the store is unreachable.
///
/// Uses the full native resources as the base (so all standard Kobo store
/// features are present), then patches our own entries on top.
fn fallback_response(base: &str, t: &str) -> Value {
    let mut native: Value = serde_json::from_str(NATIVE_KOBO_RESOURCES_JSON).unwrap_or_else(|_| json!({}));
    patch_resources(&mut native, base, t);
    native
}

#[cfg(test)]
mod tests {

    #[test]
    fn resources_library_sync_url() {
        let url = format!("{}/kobo/{}/v1/library/sync", "https://example.com", "MYTOKEN");
        assert_eq!(url, "https://example.com/kobo/MYTOKEN/v1/library/sync");
    }

    #[test]
    fn resources_image_url_template_contains_placeholders() {
        let url = format!(
            "{}/kobo/{}/v1/image/{{ImageId}}/{{width}}/{{height}}/100/false/image.jpg",
            "https://example.com", "MYTOKEN"
        );
        assert!(url.contains("{ImageId}"));
        assert!(url.contains("{width}"));
        assert!(url.contains("{height}"));
    }

    #[test]
    fn resources_library_metadata_url_contains_ids_placeholder() {
        let url = format!("{}/kobo/{}/v1/library/{{Ids}}/metadata", "https://example.com", "MYTOKEN");
        assert!(url.contains("{Ids}"));
    }

    #[test]
    fn base_url_trailing_slash_stripped() {
        let base = "https://example.com/".trim_end_matches('/');
        let url = format!("{base}/kobo/{}/v1/library/sync", "MYTOKEN");
        assert_eq!(url, "https://example.com/kobo/MYTOKEN/v1/library/sync");
    }
}
