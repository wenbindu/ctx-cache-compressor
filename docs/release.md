# Release Guide

This project can be published as platform-specific binary archives. Each binary
is self-contained enough to run directly with a TOML config file or environment
variables; no separate static asset directory is required because the demo HTML
is compiled into the binary.

## Binaries

The crate builds three entrypoints:

- `ctx-cache-compressor-api`: production API-only service
- `ctx-cache-compressor-demo`: local demo/display service
- `ctx-cache-compressor`: compatibility service, core API plus optional demo/UI

For production, publish and run `ctx-cache-compressor-api` unless you explicitly
want the built-in demo routes on the same process.

## Build One Binary

Build the production API service for the current platform:

```bash
cargo build --release --locked --bin ctx-cache-compressor-api
```

Run it with a config file:

```bash
OPENAI_API_KEY=sk-... \
CTX_CACHE_COMPRESSOR_CONFIG_FILE=deploy/config/prod.toml \
target/release/ctx-cache-compressor-api
```

You can also configure it entirely with environment variables:

```bash
OPENAI_API_KEY=sk-... \
CTX_CACHE_COMPRESSOR__SERVER__PORT=8080 \
CTX_CACHE_COMPRESSOR__SERVER__PERMISSIVE_CORS=false \
CTX_CACHE_COMPRESSOR__LLM__BASE_URL=https://api.openai.com/v1 \
CTX_CACHE_COMPRESSOR__LLM__MODEL=gpt-4.1-mini \
target/release/ctx-cache-compressor-api
```

Config precedence is:

1. built-in defaults
2. `config.toml` or `CTX_CACHE_COMPRESSOR_CONFIG_FILE`
3. `CTX_COMPRESSOR__...` and `CTX_CACHE_COMPRESSOR__...` environment variables
4. `OPENAI_API_KEY` when `[llm].api_key` is empty

## Package Archives

Build a release archive for the current host target:

```bash
scripts/package_release.sh
```

Build for an explicit target:

```bash
TARGET=x86_64-unknown-linux-gnu scripts/package_release.sh
```

By default the package includes all three binaries. To package only the
production API binary:

```bash
BINARIES=ctx-cache-compressor-api scripts/package_release.sh
```

The output is written to `dist/`:

```text
ctx-cache-compressor-0.1.0-<target>.tar.gz
ctx-cache-compressor-0.1.0-<target>.tar.gz.sha256
```

Each archive contains:

- `bin/` release binaries
- `config/prod.toml`
- `config/prod-1000.toml`
- `config/config.example.toml`
- `deploy/systemd/`
- `scripts/run_release.sh`
- `scripts/smoke.sh`
- `MANIFEST.txt`

Run from an extracted package:

```bash
OPENAI_API_KEY=sk-... scripts/run_release.sh
```

`scripts/run_release.sh` defaults to `SERVICE=api`. Other modes:

```bash
SERVICE=demo scripts/run_release.sh
SERVICE=compat scripts/run_release.sh
```

## Platform Targets

GitHub Actions publishes these targets on tag pushes:

- `x86_64-unknown-linux-gnu`
- `x86_64-unknown-linux-musl`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
- `x86_64-pc-windows-msvc`

Different operating systems and CPU architectures require different binaries.
The archive name includes the Rust target triple so operators can choose the
right artifact.

For Linux, `x86_64-unknown-linux-musl` is the closest option to a Go-style
single static binary. The `x86_64-unknown-linux-gnu` build is still portable for
typical glibc-based Linux servers, but it dynamically links glibc. Both builds
avoid OpenSSL runtime dependencies because the HTTP client uses Rustls.

## Publish Flow

Before tagging:

```bash
cargo fmt
cargo test
cargo clippy --all-targets --all-features -- -D warnings
scripts/package_release.sh
```

Then create and push a tag:

```bash
git tag v0.1.0
git push origin v0.1.0
```

The release workflow builds the platform archives and uploads them to the GitHub
Release.

## Public Release Checklist

- Confirm `Cargo.toml` version matches the tag.
- Confirm `config.example.toml` and `deploy/config/prod.toml` contain no secrets.
- Confirm a final `LICENSE` file has been chosen and added before public release.
- Confirm README screenshots and route descriptions match the current UI.
- Confirm `ctx-cache-compressor-api` is the documented production entrypoint.
