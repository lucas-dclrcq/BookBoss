//! OIDC SSO authentication handlers.
//!
//! Implements the OAuth2 authorization code flow with PKCE for SSO login.
//! See `.insights/shared/specs/BB-13-spec-sso-oidc-auth.md` for the design.

use std::sync::Arc;

use axum::{
    Extension, Router,
    extract::Query,
    response::{IntoResponse, Redirect},
    routing::get,
};
use axum_session::Session;
use openidconnect::{
    AuthenticationFlow, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet, EndpointNotSet, EndpointSet, IssuerUrl, Nonce,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
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

/// Query parameters returned by the IdP on the callback redirect.
#[derive(Debug, Deserialize)]
pub(crate) struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    /// Set when the IdP returns an error response per RFC 6749 §4.1.2.1.
    pub error: Option<String>,
    /// Optional human-readable description accompanying `error`. Logged for
    /// operator visibility; never surfaced to the user.
    pub error_description: Option<String>,
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
    #[allow(dead_code, reason = "read by get_sso_config server fn in Task 9")]
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
pub(crate) fn oidc_router() -> Router {
    Router::new()
        .route("/auth/oidc/start", get(start_handler))
        .route("/auth/oidc/callback", get(callback_handler))
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

/// Handles the OIDC callback. Validates state, exchanges code for tokens,
/// validates the ID token, matches the email claim against a BookBoss user,
/// and creates a session on success. All failures redirect to
/// `/?login_failed=1` after logging the cause via `tracing::error!`.
#[allow(
    clippy::too_many_lines,
    reason = "OIDC validation flow is sequential — splitting hides the security-sensitive ordering"
)]
async fn callback_handler(
    Extension(client): Extension<Arc<OidcClient>>,
    Extension(core_services): Extension<Arc<bb_core::CoreServices>>,
    auth_session: super::AuthSession,
    session: Session<BackendSessionPool>,
    Query(query): Query<CallbackQuery>,
) -> axum::response::Response {
    // ── Read and clear in-flight session data ────────────────────────────
    let Some(session_data) = session.get::<OidcSessionData>(SESSION_KEY) else {
        tracing::error!("OIDC callback: no in-flight session data found");
        return failure_redirect();
    };
    session.remove(SESSION_KEY);

    // ── Handle IdP-side error response ────────────────────────────────────
    if let Some(err) = query.error.as_deref() {
        tracing::error!(
            error = %err,
            error_description = query.error_description.as_deref().unwrap_or("(none)"),
            "OIDC callback: IdP returned error"
        );
        return failure_redirect();
    }

    // ── Validate state ────────────────────────────────────────────────────
    let Some(state) = query.state.as_deref() else {
        tracing::error!("OIDC callback: missing state parameter");
        return failure_redirect();
    };
    if state != session_data.state {
        tracing::error!("OIDC callback: state mismatch — possible CSRF attempt");
        return failure_redirect();
    }

    // ── Validate code presence ────────────────────────────────────────────
    let Some(code) = query.code.as_deref() else {
        tracing::error!("OIDC callback: missing code parameter");
        return failure_redirect();
    };

    // ── Build HTTP client for token exchange ──────────────────────────────
    let http_client = match openidconnect::reqwest::ClientBuilder::new()
        .redirect(openidconnect::reqwest::redirect::Policy::none())
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "OIDC callback: failed to build HTTP client");
            return failure_redirect();
        }
    };

    // ── Exchange code for tokens ──────────────────────────────────────────
    // exchange_code() returns Result<CodeTokenRequest, ConfigurationError> on
    // CoreClient with EndpointMaybeSet token URL (our DiscoveredCoreClient type).
    let pkce_verifier = PkceCodeVerifier::new(session_data.pkce_verifier);
    let token_request = match client.client.exchange_code(AuthorizationCode::new(code.to_string())) {
        Ok(req) => req,
        Err(e) => {
            tracing::error!(error = %e, "OIDC callback: token endpoint not configured");
            return failure_redirect();
        }
    };
    let token_response = match token_request.set_pkce_verifier(pkce_verifier).request_async(&http_client).await {
        Ok(resp) => resp,
        Err(e) => {
            tracing::error!(error = %e, "OIDC callback: code exchange failed");
            return failure_redirect();
        }
    };

    // ── Validate ID token ─────────────────────────────────────────────────
    // TokenResponse::id_token() is provided by the openidconnect crate's
    // trait impl on StandardTokenResponse<IdTokenFields<...>, ...>.
    // The verifier checks: signature (JWKS), audience, issuer, expiry, and nonce.
    let Some(id_token) = token_response.id_token() else {
        tracing::error!("OIDC callback: ID token missing from token response");
        return failure_redirect();
    };

    let id_token_verifier = client.client.id_token_verifier();
    let nonce = Nonce::new(session_data.nonce);
    let claims = match id_token.claims(&id_token_verifier, &nonce) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "OIDC callback: ID token validation failed");
            return failure_redirect();
        }
    };

    // ── Extract email claim ───────────────────────────────────────────────
    // email_verified is intentionally NOT checked — the admin controls BookBoss
    // accounts.
    let Some(email_claim) = claims.email() else {
        tracing::error!("OIDC callback: ID token has no email claim — check IdP scope mapping");
        return failure_redirect();
    };

    let email = match bb_core::types::EmailAddress::new(email_claim.as_str()) {
        Ok(e) => e,
        Err(e) => {
            tracing::error!(error = %e, "OIDC callback: email claim is malformed");
            return failure_redirect();
        }
    };

    // ── Look up BookBoss user ─────────────────────────────────────────────
    let user = match core_services.auth_service.is_valid_email(&email).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            tracing::error!(email = %email, "OIDC callback: no BookBoss user with this email");
            return failure_redirect();
        }
        Err(e) => {
            tracing::error!(error = %e, "OIDC callback: user lookup failed");
            return failure_redirect();
        }
    };

    // ── Create session ────────────────────────────────────────────────────
    auth_session.login_user(user.id);
    Redirect::to("/").into_response()
}

fn failure_redirect() -> axum::response::Response {
    Redirect::to("/?login_failed=1").into_response()
}
