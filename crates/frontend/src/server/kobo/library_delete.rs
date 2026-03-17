//! `DELETE /kobo/{sync_token}/v1/library/{uuid}`
//!
//! Called when the user removes a book from the Kobo device. Deletes the
//! `DeviceBook` record so the book is re-delivered as `New` on the next sync.

use std::{collections::HashMap, sync::Arc};

use axum::{extract::Path, http::StatusCode, response::IntoResponse};
use bb_core::{CoreServices, book::BookToken};

use super::KoboDevice;

// ── Handler
// ─────────────────────────────────────────────────────────────────

pub async fn handle(kobo: KoboDevice, Path(params): Path<HashMap<String, String>>, core_services: Arc<CoreServices>) -> impl IntoResponse {
    let Some(uuid) = params.get("uuid") else {
        return StatusCode::BAD_REQUEST.into_response();
    };

    let Ok(token) = BookToken::from_encoded_id(&uuid) else {
        return StatusCode::OK.into_response();
    };

    tracing::debug!(device_id = kobo.device.id, book_token = %token, "delete book from device");

    let book = match core_services.book_service.find_book_by_token(&token).await {
        Ok(Some(b)) => b,
        Ok(None) => return StatusCode::OK.into_response(), // idempotent
        Err(e) => {
            tracing::error!(error = ?e, "find_book_by_token failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if let Err(e) = core_services.device_service.remove_book_from_device(kobo.device.id, book.id).await {
        tracing::error!(error = ?e, "remove_book_from_device failed");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    tracing::debug!(device_id = kobo.device.id, book_id = book.id, "kobo library delete");

    StatusCode::OK.into_response()
}
