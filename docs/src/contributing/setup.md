# Development Setup

## Prerequisites

- Rust (nightly toolchain for formatting and clippy; `rust-version = "1.85"` edition 2024)
- [mise](https://mise.jdx.dev) — tool version manager
- [just](https://just.systems) — command runner
- [direnv](https://direnv.net/) — environment setup
- [sops](https://github.com/getsops/sops) — secrets encryption
- Node.js 24+ (managed by mise, used for Tailwind CSS)

## One-time Setup

Install and update all required tools:

```bash
just install-tools
```

This installs the nightly toolchain, wasm32 target, cargo extensions, and any other project tools.

## Secrets Configuration

```bash
just config
```

This opens the encrypted `config.sops.env` file for editing. See [Configuration Reference](../user/configuration.md) for all available settings.

## Building

```bash
just build
```

## Running

```bash
just run
```

The application will be available at `http://localhost:8080` by default.

## Integration Tests

Integration tests require Docker via [Colima](https://github.com/abiosoft/colima):

```bash
colima start
just test
colima stop
```

See [Commands](commands.md) for the full list of test commands.
