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
    extract::Path,
    http::{Method, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

use super::KoboDevice;

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

/// `GET /v1/library/{uuid}/state` — per-book reading state query.
///
/// Returns an empty state array. The Kobo won't crash, but won't restore
/// reading positions until we implement full reading state sync.
pub async fn library_state_get(_kobo: KoboDevice, Path(_params): Path<HashMap<String, String>>) -> impl IntoResponse {
    Json(json!([{}]))
}

/// `PUT /v1/library/{uuid}/state` — per-book reading state update.
///
/// Acknowledges the state push as successful without persisting anything.
/// The `UpdateResults` array is intentionally empty (no entitlement IDs
/// to echo back without parsing the request body).
pub async fn library_state_put(_kobo: KoboDevice, Path(_params): Path<HashMap<String, String>>) -> impl IntoResponse {
    Json(json!({
        "RequestResult": "Success",
        "UpdateResults": []
    }))
}

// ── Catch-all
// ─────────────────────────────────────────────────────────────────

/// Catch-all for any Kobo path not matched by a specific route.
///
/// Returns `200 {}` so the firmware does not enter an error state.
/// Every call is logged at `INFO` level so unrecognised paths show up in
/// traces and can be promoted to explicit handlers when needed.
#[tracing::instrument(
    level = "info",
    skip(kobo),
    fields(
        device_id = kobo.device.id,
        kobo_path = %uri.path(),
        http_method = %method,
    )
)]
pub async fn catch_all(kobo: KoboDevice, method: Method, uri: Uri, Path(_params): Path<HashMap<String, String>>) -> Response {
    tracing::info!("unhandled Kobo request");
    (StatusCode::OK, [(header::CONTENT_TYPE, "application/json")], "{}").into_response()
}
