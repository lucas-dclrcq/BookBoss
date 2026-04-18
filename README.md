# BookBoss

**Take control of your digital library.**

BookBoss is a self-hosted digital library manager built in Rust. It provides a
web-based interface for organising and browsing your e-book collection, backed
by a flexible database layer that supports PostgreSQL, MySQL, MariaDB, and SQLite.

## Features

### Library

- Browse your book library with cover art, title, author, series, and publisher
- View book detail pages with full metadata (description, genres, tags, language, publication date, page count, identifiers)
- Browse all authors with a dedicated authors listing page
- View author detail pages listing all their books
- Browse all series with a visual series listing page (fanned cover art, book count)
- View series detail pages listing all books in a series
- Sort books by date added, title, and author
- Search bar with support for `author:`, `title:`, `series:`, `genre:`, `tag:` refinements
- Edit book metadata (title, author, series, publisher, genres, tags, description, identifiers, cover)
- Bulk edit metadata across multiple selected books
- Delete books from the library
- Download book files directly from the book detail page
- Keyboard shortcuts for common operations
- Real-time UI updates via Server-Sent Events (SSE)

### Import Pipeline

- Drop books into a watched folder — BookBoss scans and picks them up automatically
- Metadata extracted from EPUB files automatically
- Parallel metadata enrichment from external providers (Hardcover, OpenLibrary, GoogleBooks) with title+author similarity scoring
- Review incoming books before they enter the library (approve or reject)
- Cover art fetched automatically from the most confident provider match
- Manual scan trigger from the UI

### Shelves

- Create manual shelves and add/remove books via drag-and-drop
- Create smart shelves with filter rules (e.g. "all books with read status Active")
- Smart shelves update automatically as books and reading state change

### Reading State

- Track reading status per book (Unread, Reading, Paused, Rereading, Read, Abandoned)
- Record reading progress percentage, personal rating, times read, and notes per book
- Per-user — each user has their own reading state and progress
- Set reading status individually or in bulk across selected books

### Kobo Device Sync

- Register Kobo e-readers to your account
- Each device gets a companion smart shelf — books on the shelf sync to the device
- Incremental sync — only sends new or changed books each time
- Cover art served to the Kobo automatically
- EPUB and KEPUB format support with in-house EPUB-to-KEPUB conversion
- Reading position and progress synced between Kobo and BookBoss
- Reset sync state to force a full re-sync
- Copy device sync URL to clipboard from the profile page
- Kobo-initiated book removal handled with user-defined actions

### OPDS Catalog Server

- OPDS 1.x catalog server with Atom XML feeds
- Browse all books, by author, by series, or by shelf
- Search books via OpenSearch
- Per-user OPDS password (auto-generated, regeneratable from profile page)
- Compatible with any OPDS client (KOReader, Librera, Moon+ Reader, etc.)

### User & Admin

- User registration and login
- Admin first-run setup
- Multi-user support — each user has their own reading state, shelves, and devices
- Virtual libraries — each user can maintain their own subset of books independent of other users
- Capability-based permissions (Approve Imports, Edit Book, Delete Book, OPDS Access)
- Genre and tag management with per-entry book counts
- Library management (admin)
- User management (admin)
- System messages log for background task diagnostics (admin)
- Health task dashboard showing scheduled task status with manual trigger support (admin)
- Admin setting to also create MOBI-format files

## Getting Started

See the [full documentation](docs/src/) for detailed setup and usage guides.

### Quick Start

1. Install tools: `just install-tools`
2. Configure: `just config` (edit encrypted `config.sops.env`)
3. Create database: `just create-database`
4. Run: `just run`

The application will be available at `http://localhost:8080` by default. On
first launch you will be prompted to create an administrator account.

### Configuration

Configuration is loaded from environment variables with the `BOOKBOSS__` prefix:

| Variable                                    | Purpose                                               |
| ------------------------------------------- | ----------------------------------------------------- |
| `BOOKBOSS__DATABASE__DATABASE_URL`          | SeaORM connection string (Postgres / MySQL / SQLite)  |
| `BOOKBOSS__ENCRYPTION_SECRET`               | Used for encrypting OPDS passwords in the database    |
| `BOOKBOSS__LIBRARY__LIBRARY_PATH`           | Where approved book files are stored                  |
| `BOOKBOSS__IMPORT__BOOKDROP_PATH`           | Drop e-book files here to trigger the import pipeline |
| `BOOKBOSS__FRONTEND__LISTEN_IP`             | Server listen address (default `0.0.0.0`)             |
| `BOOKBOSS__FRONTEND__LISTEN_PORT`           | Server listen port (default `8080`)                   |
| `BOOKBOSS__FRONTEND__BASE_URL`              | Public base URL (default `http://0.0.0.0:8080`)       |
| `BOOKBOSS__METADATA__HARDCOVER_API_TOKEN`   | API token for Hardcover metadata provider             |
| `BOOKBOSS__METADATA__GOOGLEBOOKS_API_TOKEN` | API token for Google Books metadata provider          |

Connection string formats:

```
postgres://user:password@host:port/database
mysql://user:password@host:port/database
sqlite:path/to/file.db
```

> Secrets are encrypted with [sops](https://github.com/getsops/sops) — never
> commit plaintext secrets.

## Development

### Requirements

- [Rust](https://rustup.rs) 1.85+ (nightly toolchain for formatting/clippy)
- [mise](https://mise.jdx.dev) — manages tool versions
- [just](https://just.systems) — task runner
- Node.js 24+ (for Tailwind CSS, managed by mise)
- An existing PostgreSQL or MySQL instance (for those database backends)

### Common Commands

| Command                  | Description                                         |
| ------------------------ | --------------------------------------------------- |
| `just build`             | Build the project                                   |
| `just run`               | Run the application                                 |
| `just fmt`               | Format code (Rust + Prettier)                       |
| `just clippy`            | Run Clippy lints                                    |
| `just quick-test`        | Component tests + Postgres/SQLite integration tests |
| `just test`              | Run all tests                                       |
| `just component-tests`   | Unit/component tests only                           |
| `just integration-tests` | All integration tests (requires Colima)             |
| `just insta`             | Run snapshot tests with cargo-insta                 |
| `just docs-serve`        | Serve documentation locally                         |
| `just deps`              | Update Rust crate dependencies                      |
| `just changelog`         | Regenerate CHANGELOG.md                             |

### Integration Tests

Integration tests use Docker containers managed by [Colima](https://github.com/abiosoft/colima):

```bash
colima start
just integration-tests
colima stop
```

## Architecture

BookBoss follows **hexagonal (ports & adapters)** architecture. All dependencies
point inward toward the core domain — the core crate has no knowledge of
adapters.

```
crates/
├── core/             # Domain: business logic, models, port traits
├── api/              # Adapter: gRPC interface
├── database/         # Adapter: SeaORM persistence (Postgres / MySQL / SQLite)
├── formats/          # Adapter: e-book formats (EPUB, OPF, KEPUB conversion)
├── frontend/         # Adapter: Dioxus web UI, OPDS server, Kobo sync
├── import/           # Adapter: library scanner + import job handler
├── metadata/         # Adapter: external metadata providers (Hardcover, OpenLibrary, GoogleBooks)
├── storage/          # Adapter: local filesystem library store
├── utils/            # Shared utilities
├── bookboss/         # Binary: wires adapters to ports
└── integration-tests/
```

See the [full documentation](docs/src/) for architecture details and contributor
guides.

## License

MIT
