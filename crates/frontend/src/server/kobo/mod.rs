//! Kobo sync router.
//!
//! All Kobo device endpoints live under `/kobo/{sync_token}/`. The sync token
//! is the device's `DV_`-prefixed token with the prefix stripped — it acts as
//! a bearer credential. See [`KoboDevice`] for the extractor that validates it.
//!
//! # Endpoint map
//!
//! | Method | Path                                         | Milestone |
//! |--------|----------------------------------------------|-----------|
//! | POST   | `/kobo/:t/v1/initialization`                 | M8.4      |
//! | GET    | `/kobo/:t/v1/library/sync`                   | M8.5      |
//! | GET    | `/kobo/:t/v1/download/:book_token/:format`   | M8.6      |
//! | GET    | `/kobo/:t/v1/image/:book_token/...`          | M8.7      |
//! | POST   | `/kobo/:t/v1/analytics/event`                | M8.8      |
//! | GET    | `/kobo/:t/v1/products/list`                  | M8.8      |
//! | GET    | `/kobo/:t/v1/library/:uuid/metadata`         | M8.8      |

pub mod cursor;
pub mod extractor;
pub mod initialization;
pub mod library_sync;

use std::sync::Arc;

use axum::{Router, http::StatusCode, routing};
use bb_core::CoreServices;
pub use extractor::KoboDevice;

/// Builds the Kobo sync router.
///
/// Registers all Kobo protocol endpoints. Handlers for M8.4–M8.7 are
/// implemented in their own milestones; the remaining analytical/ancillary
/// endpoints (M8.8) are stubbed with `501 Not Implemented` until then.
pub fn kobo_router(base_url: String, core_services: Arc<CoreServices>) -> Router {
    Router::new()
        // M8.4 — device initialization
        .route("/kobo/{sync_token}/v1/initialization", {
            let base_url = base_url.clone();
            routing::post(move |kobo: KoboDevice, body| initialization::handle(kobo, body, base_url.clone()))
        })
        // M8.5 — incremental library sync
        .route("/kobo/{sync_token}/v1/library/sync", {
            let core = core_services.clone();
            let base = base_url.clone();
            routing::get(move |kobo: KoboDevice, headers| library_sync::handle(kobo, headers, core.clone(), base.clone()))
        })
        // M8.6 — book file download
        .route("/kobo/{sync_token}/v1/download/{book_token}/{format}", routing::get(not_implemented))
        // M8.7 — cover image
        .route(
            "/kobo/{sync_token}/v1/image/{book_token}/{width}/{height}/{quality}/{grey}",
            routing::get(not_implemented),
        )
        // M8.8 — ancillary / analytical stubs
        .route("/kobo/{sync_token}/v1/analytics/event", routing::post(not_implemented))
        .route("/kobo/{sync_token}/v1/products/list", routing::get(not_implemented))
        .route("/kobo/{sync_token}/v1/library/{uuid}/metadata", routing::get(not_implemented))
}

async fn not_implemented() -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}
