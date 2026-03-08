# Architecture

BookBoss follows **hexagonal (ports & adapters)** architecture. Dependencies point inward toward the core domain. The `core` crate never depends on any outer crate.

## Crate Layout

```
crates/
├── api/            # Adapter: gRPC interface, calls into core ports
├── core/           # Domain layer: business logic, models, port traits
├── database/       # Adapter: persistence (SeaORM — Postgres, MySQL, SQLite)
├── formats/        # Adapter: e-book file format support (EPUB, OPF)
├── frontend/       # Adapter: Dioxus web UI (fullstack SSR + WASM)
├── import/         # Adapter: library scanner + import job handler
├── metadata/       # Adapter: external metadata providers
├── storage/        # Adapter: local filesystem library store
├── utils/          # Shared utilities (token encoding, etc.)
├── bookboss/       # Entry point: wires adapters to ports
└── integration-tests/
```

## Core Crate

The `core` crate uses domain-based modules. Each domain groups its model, repository trait (port), and service:

```
crates/core/src/
├── lib.rs              # CoreServices composition root, create_services()
├── error.rs            # Error, ErrorKind, RepositoryError
├── types.rs            # Shared newtypes (Email, Age)
├── repository.rs       # Repository, Transaction traits; RepositoryService; transaction macros
├── test_support.rs     # Mock implementations (behind "test-support" feature)
├── auth/               # Session auth: Session, AuthService, SessionRepository
├── book/               # Books, authors, series, publishers, genres, tags, files
├── device/             # Device sync: Device, DeviceBook, DeviceSyncLog
├── import/             # Acquisition pipeline: ImportJob, ImportJobService
├── jobs/               # Job queue: Job, JobRepository, JobWorker, JobRegistry, JobHandler
├── library/            # LibraryService (delete_book, library_stats)
├── pipeline/           # Port traits: MetadataExtractor, MetadataProvider; PipelineService
├── reading/            # Per-user reading state: UserBookMetadata, ReadStatus
├── shelf/              # Shelves (manual + smart): Shelf, ShelfFilter
├── storage/            # LibraryStore port trait + BookSidecar struct
└── user/               # Users and settings: User, UserService, UserSettingService
```

Each domain module typically contains:

- `mod.rs` — re-exports
- `model.rs` (or `model/`) — domain types (`Foo`, `NewFoo`, `FooId`, `FooToken`)
- `repository.rs` (or `repository/`) — `FooRepository` trait (port)
- `service.rs` — `FooService` trait + `FooServiceImpl`

## Metadata Providers

The `metadata` crate implements the `MetadataProvider` port from core. Providers are tried in order until one returns a result:

1. **Hardcover** — primary provider; returns metadata, cover, ratings, genres
2. **Open Library** — fallback for ISBN-based lookup; returns metadata and cover
3. **Google Books** — additional fallback; returns metadata

Each provider implements `MetadataProvider::enrich(extracted) -> Option<ProviderBook>`.

## Import Pipeline

The import subsystem (`crates/import/`) owns two background tasks:

- **LibraryScanner** — polls `BOOKBOSS__IMPORT__WATCH_DIRECTORY` on a timer, hashes new files, and enqueues `ImportJob` records
- **Import worker** (via `CoreSubsystem`/`JobWorker`) — processes `ImportJob` records through the `PipelineService`: extract metadata → enrich from providers → create book record → write sidecar → queue for review

## Subsystem Pattern

Each crate that owns background work exposes an `XxxSubsystem` struct and `create_xxx_subsystem()` factory. Subsystems are composed in `bookboss/main.rs` via `tokio-graceful-shutdown`:

```rust
Toplevel::new()
    .start(SubsystemBuilder::new("api", api_subsystem.run()))
    .start(SubsystemBuilder::new("import", import_subsystem.run()))
    .start(SubsystemBuilder::new("core", core_subsystem.run()))
    ...
```

## Adding a New Domain

1. Create a directory under `crates/core/src/` (e.g. `order/`)
2. Add `mod.rs`, `model.rs`, `repository.rs`, `service.rs`
3. Re-export from `mod.rs`
4. Register the module in `lib.rs`
5. Wire the new service into `CoreServices`

## Import Conventions

Use flat re-exports from domain modules:

```rust
use crate::user::{User, UserService, UserId};       // not user::model::User
use crate::session::{Session, NewSession};
use crate::repository::{Repository, Transaction};
use crate::types::{Email, Age};
```

Cross-domain references are allowed (e.g. `use crate::user::UserId` in a shelf model for foreign keys). Keep references one-directional where possible.
