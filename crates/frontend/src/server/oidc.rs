//! OIDC SSO authentication handlers.
//!
//! Implements the OAuth2 authorization code flow with PKCE for SSO login.
//! See `.insights/shared/specs/BB-13-spec-sso-oidc-auth.md` for the design.
//!
//! # In-flight state storage
//!
//! The OIDC flow needs to round-trip three values across the IdP redirect:
//! the CSRF state, the nonce, and the PKCE verifier. We store these in an
//! in-memory map keyed by the CSRF state value (which is also the value the
//! IdP echoes back in the callback URL). This avoids any dependency on the
//! axum session cookie surviving the third-party IdP redirect — cookie
//! behavior across redirects is browser-dependent and proxy-fragile.
//!
//! Entries expire after `STATE_ENTRY_TTL` and are cleaned up opportunistically
//! when new entries are inserted.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Extension, Router,
    extract::Query,
    response::{IntoResponse, Redirect},
    routing::get,
};
use openidconnect::{
    AuthenticationFlow, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet, EndpointNotSet, EndpointSet, IssuerUrl, Nonce,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
    core::{CoreClient, CoreProviderMetadata, CoreResponseType},
};
use serde::Deserialize;

use crate::OidcConfig;

/// OIDC in-flight state entries expire 5 minutes after creation. Authorization
/// code flow round-trips are typically a few seconds; 5 minutes is generous.
const STATE_ENTRY_TTL: Duration = Duration::from_secs(5 * 60);

