# Database Configuration

BookBoss supports four database backends. Choose the one that fits your deployment:

| Database   | Best for                                    |
| ---------- | ------------------------------------------- |
| SQLite     | Single-user, low maintenance, simple setups |
| PostgreSQL | Multi-user, production deployments          |
| MariaDB    | Existing MariaDB infrastructure             |
| MySQL      | Existing MySQL infrastructure               |

---

## SQLite

SQLite requires no separate server — the database is a single file on disk.

Set `BOOKBOSS__DATABASE__DATABASE_URL` to a file path:

```
sqlite:///path/to/bookboss.db
```

Or use a relative path:

```
sqlite://./bookboss.db
```

> **Tip:** SQLite is the simplest option for personal use. No additional software required.

---

## PostgreSQL

PostgreSQL is recommended for multi-user or production deployments.

### Prerequisites

A running PostgreSQL instance is required. You can run one with Docker:

```bash
docker run -d \
  --name bookboss-postgres \
  -e POSTGRES_USER=bookboss \
  -e POSTGRES_PASSWORD=yourpassword \
  -e POSTGRES_DB=bookboss \
  -p 5432:5432 \
  postgres:16
```

### Configuration

```
BOOKBOSS__DATABASE__DATABASE_URL=postgres://user:password@host:5432/database
```

---

## MySQL / MariaDB

### Prerequisites

A running MySQL or MariaDB instance is required. You can run one with Docker:

```bash
docker run -d \
  --name bookboss-mysql \
  -e MYSQL_USER=bookboss \
  -e MYSQL_PASSWORD=yourpassword \
  -e MYSQL_DATABASE=bookboss \
  -e MYSQL_ROOT_PASSWORD=rootpassword \
  -p 3306:3306 \
  mysql:8
```

### Configuration

Both MySQL and MariaDB use the same connection string format:

```
BOOKBOSS__DATABASE__DATABASE_URL=mysql://user:password@host:3306/database
```

---

## Migrations

BookBoss applies database migrations automatically on startup. No manual steps are required.
