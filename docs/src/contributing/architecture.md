# Architecture

BookBoss follows **hexagonal (ports & adapters)** architecture. Dependencies point inward toward the core domain. The `core` crate never depends on any outer crate.

## Crate Layout

```
crates/
├── api/            # Adapter: gRPC interface, calls into core ports
├── core/           # Domain layer: business logic, models, port traits
├── database/       # Adapter: persistence (SeaORM — Postgres, MySQL, MariaDB, SQLite)
├── formats/        # Adapter: e-book file formats (EPUB, OPF, KEPUB conversion)
├── frontend/       # Adapter: Dioxus web UI, OPDS catalog server, Kobo sync protocol
├── import/         # Adapter: library scanner + import job handler
├── metadata/       # Adapter: external metadata providers (Hardcover, OpenLibrary, GoogleBooks)
├── storage/        # Adapter: local filesystem library store
├── utils/          # Shared utilities (hashing, string similarity, token generation)
├── bookboss/       # Entry point: wires adapters to ports, loads configuration
└── integration-tests/
```

## Core Crate

The `core` crate uses domain-based modules. Each domain groups its model, repository trait (port), and service:

```
crates/core/src/
├── lib.rs              # CoreServices composition root, create_services()
├── error.rs            # Error, ErrorKind, RepositoryError
├── types.rs            # Shared newtypes (Email, Capability, etc.)
├── repository.rs       # Repository, Transaction traits; RepositoryService; transaction macros
├── test_support.rs     # Mock implementations (behind "test-support" feature)
├── auth/               # Session auth: Session, AuthService, SessionRepository
├── book/               # Books, authors, series, publishers, genres, tags, files
├── conversion/         # ConversionService port trait (EPUB enrichment, KEPUB conversion)
├── device/             # Device sync: Device, DeviceBook, DeviceSyncLog
├── event/              # EventService: broadcast channel for real-time UI updates (SSE)
├── filter/             # BookFilter, FilterCondition, operators (for smart shelves)
├── import/             # Acquisition pipeline: ImportJob, ImportJobService
├── jobs/               # Job queue: Job, JobRepository, JobWorker, JobRegistry, JobHandler
├── library/            # LibraryService (delete_book, library_stats), LibraryRepository
├── opds/               # OpdsService port (OPDS password management)
├── pipeline/           # Port traits: MetadataExtractor, MetadataProvider; PipelineService
├── reading/            # Per-user reading state: UserBookMetadata, ReadStatus
├── shelf/              # Shelves (manual + smart): Shelf, ShelfFilter, ShelfService
├── storage/            # LibraryStore port trait + BookSidecar struct
└── user/               # Users and settings: User, UserService, UserSettingService
```

Each domain module typically contains:

- `mod.rs` — re-exports
- `model.rs` (or `model/`) — domain types (`Foo`, `NewFoo`, `FooId`, `FooToken`)
- `repository.rs` (or `repository/`) — `FooRepository` trait (port)
- `service.rs` — `FooService` trait + `FooServiceImpl`

## Metadata Providers

The `metadata` crate implements the `MetadataProvider` port from core. Providers are queried in parallel with title+author similarity scoring to select the best match:

1. **Hardcover** — primary provider (GraphQL API); returns metadata, cover, ratings, genres
2. **Open Library** — ISBN-based and title search fallback; returns metadata and cover
3. **Google Books** — additional fallback; returns metadata and cover

Each provider implements `MetadataProvider::enrich(extracted) -> Option<ProviderBook>`.

## Import Pipeline

The import subsystem (`crates/import/`) owns the library scanner:

- **LibraryScanner** — polls `BOOKBOSS__IMPORT__BOOKDROP_PATH` on a timer (or via manual trigger), hashes new files, and enqueues `ImportJob` records

The import worker runs via the core job system (`CoreSubsystem`/`JobWorker`):

- Processes `ImportJob` records through the `PipelineService`: extract metadata → enrich from providers → create book record → write sidecar → queue for review

## Job System

The core job system provides a generic background task framework:

- **JobRegistry** — maps job types to `JobHandler` implementations
- **JobWorker** — polls for pending jobs and dispatches to handlers
- **JobHandler** — trait implemented by each handler (import pipeline, EPUB enrichment, KEPUB conversion)

Jobs are persisted in the database with status tracking (Pending, Processing, Completed, Failed). Recovery runs on startup to retry stalled jobs.

## Event System

The `EventService` broadcasts real-time events via a tokio broadcast channel:

- `IncomingChanged` — fired when imports reach NeedsReview, or are approved/rejected
- `JobsChanged` — fired when background jobs are queued, completed, or failed

The frontend exposes these as Server-Sent Events (SSE) at `GET /api/v1/events` for live UI updates.

## Subsystem Pattern

Each crate that owns background work exposes an `XxxSubsystem` struct and `create_xxx_subsystem()` factory. Subsystems are composed in `bookboss/main.rs` via `tokio-graceful-shutdown`:

```rust
Toplevel::new()
    .start(SubsystemBuilder::new("api", api_subsystem.run()))
    .start(SubsystemBuilder::new("import", import_subsystem.run()))
    .start(SubsystemBuilder::new("core", core_subsystem.run()))
    ...
```

Current subsystems: `ApiSubsystem` (gRPC), `CoreSubsystem` (job worker), `ImportSubsystem` (library scanner).

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
