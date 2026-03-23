# Installation

## Docker Compose (recommended)

The easiest way to run BookBoss is with Docker Compose. Pre-built compose files are provided
in the [`deploy/`](https://github.com/szinn/BookBoss/tree/main/deploy) directory for each
supported database backend.

### Quick Start

1. Create directories for your library and bookdrop:

   ```bash
   mkdir -p Library/Books Library/Bookdrop
   ```

2. Download the compose file for your preferred database:

   | Backend                      | File                           | Notes                                                  |
   | ---------------------------- | ------------------------------ | ------------------------------------------------------ |
   | **PostgreSQL** (recommended) | `docker-compose-postgres.yaml` | Best performance, recommended for production           |
   | **MySQL / MariaDB**          | `docker-compose-mysql.yaml`    | Alternative relational backend                         |
   | **SQLite**                   | `docker-compose-sqlite.yaml`   | Simplest setup, single container, no separate database |

   ```bash
   # Example: PostgreSQL
   curl -O https://raw.githubusercontent.com/szinn/BookBoss/main/deploy/docker-compose-postgres.yaml
   ```

3. Edit the compose file and change `BOOKBOSS__ENCRYPTION_SECRET` to a random string. Update
   `BOOKBOSS__FRONTEND__BASE_URL` if you are accessing BookBoss from a different hostname or
   behind a reverse proxy.

4. Start BookBoss:

   ```bash
   docker compose -f docker-compose-postgres.yaml up -d
   ```

5. Open <http://localhost:8080> and create your admin account.

### What's Next

- [Getting Started](getting-started.md) — first-run admin setup and orientation
- [Database Configuration](database.md) — details on each database backend
- [Configuration Reference](configuration.md) — all available environment variables

## Requirements

BookBoss needs:

- A supported database — [SQLite](database.md#sqlite), [PostgreSQL](database.md#postgresql), [MySQL](database.md#mysql--mariadb), or MariaDB
- A filesystem directory for the book library (`BOOKBOSS__LIBRARY__LIBRARY_PATH`)
- A filesystem directory for the bookdrop inbox (`BOOKBOSS__IMPORT__BOOKDROP_PATH`)

## Build from Source

See [Development Setup](../contributing/setup.md) for instructions on building and running
BookBoss from source.
