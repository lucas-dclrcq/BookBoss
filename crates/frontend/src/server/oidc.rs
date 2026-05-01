//! OIDC SSO authentication handlers.
//!
//! Implements the OAuth2 authorization code flow with PKCE for SSO login.
//! See `.insights/shared/specs/BB-13-spec-sso-oidc-auth.md` for the design.
#![expect(dead_code, reason = "wired up in Task 8")]

use std::sync::Arc;

use axum::{
    Extension, Router,
    response::{IntoResponse, Redirect},
    routing::get,
};
use axum_session::Session;
use openidconnect::{
    AuthenticationFlow, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet, EndpointNotSet, EndpointSet, IssuerUrl, Nonce, PkceCodeChallenge, RedirectUrl,
    Scope,
    core::{CoreClient, CoreProviderMetadata, CoreResponseType},
};
use serde::{Deserialize, Serialize};

use crate::{OidcConfig, server::BackendSessionPool};

/// Key under which we store the in-flight OIDC session data
/// (state/nonce/PKCE verifier) in the axum session.
pub(crate) const SESSION_KEY: &str = "oidc_in_flight";

/// Data we round-trip through the user's session for the OIDC redirect.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct OidcSessionData {
    pub state: String,
    pub nonce: String,
    pub pkce_verifier: String,
}

/// Errors that can occur during OIDC client initialization at startup.
#[derive(Debug, thiserror::Error)]
pub(crate) enum OidcInitError {
    #[error("invalid OIDC discovery URL: {0}")]
    InvalidDiscoveryUrl(String),
    #[error("invalid base URL for OIDC redirect: {0}")]
    InvalidRedirectUrl(String),
    #[error("OIDC discovery failed: {0}")]
    DiscoveryFailed(String),
    #[error("OIDC HTTP client init failed: {0}")]
    HttpClientFailed(String),
}

/// Type alias for the configured `CoreClient` state produced after OIDC
/// discovery: auth URL is always set, token URL is optional (per spec),
/// all other endpoint URLs are absent at the oauth2 layer.
type DiscoveredCoreClient = CoreClient<
    EndpointSet,      // HasAuthUrl
    EndpointNotSet,   // HasDeviceAuthUrl
    EndpointNotSet,   // HasIntrospectionUrl
    EndpointNotSet,   // HasRevocationUrl
    EndpointMaybeSet, // HasTokenUrl
    EndpointMaybeSet, // HasUserInfoUrl
>;

/// A configured OIDC client ready to handle the start/callback flow.
pub(crate) struct OidcClient {
    pub(crate) client: DiscoveredCoreClient,
    pub(crate) button_label: String,
}

impl OidcClient {
    /// Performs OIDC discovery and constructs a configured client.
    ///
    /// `discovery_url` is the full URL (typically ending in
    /// `/.well-known/openid-configuration`). The `openidconnect` crate's
    /// `discover_async` method takes an issuer URL — so we strip the
    /// well-known suffix to derive the issuer.
    ///
    /// `base_url` is the BookBoss frontend base URL (from
    /// `FrontendConfig::base_url`); used to construct the redirect URI.
    pub(crate) async fn new(config: &OidcConfig, base_url: &str) -> Result<Self, OidcInitError> {
        const WELL_KNOWN_SUFFIX: &str = "/.well-known/openid-configuration";

        // The three Option fields below are guaranteed Some by
        // `OidcConfig::is_valid()`, which bookboss calls during startup before
        // this constructor is reached. The `ok_or_else` guards are defensive
        // only — they should never trigger in practice.
        let discovery_url = config
            .discovery_url
            .as_deref()
            .ok_or_else(|| OidcInitError::InvalidDiscoveryUrl("missing discovery_url".to_string()))?;
        let client_id = config
            .client_id
            .as_deref()
            .ok_or_else(|| OidcInitError::InvalidDiscoveryUrl("missing client_id".to_string()))?;
        let client_secret = config
            .client_secret
            .as_deref()
            .ok_or_else(|| OidcInitError::InvalidDiscoveryUrl("missing client_secret".to_string()))?;

        let issuer_str = discovery_url.strip_suffix(WELL_KNOWN_SUFFIX).unwrap_or(discovery_url);
        let issuer = IssuerUrl::new(issuer_str.to_string()).map_err(|e| OidcInitError::InvalidDiscoveryUrl(e.to_string()))?;

        let redirect =
            RedirectUrl::new(format!("{}/auth/oidc/callback", base_url.trim_end_matches('/'))).map_err(|e| OidcInitError::InvalidRedirectUrl(e.to_string()))?;

        let http_client = openidconnect::reqwest::ClientBuilder::new()
            .redirect(openidconnect::reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| OidcInitError::HttpClientFailed(e.to_string()))?;

        let provider_metadata = CoreProviderMetadata::discover_async(issuer, &http_client)
            .await
            .map_err(|e| OidcInitError::DiscoveryFailed(e.to_string()))?;

        let client = CoreClient::from_provider_metadata(
            provider_metadata,
            ClientId::new(client_id.to_string()),
            Some(ClientSecret::new(client_secret.to_string())),
        )
        .set_redirect_uri(redirect);

        Ok(Self {
            client,
            button_label: config.button_label().to_string(),
        })
    }
}

/// Returns an axum router with the OIDC start and callback routes.
///
/// Task 7 will replace the callback stub with the real implementation.
pub(crate) fn oidc_router() -> Router {
    Router::new()
        .route("/auth/oidc/start", get(start_handler))
        .route("/auth/oidc/callback", get(callback_handler_stub))
}

/// Initiates the OIDC authorization code flow. Generates state, nonce, and a
/// PKCE verifier, stores them in the user's session, and redirects to the
/// IdP authorization endpoint.
async fn start_handler(Extension(client): Extension<Arc<OidcClient>>, session: Session<BackendSessionPool>) -> axum::response::Response {
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

    let (auth_url, csrf_token, nonce) = client
        .client
        .authorize_url(
            AuthenticationFlow::<CoreResponseType>::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .add_scope(Scope::new("openid".to_string()))
        .add_scope(Scope::new("email".to_string()))
        .set_pkce_challenge(pkce_challenge)
        .url();

    let session_data = OidcSessionData {
        state: csrf_token.secret().clone(),
        nonce: nonce.secret().clone(),
        pkce_verifier: pkce_verifier.secret().clone(),
    };

    // axum_session::Session::set is infallible (writes to in-memory session
    // state; persistence happens via SessionLayer middleware after the
    // response is built).
    session.set(SESSION_KEY, session_data);

    Redirect::to(auth_url.as_str()).into_response()
}

async fn callback_handler_stub() -> &'static str {
    "OIDC callback (not yet implemented)"
}
