//! Transparent proxy to `storeapi.kobo.com`.
//!
//! Forwards Kobo device requests to the real Kobo store, passing through
//! request headers and returning response headers. Hop-by-hop headers are
//! stripped from the store response before it is returned to the device.

use std::sync::LazyLock;

use axum::{
    body::Bytes,
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
};

pub const KOBO_STORE_URL: &str = "https://storeapi.kobo.com";

/// Headers that must not be forwarded between hops.
/// Note: `content-encoding` is excluded because reqwest auto-decompresses
/// gzip responses and removes that header before we see it.
const HOP_BY_HOP: &[&str] = &["connection", "content-length", "transfer-encoding"];

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| reqwest::Client::builder().build().expect("failed to build Kobo store proxy client"));

/// Forward a request to the Kobo store and return `(status, headers, body)`.
///
/// `store_path` is the path (plus optional query string) with the
/// `/kobo/{token}` prefix already stripped — e.g. `"/v1/initialization"`.
///
/// Returns `None` if the store is unreachable or returns an error.
pub async fn proxy_to_store(store_path: &str, method: Method, req_headers: &HeaderMap, body: Bytes) -> Option<(StatusCode, HeaderMap, Bytes)> {
    let url = format!("{KOBO_STORE_URL}{store_path}");

    // Forward all request headers except Host.
    let mut outgoing = reqwest::header::HeaderMap::new();
    for (name, value) in req_headers {
        if name.as_str() == "host" {
            continue;
        }
        if let (Ok(n), Ok(v)) = (
            reqwest::header::HeaderName::from_bytes(name.as_str().as_bytes()),
            reqwest::header::HeaderValue::from_bytes(value.as_bytes()),
        ) {
            outgoing.append(n, v);
        }
    }

    let result = CLIENT.request(method, &url).headers(outgoing).body(body).send().await;

    let response = match result {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(store_url = %url, error = %e, "Kobo store proxy request failed");
            return None;
        }
    };

    let status = StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);

    // Forward response headers, stripping hop-by-hop headers.
    let mut resp_headers = HeaderMap::new();
    for (name, value) in response.headers() {
        if HOP_BY_HOP.contains(&name.as_str()) {
            continue;
        }
        if let (Ok(n), Ok(v)) = (HeaderName::from_bytes(name.as_str().as_bytes()), HeaderValue::from_bytes(value.as_bytes())) {
            resp_headers.append(n, v);
        }
    }

    match response.bytes().await {
        Ok(body) => Some((status, resp_headers, body)),
        Err(e) => {
            tracing::warn!(error = %e, "failed to read Kobo store response body");
            None
        }
    }
}

/// Convert a proxy result into an axum [`Response`].
///
/// Falls back to `200 {}` if the store was unreachable so the Kobo firmware
/// does not enter an error state.
pub fn into_fallback_response(result: Option<(StatusCode, HeaderMap, Bytes)>) -> Response {
    match result {
        Some((status, headers, body)) => (status, headers, body).into_response(),
        None => (StatusCode::OK, Bytes::from_static(b"{}")).into_response(),
    }
}
