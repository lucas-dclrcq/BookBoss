//! `PUT /kobo/{sync_token}/v1/library/{uuid}/state`
//!
//! Handles per-book reading state updates from the Kobo device. The body is a
//! JSON array of state items; if any item carries `"DeleteEntitlement": true`
//! the book is removed from the device's sync list (same effect as the DELETE
//! endpoint, which some firmware versions never call).

use std::{collections::HashMap, str::FromStr, sync::Arc};

use axum::{Json, extract::Path, http::StatusCode, response::IntoResponse};
use bb_core::{CoreServices, book::BookToken};
use serde::Deserialize;

use super::KoboDevice;

#[derive(Debug, Deserialize)]
pub(super) struct StateItem {
    #[serde(rename = "DeleteEntitlement", default)]
    delete_entitlement: bool,
}

#[tracing::instrument(level = "trace", skip(kobo, core_services),
    fields(
        device_id = kobo.device.id,
    )
)]
pub async fn handle(
    kobo: KoboDevice,
    Path(params): Path<HashMap<String, String>>,
    core_services: Arc<CoreServices>,
    Json(items): Json<Vec<StateItem>>,
) -> impl IntoResponse {
    let wants_delete = items.iter().any(|i| i.delete_entitlement);

    if wants_delete {
        let Some(uuid) = params.get("uuid") else {
            return StatusCode::BAD_REQUEST.into_response();
        };

        let full_token = format!("BK_{uuid}");
        let token = match BookToken::from_str(&full_token) {
            Ok(t) => t,
            Err(_) => return StatusCode::OK.into_response(),
        };

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

        tracing::debug!(device_id = kobo.device.id, book_id = book.id, "kobo delete entitlement via state");
    }

    Json(serde_json::json!({
        "RequestResult": "Success",
        "UpdateResults": []
    }))
    .into_response()
}