/// Walks an error's source chain and concatenates each layer's `Display`.
/// `openidconnect`'s and `reqwest`'s errors expose useful detail (DNS,
/// TLS, connection refused, etc.) only in their source chain — the
/// top-level message is often a generic "Request failed".
fn error_chain(err: &(dyn std::error::Error + 'static)) -> String {
    let mut msg = err.to_string();
    let mut source = err.source();
    while let Some(s) = source {
        msg.push_str(": ");
        msg.push_str(&s.to_string());
        source = s.source();
    }
    msg
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

/// In-flight OIDC state stored server-side and looked up by CSRF state value.
#[derive(Debug)]
struct StateEntry {
    nonce: String,
    pkce_verifier: String,
    created_at: Instant,
}

/// A configured OIDC client ready to handle the start/callback flow.
pub(crate) struct OidcClient {
    pub(crate) client: DiscoveredCoreClient,
    /// In-flight OIDC state keyed by CSRF state value. Populated in
    /// [`start_handler`] and consumed in [`callback_handler`].
    state_store: Mutex<HashMap<String, StateEntry>>,
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
        // `OidcConfig::is_sso_available()`, which the frontend subsystem
        // checks before calling this constructor. The `ok_or_else` guards are
        // defensive only — they should never trigger in practice.
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

        let provider_metadata = match CoreProviderMetadata::discover_async(issuer, &http_client).await {
            Ok(m) => m,
            Err(first_err) => {
                // Some IdPs (notably Authentik) publish their issuer with a
                // trailing slash that gets stripped along with the well-known
                // suffix. Retry once with the slash toggled before giving up.
                let alt = match issuer_str.strip_suffix('/') {
                    Some(stripped) => stripped.to_string(),
                    None => format!("{issuer_str}/"),
                };
                match IssuerUrl::new(alt) {
                    Ok(alt_issuer) => CoreProviderMetadata::discover_async(alt_issuer, &http_client)
                        .await
                        .map_err(|_| OidcInitError::DiscoveryFailed(error_chain(&first_err)))?,
                    Err(_) => return Err(OidcInitError::DiscoveryFailed(error_chain(&first_err))),
                }
            }
        };

        let client = CoreClient::from_provider_metadata(
            provider_metadata,
            ClientId::new(client_id.to_string()),
            Some(ClientSecret::new(client_secret.to_string())),
        )
        .set_redirect_uri(redirect);

        Ok(Self {
            client,
            state_store: Mutex::new(HashMap::new()),
        })
    }

    /// Stores the in-flight state for an OIDC flow keyed by the CSRF state
    /// value. Also opportunistically purges expired entries.
    fn store_state(&self, state: String, nonce: String, pkce_verifier: String) {
        let now = Instant::now();
        let mut store = self.state_store.lock().expect("state_store mutex poisoned");
        store.retain(|_, entry| now.duration_since(entry.created_at) < STATE_ENTRY_TTL);
        store.insert(
            state,
            StateEntry {
                nonce,
                pkce_verifier,
                created_at: now,
            },
        );
    }

    /// Removes and returns the in-flight state for a given CSRF state value.
    /// Returns `None` if the entry is missing or expired (also removed when
    /// expired).
    fn take_state(&self, state: &str) -> Option<StateEntry> {
        let now = Instant::now();
        let mut store = self.state_store.lock().expect("state_store mutex poisoned");
        let entry = store.remove(state)?;
        if now.duration_since(entry.created_at) > STATE_ENTRY_TTL {
            None
        } else {
            Some(entry)
        }
    }
}

/// Returns an axum router with the OIDC start and callback routes.
pub(crate) fn oidc_router() -> Router {
    Router::new()
        .route("/auth/oidc/start", get(start_handler))
        .route("/auth/oidc/callback", get(callback_handler))
}

/// Initiates the OIDC authorization code flow. Generates state, nonce, and a
/// PKCE verifier, stores them in the server-side state store keyed by the
/// state value, and redirects to the IdP authorization endpoint.
async fn start_handler(Extension(client): Extension<Arc<OidcClient>>) -> axum::response::Response {
    tracing::info!("OIDC start: handler entered");
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
        // `profile` gives us `preferred_username` / `name` for provisioned
        // accounts; harmless for the match-by-email path.
        .add_scope(Scope::new("profile".to_string()))
        .set_pkce_challenge(pkce_challenge)
        .url();

    client.store_state(csrf_token.secret().clone(), nonce.secret().clone(), pkce_verifier.secret().clone());

    Redirect::to(auth_url.as_str()).into_response()
}

/// Handles the OIDC callback. Validates state via lookup in the in-flight
/// state store, exchanges code for tokens, validates the ID token, matches
/// the email claim against a BookBoss user, and creates a session on success.
/// All failures redirect to `/?login_failed=1` after logging the cause via
/// `tracing::error!`.
#[allow(
    clippy::too_many_lines,
    reason = "OIDC validation flow is sequential — splitting hides the security-sensitive ordering"
)]
async fn callback_handler(
    Extension(client): Extension<Arc<OidcClient>>,
    Extension(core_services): Extension<Arc<bb_core::CoreServices>>,
    Extension(auto_provision): Extension<super::AutoProvisionEnabled>,
    auth_session: super::AuthSession,
    Query(query): Query<CallbackQuery>,
) -> axum::response::Response {
    tracing::info!("OIDC callback: received");

    // ── Extract state and look up the in-flight entry ─────────────────────
    // The lookup IS the CSRF defense: only state values we generated and
    // stored will succeed. Take-and-remove also prevents replay.
    let Some(state) = query.state.as_deref() else {
        tracing::error!("OIDC callback: missing state parameter");
        return failure_redirect();
    };
    let Some(entry) = client.take_state(state) else {
        tracing::error!("OIDC callback: state not found in store (expired, replayed, or CSRF attempt)");
        return failure_redirect();
    };
    tracing::info!("OIDC callback: state validated");

    // ── Handle IdP-side error response ────────────────────────────────────
    if let Some(err) = query.error.as_deref() {
        tracing::error!(
            error = %err,
            error_description = query.error_description.as_deref().unwrap_or("(none)"),
            "OIDC callback: IdP returned error"
        );
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
    let pkce_verifier = PkceCodeVerifier::new(entry.pkce_verifier);
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
            tracing::error!(error = %error_chain(&e), "OIDC callback: code exchange failed");
            return failure_redirect();
        }
    };
    tracing::info!("OIDC callback: token exchange succeeded");

    // ── Validate ID token ─────────────────────────────────────────────────
    // TokenResponse::id_token() is provided by the openidconnect crate's
    // trait impl on StandardTokenResponse<IdTokenFields<...>, ...>.
    // The verifier checks: signature (JWKS), audience, issuer, expiry, and nonce.
    let Some(id_token) = token_response.id_token() else {
        tracing::error!("OIDC callback: ID token missing from token response");
        return failure_redirect();
    };

    let id_token_verifier = client.client.id_token_verifier();
    let nonce = Nonce::new(entry.nonce);
    let claims = match id_token.claims(&id_token_verifier, &nonce) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %error_chain(&e), "OIDC callback: ID token validation failed");
            return failure_redirect();
        }
    };
    tracing::info!(
        sub = %claims.subject().as_str(),
        iss = %claims.issuer().as_str(),
        "OIDC callback: ID token validated"
    );

    // ── Extract email claim ───────────────────────────────────────────────
    // email_verified is intentionally NOT checked — the admin controls BookBoss
    // accounts.
    let Some(email_claim) = claims.email() else {
        tracing::error!(
            sub = %claims.subject().as_str(),
            "OIDC callback: ID token has no email claim — check IdP scope mapping (need `email` scope)"
        );
        return failure_redirect();
    };
    tracing::info!(email_claim = %email_claim.as_str(), "OIDC callback: email claim extracted");

    let email = match bb_core::types::EmailAddress::new(email_claim.as_str()) {
        Ok(e) => e,
        Err(e) => {
            tracing::error!(error = %e, email_claim = %email_claim.as_str(), "OIDC callback: email claim is malformed");
            return failure_redirect();
        }
    };

    // ── Look up BookBoss user (auto-provision if enabled) ─────────────────
    let user = match core_services.auth_service.is_valid_email(&email).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            if !auto_provision.0 {
                tracing::error!(
                    email = %email,
                    "OIDC callback: no BookBoss user with this email — check that a BookBoss user has this exact email_address (case-sensitive)"
                );
                return failure_redirect();
            }
            // Prefer IdP-provided identity (requires the `profile` scope); fall
            // back to the email local-part inside `provision_user`.
            let preferred_username = claims.preferred_username().map(|u| u.as_str().to_string());
            let full_name_hint = claims.name().and_then(|n| n.get(None)).map(|n| n.as_str().to_string());
            match provision_user(&core_services, &email, preferred_username, full_name_hint).await {
                Ok(u) => {
                    tracing::info!(
                        username = %u.username,
                        email = %email,
                        sub = %claims.subject().as_str(),
                        "OIDC callback: auto-provisioned new user"
                    );
                    u
                }
                Err(e) => {
                    tracing::error!(error = %e, email = %email, "OIDC callback: auto-provisioning failed");
                    return failure_redirect();
                }
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "OIDC callback: user lookup failed");
            return failure_redirect();
        }
    };

    // ── Create session ────────────────────────────────────────────────────
    auth_session.login_user(user.id);
    tracing::info!(username = %user.username, email = %email, "OIDC callback: login succeeded");
    Redirect::to("/").into_response()
}

