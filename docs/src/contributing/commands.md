# Commands

All commands are run via `just`.

## Development

| Command              | Description                               |
| -------------------- | ----------------------------------------- |
| `just build`         | Build the project                         |
| `just run`           | Run the application (Dioxus dev server)   |
| `just fmt`           | Format code (nightly rustfmt + Prettier)  |
| `just clippy`        | Run Clippy lints (nightly)                |
| `just clean`         | Clean the workspace                       |
| `just deps`          | Update Rust crate dependencies            |
| `just tailwindcss`   | Regenerate Tailwind CSS                   |
| `just config`        | Edit encrypted configuration (sops)       |
| `just install-tools` | Install mise, nightly Rust, wasm32 target |
| `just bundle`        | Bundle web + server for release           |

## Testing

| Command                           | Description                                     |
| --------------------------------- | ----------------------------------------------- |
| `just component-tests`            | Component/unit tests only (nextest)             |
| `just quick-test`                 | Component + Postgres + SQLite integration tests |
| `just test`                       | All tests (all database backends)               |
| `just integration-tests`          | All integration tests (requires Colima)         |
| `just postgres-integration-tests` | Postgres integration tests                      |
| `just sqlite-integration-tests`   | SQLite integration tests                        |
| `just mysql-integration-tests`    | MySQL integration tests                         |
| `just mariadb-integration-tests`  | MariaDB integration tests                       |
| `just insta`                      | Run insta snapshot tests                        |
| `just insta-review`               | Review insta snapshot deltas                    |

## Database

| Command                | Description                           |
| ---------------------- | ------------------------------------- |
| `just database`        | Database admin                        |
| `just create-database` | Create the Postgres database and user |

## Documentation

| Command           | Description                          |
| ----------------- | ------------------------------------ |
| `just docs-serve` | Serve documentation locally (mdBook) |
| `just docs-build` | Build documentation                  |

## Release

| Command                | Description                         |
| ---------------------- | ----------------------------------- |
| `just changelog`       | Generate changelog from git history |
| `just release VERSION` | Create a release                    |
