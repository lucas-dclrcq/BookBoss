//! KOReader sync protocol server.
//!
//! Implements the 5-endpoint KOReader sync API under `/koreader/`.
//! Auth uses the same OPDS password (x-auth-user + md5(password) as
//! x-auth-key). No device registration — progress is stored directly in
//! UserBookMetadata.

pub mod extractor;
pub mod handlers;

use axum::{Router, routing};

pub fn koreader_router() -> Router {
    Router::new()
        .route("/koreader/healthcheck", routing::get(handlers::healthcheck))
        .route("/koreader/users/create", routing::post(handlers::users_create))
        .route("/koreader/users/auth", routing::get(handlers::users_auth))
        .route("/koreader/syncs/progress", routing::put(handlers::syncs_progress_push))
        .route("/koreader/syncs/progress/{document}", routing::get(handlers::syncs_progress_pull))
}
