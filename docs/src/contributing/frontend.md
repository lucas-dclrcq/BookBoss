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

Some endpoints need full axum handler control — for example, file downloads and image serving.
These live in `crates/frontend/src/server/` and are registered manually in `server/mod.rs`:

```rust
let router = dioxus::server::router(BookBossFrontend)
    .route("/api/v1/covers/{book_token}", axum::routing::get(covers::serve_cover))
    .route("/api/v1/books/{book_token}/download/{format}", axum::routing::get(downloads::serve_book_file))
    .layer(Extension(core_services))
    .layer(middleware);
```

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
    LibraryPage {},
}
```

Navigate programmatically after a server fn succeeds:

```rust
let navigator = use_navigator();
navigator.push(Route::LibraryPage {});
```

### Hydration

Use `use_server_future` (not `use_resource`) for data that must be available on first render:

```rust
let data = use_server_future(fetch_data)?;
```

Browser-specific code (e.g. `localStorage`) must go inside `use_effect`, which runs only after hydration.

## Frontend Structure

```
crates/frontend/src/
├── lib.rs                           # Route enum, AppLayout, root component
├── settings.rs                      # FrontendConfig
├── error.rs                         # Error types
│
├── routes/
│   ├── landing_page.rs              # Login, register admin — server fns
│   ├── books_page.rs                # Library grid/table with ShelfBar + TreeExplorer
│   ├── book_detail_page.rs          # Book detail view + download + delete
│   ├── edit_metadata_page.rs        # Metadata editor (title, authors, cover, etc.)
│   ├── author_detail_page.rs        # Author detail + books list
│   ├── series_detail_page.rs        # Series detail + books list
│   ├── shelf_page.rs                # Shelf contents view
│   ├── incoming_page.rs             # Import review queue
│   ├── settings_page.rs             # Settings (library stats, shelves)
│   └── review_page/
│       ├── mod.rs                   # ReviewPage component
│       ├── editor.rs                # Side-by-side metadata editor
│       ├── server.rs                # Server functions (get_review, approve, reject)
│       └── types.rs                 # ReviewBook, ReviewField, etc.
│
├── components/
│   ├── app_layout.rs                # AppLayout wrapper (NavBar + outlet)
│   ├── nav_bar.rs                   # Top navigation bar
│   ├── book_grid.rs                 # Cover thumbnail grid (with DnD drag source)
│   ├── book_table.rs                # Tabular book listing (with DnD drag source)
│   ├── shelf_bar.rs                 # Horizontal shelf pills (DnD drop targets)
│   ├── tree_explorer.rs             # Sidebar tree (Authors, Series, Shelves, etc.)
│   ├── autocomplete_input.rs        # Typeahead input for authors, series, etc.
│   ├── chip_input.rs                # Tag/genre chip input
│   ├── login_form.rs                # Login form
│   └── register_admin_form.rs       # Admin registration form
│
└── server/
    ├── mod.rs                       # Server setup, router, middleware
    ├── auth_user.rs                 # AuthUser impl for axum-session-auth
    ├── session_pool.rs              # BackendSessionPool (session store)
    ├── covers.rs                    # GET /api/v1/covers/{token} — serve cover images
    └── downloads.rs                 # GET /api/v1/books/{token}/download/{format} — serve book files
```
