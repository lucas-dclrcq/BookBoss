# Frontend (Dioxus)

The frontend is built with [Dioxus 0.7](https://dioxuslabs.com/learn/0.7) in fullstack mode — server-side rendering with client-side hydration, using axum as the server.

> **Warning:** Dioxus 0.7 is a major API break from earlier versions. `cx`, `Scope`, and `use_state` are gone.
> Only use 0.7 documentation and patterns.

## Key Patterns

### Server Functions

Use `#[get]` or `#[post]` to define server functions. The macro takes the endpoint path followed by
any axum extensions the function needs, declared as `name: axum::Extension<Type>`. These are
injected server-side and are not part of the function's parameter list.

```rust
#[get("/api/v1/check_auth", auth_session: axum::Extension<AuthSession>)]
async fn check_auth() -> Result<bool, ServerFnError> {
    Ok(auth_session.current_user.as_ref().map(|u| !u.username.is_empty()).unwrap_or(false))
}
```

Function parameters (the request arguments) are declared normally on the function:

```rust
#[post("/api/v1/login", core_services: axum::Extension<Arc<CoreServices>>, auth_session: axum::Extension<AuthSession>)]
async fn perform_login(username: String, password: String) -> Result<(), ServerFnError> {
    // username and password come from the caller
    // core_services and auth_session are injected by axum
}
```

Use `#[tracing::instrument]` to add tracing — always `skip` the injected extensions:

```rust
#[get("/api/v1/get_landing_state", core_services: axum::Extension<Arc<CoreServices>>, auth_session: axum::Extension<AuthSession>)]
#[tracing::instrument(level = "trace", skip(core_services, auth_session))]
async fn get_landing_state() -> Result<LandingState, ServerFnError> { ... }
```

The server-side imports (`AuthSession`, `CoreServices`, etc.) are gated behind the `server` feature:

```rust
#[cfg(feature = "server")]
use {crate::server::AuthSession, bb_core::CoreServices, std::sync::Arc};
```

### Axum Handlers (non-server-fn routes)

Some endpoints need full axum handler control — for example, file downloads, image serving,
OPDS feeds, and the Kobo sync protocol. These live in `crates/frontend/src/server/` and are
registered manually in `server/mod.rs`.

Follow the pattern in `covers.rs` / `downloads.rs`: check auth via `auth_session.current_user`,
return `Response` directly, use `Body::from(data)` with appropriate `Content-Type` and
`Cache-Control` headers.

### Auth / Session

- `AuthSession` is stored in request extensions by `AuthSessionLayer`
- Check `!user.username.is_empty()` to determine if the user is authenticated (anonymous users have empty usernames)
- `auth_session.login_user(user_id)` logs in a user
- Capability checks use `Auth::build([Method::POST], true).requires(...).validate(...)` — do not use `.permissions.contains()` directly, as it misses transitive grants from Admin/SuperAdmin

### Routing

Routes are defined as a `Routable` enum. `LandingPage` lives outside `AppLayout` (no NavBar):

```rust
#[derive(Routable, Clone, PartialEq)]
enum Route {
    #[route("/")]
    LandingPage {},         // no layout — appears before #[layout(...)]

    #[layout(AppLayout)]
    #[route("/library")]
    BooksPage {},
    #[route("/library/books/:token")]
    BookDetailPage { token: String },
    #[route("/library/books/:token/edit")]
    EditMetadataPage { token: String },
    #[route("/library/authors/:token")]
    AuthorDetailPage { token: String },
    #[route("/library/series/:token")]
    SeriesDetailPage { token: String },
    #[route("/library/incoming")]
    IncomingPage {},
    #[route("/library/incoming/:token")]
    ReviewPage { token: String },
    #[route("/shelves/:token")]
    ShelfPage { token: String },
    #[route("/settings")]
    SettingsPage {},
    #[route("/profile")]
    ProfilePage {},
}
```

Navigate programmatically after a server fn succeeds:

```rust
let navigator = use_navigator();
navigator.push(Route::BooksPage {});
```

### Hydration

Use `use_server_future` (not `use_resource`) for data that must be available on first render:

```rust
let data = use_server_future(fetch_data)?;
```

Browser-specific code (e.g. `localStorage`) must go inside `use_effect`, which runs only after hydration.

## Server Modules

```
crates/frontend/src/server/
├── mod.rs                       # Server setup, router, middleware stack
├── auth_user.rs                 # AuthUser impl for axum-session-auth
├── session_pool.rs              # BackendSessionPool (session store)
├── covers.rs                    # GET /api/v1/covers/{token} — serve cover images
├── downloads.rs                 # GET /api/v1/books/{token}/download/{format}
├── events.rs                    # GET /api/v1/events — SSE event stream
├── opds/                        # OPDS 1.x catalog server (Atom XML feeds)
│   ├── mod.rs                   # Router, auth extractor
│   ├── feeds.rs                 # All OPDS feed endpoints
│   ├── xml.rs                   # Atom XML builder
│   └── ...
└── kobo/                        # Kobo device sync protocol
    ├── mod.rs                   # Router, auth extractor
    ├── initialization.rs        # Device init + store API proxy
    ├── library_sync.rs          # Incremental library sync
    ├── metadata.rs              # Per-book metadata
    ├── state.rs                 # Reading state GET/PUT
    └── ...
```

## Frontend Structure

```
crates/frontend/src/
├── lib.rs                           # Route enum, AppLayout, root component
├── settings.rs                      # FrontendConfig
├── error.rs                         # Error types
│
├── routes/
│   ├── landing_page.rs              # Login, register admin
│   ├── books_page.rs                # Library grid with search, sort, bulk ops
│   ├── book_detail_page.rs          # Book detail + download + delete
│   ├── edit_metadata_page.rs        # Metadata editor
│   ├── author_detail_page.rs        # Author detail + books list
│   ├── series_detail_page.rs        # Series detail + books list
│   ├── shelf_page.rs                # Shelf contents view
│   ├── incoming_page.rs             # Import review queue
│   ├── settings_page.rs             # Settings, user management
│   ├── profile_page.rs             # User profile, OPDS, Kobo devices
│   └── review_page/
│       ├── mod.rs                   # ReviewPage component
│       ├── editor.rs                # Side-by-side metadata editor
│       ├── server.rs                # Server functions
│       └── types.rs                 # ReviewBook, ReviewField, etc.
│
├── components/
│   ├── app_layout.rs                # AppLayout wrapper (NavBar + outlet)
│   ├── nav_bar.rs                   # Top navigation bar with search
│   ├── book_grid.rs                 # Cover thumbnail grid (DnD, multi-select)
│   ├── shelf_bar.rs                 # Horizontal shelf pills (DnD drop targets)
│   ├── autocomplete_input.rs        # Typeahead input for authors, series, etc.
│   ├── chip_input.rs                # Tag/genre chip input
│   ├── login_form.rs                # Login form
│   └── register_admin_form.rs       # Admin registration form
│
└── server/                          # (see Server Modules above)
```

## SSE Events

BookBoss uses Server-Sent Events for real-time UI updates. The `GET /api/v1/events` endpoint
streams `AppEvent` variants:

- `IncomingChanged` — new imports ready for review, or imports approved/rejected
- `JobsChanged` — background job status changes

The frontend subscribes to this stream and updates relevant UI components automatically (e.g. the
incoming badge count refreshes without a page reload).
