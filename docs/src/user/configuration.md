# Configuration Reference

Configuration is loaded from environment variables with the prefix `BOOKBOSS` and `__` as the
separator (e.g. `BOOKBOSS__DATABASE__DATABASE_URL`).

Secrets are stored in an encrypted `config.sops.env` file managed by [sops](https://github.com/getsops/sops). Run `just config` to edit it.

## Database

| Variable | Description | Default |
| --- | --- | --- |
| `BOOKBOSS__DATABASE__DATABASE_URL` | Database connection string (**required**) | — |

See [Database Configuration](database.md) for connection string formats and examples.

## Encryption

| Variable | Description | Default |
| --- | --- | --- |
| `BOOKBOSS__ENCRYPTION_SECRET` | Encryption key for sensitive data (OPDS passwords) (**required**) | — |

## Frontend

| Variable | Description | Default |
| --- | --- | --- |
| `BOOKBOSS__FRONTEND__LISTEN_IP` | IP address the web server listens on | `0.0.0.0` |
| `BOOKBOSS__FRONTEND__LISTEN_PORT` | Port the web server listens on | `8080` |
| `BOOKBOSS__FRONTEND__BASE_URL` | Public-facing base URL (used for Kobo sync URLs) | `http://0.0.0.0:8080` |

## Library

| Variable | Description | Default |
| --- | --- | --- |
| `BOOKBOSS__LIBRARY__LIBRARY_PATH` | Path where approved book files are stored (**required**) | — |

## Import

| Variable | Description | Default |
| --- | --- | --- |
| `BOOKBOSS__IMPORT__BOOKDROP_PATH` | Directory to watch for new e-book files (**required**) | — |
| `BOOKBOSS__IMPORT__SCAN_INTERVAL_SECS` | How often (seconds) to scan the bookdrop directory | `60` |
| `BOOKBOSS__IMPORT__WORKER_POLL_INTERVAL_SECS` | How often (seconds) the import worker polls for jobs | `5` |

## Metadata Providers

| Variable | Description | Default |
| --- | --- | --- |
| `BOOKBOSS__METADATA__HARDCOVER_API_TOKEN` | API token for Hardcover (primary metadata provider) | — |
| `BOOKBOSS__METADATA__GOOGLEBOOKS_API_TOKEN` | API token for Google Books | — |

Open Library does not require an API token.

Providers are queried in parallel. The best match is selected by title+author similarity scoring.

## API (gRPC)

| Variable | Description | Default |
| --- | --- | --- |
| `BOOKBOSS__API__GRPC_LISTEN_IP` | IP address the gRPC server listens on | `0.0.0.0` |
| `BOOKBOSS__API__GRPC_LISTEN_PORT` | Port the gRPC server listens on | `8081` |

## Database Admin (just commands)

These variables are used by `just create-database` and `just database`, not by BookBoss itself:

| Variable | Used by |
| --- | --- |
| `PGUSER` | `just create-database`, `just database` |
| `PGPASSWORD` | `just create-database`, `just database` |
| `PGDATABASE` | `just create-database`, `just database` |
| `PGADMINUSER` | `just create-database` |
| `PGADMINPASSWORD` | `just create-database` |
