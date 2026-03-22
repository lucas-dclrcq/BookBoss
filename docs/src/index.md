# BookBoss

**Take Control Of Your Digital Library**

BookBoss is a self-hosted application for managing your e-book collection. It provides a web-based
interface for organising, browsing, and syncing your library across devices — backed by PostgreSQL,
MySQL, MariaDB, or SQLite.

---

## For Users

New to BookBoss? Start here:

- [Installation](user/installation.md) — set up BookBoss on your server or machine
- [Getting Started](user/getting-started.md) — first run, admin account setup
- [Database Configuration](user/database.md) — choose and configure your database backend
- [Managing Your Library](user/managing-library.md) — importing books, browsing, editing metadata
- [Shelves & Reading State](user/shelves.md) — organise books into shelves, track reading progress
- [OPDS Catalog](user/opds.md) — browse your library from any OPDS-compatible reader app
- [Kobo Device Sync](user/kobo.md) — sync books and reading progress with Kobo e-readers
- [Configuration Reference](user/configuration.md) — all available configuration options

## For Contributors

Looking to contribute or run from source?

- [Architecture](contributing/architecture.md) — hexagonal design, crate layout, domain modules
- [Development Setup](contributing/setup.md) — tools, environment, first build
- [Commands](contributing/commands.md) — all `just` commands
- [Conventions](contributing/conventions.md) — commits, error handling, testing, secrets
- [Frontend (Dioxus)](contributing/frontend.md) — server functions, routing, auth, SSE events
- [Database Internals](contributing/database.md) — SeaORM, migrations, entity generation
