//! Minimal-response stubs for Kobo ancillary endpoints (M8.8).
//!
//! The catch-all handler is instrumented so any unrecognised path and method
//! appears in the trace log — useful for discovering endpoints the firmware
//! hits that we haven't explicitly handled yet.
//!
//! Sources consulted: Komga (catch-all `{*path}` → `{}`) and Calibre-Web
//! (`redirect_or_proxy_request` → `{}` when no proxy configured).

use std::collections::HashMap;

use axum::{
    Json,
    body::Bytes,
    extract::Path,
    http::{HeaderMap, Method, Uri},
    response::{IntoResponse, Response},
};
use serde_json::json;

use super::{KoboDevice, proxy};

// ── Simple stubs
// ─────────────────────────────────────────────────────────────────

/// `GET /v1/analytics/gettests` — A/B test config endpoint.
///
/// Calibre-Web returns
/// `{"Result":"Success","TestKey":"<X-Kobo-userkey>","Tests":{}}`.
/// Komga has this endpoint commented out (never reached). We implement the
/// Calibre-Web shape since the firmware expects it.
pub async fn analytics_gettests(_kobo: KoboDevice, req_headers: axum::http::HeaderMap) -> impl IntoResponse {
    let test_key = req_headers.get("x-kobo-userkey").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    Json(json!({
        "Result": "Success",
        "TestKey": test_key,
        "Tests": {}
    }))
}

/// `GET /v1/user/loyalty/benefits` — loyalty points/benefits endpoint.
///
/// Calibre-Web returns `{"Benefits":{}}`. Komga returns `{}` via catch-all.
pub async fn loyalty_benefits(_kobo: KoboDevice) -> impl IntoResponse {
    Json(json!({ "Benefits": {} }))
}

// ── Catch-all
// ─────────────────────────────────────────────────────────────────

/// Catch-all for any Kobo path not matched by a specific route.
///
/// Proxies the request transparently to `storeapi.kobo.com`, forwarding
/// request headers and returning response headers. This covers auth, user
/// profile, analytics, and any other store endpoint the firmware needs.
///
/// Falls back to `200 {}` if the store is unreachable.
#[tracing::instrument(
    level = "info",
    skip(kobo, req_headers, body),
    fields(
        device_id = kobo.device.id,
        kobo_path = %uri.path(),
        http_method = %method,
    )
)]
pub async fn catch_all(
    kobo: KoboDevice,
    method: Method,
    uri: Uri,
    req_headers: HeaderMap,
    Path(params): Path<HashMap<String, String>>,
    body: Bytes,
) -> Response {
    // Strip the /kobo/{token} prefix to get the bare store path.
    let path = uri.path();
    let prefix = format!("/kobo/{}", kobo.sync_token);
    let store_path = path.strip_prefix(&prefix).unwrap_or(path);
    let store_path_with_query = match uri.query() {
        Some(q) => format!("{store_path}?{q}"),
        None => store_path.to_string(),
    };

    tracing::info!(store_path = %store_path_with_query, "proxying unhandled Kobo request to store");

    let result = proxy::proxy_to_store(&store_path_with_query, method, &req_headers, body).await;
    proxy::into_fallback_response(result)
}