fn failure_redirect() -> axum::response::Response {
    Redirect::to("/?login_failed=1").into_response()
}

/// Auto-provisions a BookBoss account for an OIDC login with no matching local
/// user, applying the defaults configured under Settings > Users.
///
/// `preferred_username` / `full_name_hint` come from the IdP (`profile` scope);
/// both fall back to the email local-part. The generated password is never used
/// (SSO accounts authenticate via the IdP) but satisfies the `users` schema.
async fn provision_user(
    core: &bb_core::CoreServices,
    email: &bb_core::types::EmailAddress,
    preferred_username: Option<String>,
    full_name_hint: Option<String>,
) -> Result<bb_core::user::User, bb_core::Error> {
    use bb_core::{library::LibraryToken, types::Capability, user::NewUser};

    let defaults = core.app_setting_service.oidc_provisioning_defaults().await?;

    // Enforce the documented invariant at the trust boundary: SuperAdmin is never
    // auto-provisioned, whatever the stored defaults happen to contain.
    let mut capabilities = defaults.capabilities.clone();
    capabilities.remove(&Capability::SuperAdmin);

    let local_part = email.as_str().split('@').next().unwrap_or_else(|| email.as_str()).to_string();
    let base_username = preferred_username
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| local_part.clone());
    let full_name = full_name_hint
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| base_username.clone());

    let username = unique_username(core, &base_username).await?;

    let new_user = NewUser::new(
        username,
        crate::password::make_password(),
        email.as_str(),
        capabilities,
        full_name,
        // SSO accounts never log in with a password, so never force a change
        // (which would otherwise trap the user with no password form to fill).
        false,
    )?;
    let user = core.user_service.add_user(new_user).await?;

    // Assign the configured libraries (best-effort). The user row is already
    // committed, so a failing assignment must not abort with `?`: doing so would
    // leave a half-provisioned account that the next login skips re-provisioning
    // for (is_valid_email now matches), stranding it permanently. A stale/deleted
    // but well-formed token would otherwise brick every future provisioning.
    for token_str in &defaults.library_tokens {
        if let Ok(token) = token_str.parse::<LibraryToken>() {
            if let Err(e) = core.library_service.assign_library_to_user(user.id, token).await {
                tracing::warn!(error = %e, token = %token_str, "OIDC provisioning: could not assign library; skipping");
            }
        } else {
            tracing::warn!(token = %token_str, "OIDC provisioning: skipping malformed library token");
        }
    }

    // Set the default library (best-effort — a bad default must not block an
    // otherwise-successful provisioning; it falls back to All Books).
    if let Some(default_str) = defaults.default_library.as_deref() {
        if let Ok(token) = default_str.parse::<LibraryToken>() {
            if let Err(e) = core.library_service.set_default_library(user.id, token).await {
                tracing::warn!(error = %e, "OIDC provisioning: could not set default library; falling back to All Books");
            }
        } else {
            tracing::warn!(token = %default_str, "OIDC provisioning: malformed default library token");
        }
    }

    Ok(user)
}

/// Finds a free username derived from `base`, appending `2`, `3`, … on
/// collision (usernames are matched case-insensitively).
async fn unique_username(core: &bb_core::CoreServices, base: &str) -> Result<String, bb_core::Error> {
    for suffix in 0..1000u32 {
        let candidate = if suffix == 0 { base.to_string() } else { format!("{base}{}", suffix + 1) };
        if core.user_service.find_by_username(&candidate).await?.is_none() {
            return Ok(candidate);
        }
    }
    Err(bb_core::Error::Validation(format!("could not derive a unique username from '{base}'")))
}
