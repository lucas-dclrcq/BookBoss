//! Kobo sync router.
//!
//! All Kobo device endpoints live under `/kobo/{sync_token}/`. The sync token
//! is the device's `DV_`-prefixed token with the prefix stripped — it acts as
//! a bearer credential. See [`KoboDevice`] for the extractor that validates it.
//!
//! # Endpoint map
//!
//! | Method    | Path                                                  | Milestone |
//! |-----------|-------------------------------------------------------|-----------|
//! | POST      | `/kobo/:t/v1/initialization`                          | M8.4      |
//! | GET       | `/kobo/:t/v1/library/sync`                            | M8.5      |
//! | GET       | `/kobo/:t/v1/download/:book_token/:format`            | M8.6      |
//! | GET       | `/kobo/:t/v1/image/:book_token/...`                   | M8.7      |
//! | GET       | `/kobo/:t/v1/library/:uuid/metadata`                  | M8.8      |
//! | GET/POST  | `/kobo/:t/v1/analytics/gettests`                      | M8.8      |
//! | GET       | `/kobo/:t/v1/user/loyalty/benefits`                   | M8.8      |
//! | GET       | `/kobo/:t/v1/library/:uuid/state`                     | M8.8 stub |
//! | PUT       | `/kobo/:t/v1/library/:uuid/state`                     | M8.8 stub |
//! | `{*path}` | `/kobo/:t/{*path}`                                    | M8.8 catch-all |

pub mod book_metadata;
pub mod config;
pub mod cursor;
pub mod download;
pub mod dto;
pub mod extractor;
pub mod image;
pub mod initialization;
pub mod library_delete;
pub mod library_state;
pub mod library_sync;
pub mod stubs;

use std::sync::Arc;

use axum::{Router, routing};
use bb_core::CoreServices;
pub use config::KoboConfig;
pub use extractor::KoboDevice;

/// Builds the Kobo sync router.
///
/// Registers all Kobo protocol endpoints. Specific handlers cover the core
/// sync flow and known ancillary endpoints; a catch-all absorbs any remaining
/// firmware requests and logs them at INFO level.
pub fn kobo_router(base_url: String, core_services: Arc<CoreServices>) -> Router {
    Router::new()
        // M8.4 — device initialization (GET per Calibre-Web/native protocol; POST also accepted)
        .route("/kobo/{sync_token}/v1/initialization", {
            let base_url = base_url.clone();
            let base_url2 = base_url.clone();
            routing::get(move |kobo: KoboDevice, body| initialization::handle(kobo, body, base_url.clone()))
                .post(move |kobo: KoboDevice, body| initialization::handle(kobo, body, base_url2.clone()))
        })
        // M8.5 — incremental library sync
        .route("/kobo/{sync_token}/v1/library/sync", {
            let core = core_services.clone();
            let base = base_url.clone();
            routing::get(move |kobo: KoboDevice, headers| library_sync::handle(kobo, headers, core.clone(), base.clone()))
        })
        // M8.6 — book file download
        .route("/kobo/{sync_token}/v1/download/{book_token}/{format}", {
            let core = core_services.clone();
            routing::get(move |kobo: KoboDevice, params| download::handle(kobo, params, core.clone()))
        })
        // M8.7 — cover image (two variants: with and without quality segment)
        .route("/kobo/{sync_token}/v1/image/{book_token}/{width}/{height}/{quality}/{grey}/image.jpg", {
            let core = core_services.clone();
            routing::get(move |kobo: KoboDevice, params| image::handle(kobo, params, core.clone()))
        })
        .route("/kobo/{sync_token}/v1/image/{book_token}/{width}/{height}/{grey}/image.jpg", {
            let core = core_services.clone();
            routing::get(move |kobo: KoboDevice, params| image::handle(kobo, params, core.clone()))
        })
        // M8.8 — per-book metadata
        .route("/kobo/{sync_token}/v1/library/{uuid}/metadata", {
            let core = core_services.clone();
            let base = base_url.clone();
            routing::get(move |kobo: KoboDevice, params| book_metadata::handle(kobo, params, core.clone(), base.clone()))
        })
        // M8.8 — analytics A/B test config
        .route(
            "/kobo/{sync_token}/v1/analytics/gettests",
            routing::get(stubs::analytics_gettests).post(stubs::analytics_gettests),
        )
        // M8.8 — loyalty benefits
        .route("/kobo/{sync_token}/v1/user/loyalty/benefits", routing::get(stubs::loyalty_benefits))
        // M8.8 — reading state (GET stub; PUT handles DeleteEntitlement)
        .route("/kobo/{sync_token}/v1/library/{uuid}/state", {
            let core = core_services.clone();
            routing::get(stubs::library_state_get).put(move |kobo: KoboDevice, params, body| library_state::handle(kobo, params, core.clone(), body))
        })
        // M8.8 — book delete (Kobo removed book from device; drop DeviceBook so it re-syncs as New)
        .route("/kobo/{sync_token}/v1/library/{uuid}", {
            let core = core_services.clone();
            routing::delete(move |kobo: KoboDevice, params| library_delete::handle(kobo, params, core.clone()))
        })
        // M8.8 — catch-all: log and return {} for any unrecognised Kobo path
        .route("/kobo/{sync_token}/{*path}", routing::any(stubs::catch_all))
}
