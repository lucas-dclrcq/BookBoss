//! `OpdsUser` extractor for OPDS endpoints.
//!
//! Authenticates via HTTP Basic Auth using the user's BookBoss username and
//! their OPDS-specific password (stored as an Argon2 hash in `user_settings`).
//! Returns 401 if credentials are missing or invalid, 403 if the user lacks
//! the `OpdsAccess` capability.

use std::sync::Arc;

use axum::{
    Extension,
    extract::FromRequestParts,
    http::{StatusCode, header, request::Parts},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use bb_core::{CoreServices, types::Capability, user::User};

/// Holds the authenticated user for an OPDS request.
#[derive(Clone)]
pub struct OpdsUser {
    pub user: User,
}

impl<S: Send + Sync> FromRequestParts<S> for OpdsUser {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // 1. Extract Authorization header.
        let auth_header = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or(StatusCode::UNAUTHORIZED)?;

        // 2. Parse "Basic <base64>" scheme.
        let encoded = auth_header.strip_prefix("Basic ").ok_or(StatusCode::UNAUTHORIZED)?;

        let decoded = STANDARD.decode(encoded).map_err(|_| StatusCode::UNAUTHORIZED)?;
        let credentials = String::from_utf8(decoded).map_err(|_| StatusCode::UNAUTHORIZED)?;

        let (username, password) = credentials.split_once(':').ok_or(StatusCode::UNAUTHORIZED)?;

        if username.is_empty() || password.is_empty() {
            return Err(StatusCode::UNAUTHORIZED);
        }

        // 3. Resolve CoreServices.
        let Extension(core_services) = Extension::<Arc<CoreServices>>::from_request_parts(parts, state)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        // 4. Look up user by username.
        let user = core_services
            .user_service
            .find_by_username(username)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::UNAUTHORIZED)?;

        // 5. Check OpdsAccess capability.
        if !user.has_capability(Capability::OpdsAccess) {
            return Err(StatusCode::FORBIDDEN);
        }

        // 6. Verify OPDS password.
        let valid = core_services
            .opds_service
            .verify_password(&user, password)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        if !valid {
            return Err(StatusCode::UNAUTHORIZED);
        }

        Ok(Self { user })
    }
}
