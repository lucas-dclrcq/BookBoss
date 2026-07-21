use std::sync::Arc;

use axum::{
    Extension,
    extract::DefaultBodyLimit,
    http::{HeaderName, Request},
};
use axum_session::{SessionConfig, SessionLayer, SessionStore};
use axum_session_auth::{AuthConfig, AuthSessionLayer};
use bb_core::{CoreServices, user::UserId};
use chrono::Duration;
use dioxus::server::DioxusRouterExt;
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemHandle};
use tower::ServiceBuilder;
use tower_http::{
    compression::CompressionLayer,
    limit::RequestBodyLimitLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};

use crate::{BookBossFrontend, FrontendConfig, OidcConfig};

pub(crate) mod covers;
pub(crate) mod downloads;
pub(crate) mod events;
pub(crate) mod kobo;
pub(crate) mod koreader;
pub(crate) mod oidc;
pub(crate) mod opds;
pub(crate) mod session_pool;

pub(crate) use session_pool::{AuthSession, BackendSessionPool};

pub(crate) mod auth_user;

pub(crate) use auth_user::AuthUser;

const REQUEST_ID_HEADER: &str = "x-request-id";
const DEFAULT_EXPIRATION_DURATION: Duration = Duration::days(7);
const MAX_REQUEST_BODY_SIZE: usize = 70 * 1024 * 1024; // 70 MiB

/// Server-wide mirror of [`OidcConfig::auto_provision`], exposed as an axum
/// `Extension` so both the OIDC callback and the admin settings server fns can
/// read it — even when SSO is not otherwise configured (in which case it is
/// `false`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct AutoProvisionEnabled(pub bool);

pub struct FrontendSubsystem {
    config: FrontendConfig,
    oidc_config: Option<OidcConfig>,
    core_services: Arc<CoreServices>,
}

impl IntoSubsystem<anyhow::Error> for FrontendSubsystem {
    async fn run(self, subsys: &mut SubsystemHandle) -> Result<(), anyhow::Error> {
        tracing::info!("FrontendSubsystem starting...");

        let core_services = self.core_services.clone();
        let backend_pool = BackendSessionPool::new(core_services.clone());
        let session_config = SessionConfig::default().with_lifetime(DEFAULT_EXPIRATION_DURATION);
        let auth_config = AuthConfig::<UserId>::default();

        // Build the OIDC client at startup so discovery happens once, not per-request.
        // SSO is best-effort: any failure (partial config, unreachable IdP, malformed
        // discovery URL) leaves SSO disabled and logs the cause, but the server still
        // starts with password login working. is_sso_available() handles the partial-
        // config case (logs each missing field).
        let oidc_client: Option<Arc<oidc::OidcClient>> = match self.oidc_config.as_ref() {
            Some(cfg) if cfg.is_sso_available() => match oidc::OidcClient::new(cfg, &self.config.base_url).await {
                Ok(client) => {
                    tracing::info!("OIDC SSO enabled via {}", cfg.discovery_url.as_deref().unwrap_or("?"));
                    Some(Arc::new(client))
                }
                Err(e) => {
                    tracing::error!(error = %e, "OIDC SSO disabled — initialization failed; password login still available");
                    None
                }
            },
            _ => None,
        };

        let x_request_id = HeaderName::from_static(REQUEST_ID_HEADER);
        let session_store = SessionStore::<BackendSessionPool>::new(Some(backend_pool.clone()), session_config).await?;

        let middleware = ServiceBuilder::new()
            .layer(CompressionLayer::new())
            .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_SIZE))
            .layer(RequestBodyLimitLayer::new(MAX_REQUEST_BODY_SIZE))
            .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
            .layer(TraceLayer::new_for_http().make_span_with(|request: &Request<_>| {
                let request_id = request
                    .headers()
                    .get(REQUEST_ID_HEADER)
                    .map(|v| v.to_str().unwrap_or_default())
                    .unwrap_or_default();

                tracing::trace_span!(
                    "",
                    request_id = ?request_id,
                )
            }))
            .layer(PropagateRequestIdLayer::new(x_request_id))
            .layer(SessionLayer::new(session_store))
            .layer(AuthSessionLayer::<AuthUser, UserId, BackendSessionPool, BackendSessionPool>::new(Some(backend_pool)).with_config(auth_config));

        let frontend_config = Arc::new(self.config.clone());
        let kobo = kobo::kobo_router(&self.config.base_url, core_services.clone());
        let koreader = koreader::koreader_router();
        let opds = opds::opds_router();

        let mut app_router = axum::Router::new()
            .route("/api/v1/covers/{book_token}", axum::routing::get(covers::serve_cover))
            .route("/api/v1/books/{book_token}/download/{format}", axum::routing::get(downloads::serve_book_file))
            .route("/api/v1/events", axum::routing::get(events::event_stream))
            .serve_dioxus_application(dioxus_server::ServeConfig::new(), BookBossFrontend)
            .merge(kobo)
            .merge(koreader)
            .merge(opds);

        // When SSO is configured, merge the OIDC router and expose the client
        // and config to handlers / server fns. `oidc_client` is `Some` exactly
        // when `oidc_config.is_set()` was true above, so unwrapping the cloned
        // config here is safe by construction.
        if let Some(client) = oidc_client {
            let cfg = self.oidc_config.clone().expect("oidc_config is Some when oidc_client was built");
            app_router = app_router.merge(oidc::oidc_router()).layer(Extension(client)).layer(Extension(Arc::new(cfg)));
        }

        // Expose the auto-provision gate to every route (callback + admin server
        // fns). `false` when SSO is not configured.
        let auto_provision = AutoProvisionEnabled(self.oidc_config.as_ref().is_some_and(|c| c.auto_provision));

        let app_router = app_router
            .layer(Extension(core_services.health_service.clone()))
            .layer(Extension(core_services))
            .layer(Extension(frontend_config))
            .layer(Extension(auto_provision))
            .layer(middleware);

        let health_handler = || async { axum::http::StatusCode::OK };
        let router = axum::Router::new()
            .route("/healthz", axum::routing::get(health_handler))
            .route("/readyz", axum::routing::get(health_handler))
            .merge(app_router);

        let ip = std::env::var("IP").ok().unwrap_or_else(|| self.config.listen_ip.clone());
        let port: u16 = std::env::var("PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(self.config.listen_port);
        let listener = tokio::net::TcpListener::bind(&format!("{ip}:{port}")).await?;

        tracing::info!("Frontend listening on {}", listener.local_addr()?);

        tokio::select! {
            () = subsys.on_shutdown_requested() => {
                tracing::info!("Frontend shutting down...");
            }
            result = axum::serve(listener, router) => {
                if let Err(e) = result {
                    tracing::error!("Frontend server error: {}", e);
                }
                subsys.request_shutdown();
            }
        }

        Ok(())
    }
}

#[must_use]
pub fn create_frontend_subsystem(config: &FrontendConfig, oidc_config: Option<OidcConfig>, core_services: Arc<CoreServices>) -> FrontendSubsystem {
    FrontendSubsystem {
        config: config.to_owned(),
        oidc_config,
        core_services,
    }
}
