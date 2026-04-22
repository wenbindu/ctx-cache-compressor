# ctx-cache-compressor

[中文说明](./README.zh-CN.md)

> A Redis-like context cache plus asynchronous history compression for long-running LLM conversations.

![ctx-cache-compressor playground](./docs/sample.png)

`ctx-cache-compressor` is a Rust service for storing session-scoped conversation state, returning a live merged context view, and compressing older turns in the background through an OpenAI-compatible model.

If you want the shortest mental model:

- your application owns the final assistant behavior
- `ctx-cache-compressor` owns context storage, caching, and history compression

That makes it useful as a lightweight building block between your chat application and your upstream LLM.

## Why This Exists

Long conversations usually fail in the same way:

- prompt size grows linearly with turn count
- cost and latency drift upward
- older but still-important context gets diluted

`ctx-cache-compressor` addresses that by treating conversation state as a first-class service:

- keep messages in memory per session
- append safely across `system`, `user`, `assistant`, and `tool` roles
- detect completed turn boundaries before compressing
- preserve the most recent turns verbatim
- summarize older history asynchronously
- always return a usable merged context view

## What This Project Is

`ctx-cache-compressor` is:

- an in-memory session store for LLM conversation context
- a turn-aware compression scheduler
- an OpenAI-compatible summary caller
- a context cache that can sit in front of your main model

`ctx-cache-compressor` is not:

- a full chat product
- a database-backed memory platform
- a vector database or RAG layer
- an agent runtime

The repository also includes demo routes and playground UIs, but the core product is the compression-and-cache service itself.

## Core Mental Model

Each session is split into two buffers:

- `stable`: the current confirmed context snapshot
- `pending`: messages appended while compression is in flight

Invariant:

`full context = stable + pending`

This gives the service its most important behavior:

- `append` does not wait for compression
- `fetch` does not wait for compression
- compression works on a snapshot
- failures degrade gracefully without losing messages

## Turn-Aware Compression

Compression is only allowed at safe boundaries:

- simple turn: `user -> assistant`
- tool turn: `user -> assistant(tool_calls) -> tool... -> assistant`

When the configured threshold is reached, the service compresses the older window into a summary message prefixed with `[CONTEXT SUMMARY]`, while keeping the newest turns uncompressed.

## Route Groups

### Core Service Routes

- `POST /sessions`
- `GET /sessions`
- `DELETE /sessions/{session_id}`
- `POST /sessions/{session_id}/messages`
- `GET /sessions/{session_id}/context`
- `GET /health`

### Demo Routes

- `GET /demo/config`
- `PATCH /demo/config`
- `POST /demo/chat`

`/demo/chat` is a convenience route for the playground. It is not the pure compression contract. It appends the user message, calls the upstream chat model, appends the assistant reply, and returns the updated context.

### UI Routes

- `/compressor`
- `/ex/dashboard`
- `/ex/playground`

The `/compressor` page defaults to English and includes a built-in `EN / 中文` toggle.

## Recommended Production Integration

For real applications, the clean integration pattern is:

1. create a session
2. append the user message
3. fetch the current context
4. call your own LLM with that context
5. append the assistant message
6. repeat

In other words:

- your app owns product behavior and final answers
- `ctx-cache-compressor` owns context state, caching, and compression

## Quick Start

### Requirements

- Rust stable
- `cargo`
- an OpenAI-compatible upstream endpoint
- an API key for that endpoint

### Local Environment Workflow

Create a local config and a local env file:

```bash
cp config.example.toml config.toml
cp .env.example .env.local
```

Edit `.env.local` and set `OPENAI_API_KEY` for your OpenAI-compatible upstream provider.

Then load it into the current shell:

```bash
source scripts/source_env.sh .env.local
```

That exports values such as:

- `OPENAI_API_KEY`
- `CTX_CACHE_COMPRESSOR_CONFIG_FILE`

### Local Run

With the config file and env file ready:

```bash
cargo run
```

If you prefer one-off shell exports instead of `.env.local`, this also works:

```bash
export OPENAI_API_KEY="your-api-key"
cargo run
```

Health check:

```bash
curl -sS http://127.0.0.1:8080/health | jq .
```

Open the playground:

```text
http://127.0.0.1:8080/compressor
```

### Run With An Explicit Config File

```bash
export OPENAI_API_KEY="your-api-key"
CTX_CACHE_COMPRESSOR_CONFIG_FILE=deploy/config/prod.toml cargo run --release
```

## Configuration

`config.toml` is intentionally ignored by Git so local secrets do not get committed. Start from `config.example.toml` and keep keys in environment variables.

Configuration load order:

1. built-in defaults
2. config file
   - `CTX_CACHE_COMPRESSOR_CONFIG_FILE` if set
   - otherwise repository `config.toml` when present
3. `CTX_CACHE_COMPRESSOR__...` environment overrides
4. canonical API key fallback when `llm.api_key` is empty

API key fallback order:

- `OPENAI_API_KEY`

Useful config files:

- `.env.example`
- `config.example.toml`
- `deploy/config/prod.toml`
- `deploy/config/prod-1000.toml`
- `deploy/systemd/ctx-cache-compressor.env.example`

## Repository Layout

```text
ctx-cache-compressor/
├── src/
│   ├── api/               # HTTP DTOs, handlers, routes
│   ├── compression/       # compression planning, prompt building, scheduler
│   ├── llm/               # OpenAI-compatible client
│   ├── session/           # session types, validation, turn logic, store
│   ├── config.rs          # config loading
│   ├── error.rs           # app error model
│   ├── lib.rs
│   └── main.rs
├── static/                # demo and playground pages
├── deploy/                # deploy configs and systemd templates
├── scripts/               # smoke, packaging, local helpers
├── tests/                 # integration tests
└── docs/                  # architecture and operator-facing docs
```

## API Usage

### Create A Session

```bash
curl -sS -X POST http://127.0.0.1:8080/sessions \
  -H 'content-type: application/json' \
  -d '{"system_prompt":"You are a concise assistant."}'
```

### Append A User Message

```bash
curl -sS -X POST http://127.0.0.1:8080/sessions/<session-id>/messages \
  -H 'content-type: application/json' \
  -d '{"role":"user","content":"Summarize what we decided so far."}'
```

### Append An Assistant Message

```bash
curl -sS -X POST http://127.0.0.1:8080/sessions/<session-id>/messages \
  -H 'content-type: application/json' \
  -d '{"role":"assistant","content":"Here is the current summary..."}'
```

### Fetch The Current Context

```bash
curl -sS http://127.0.0.1:8080/sessions/<session-id>/context | jq .
```

### List Active Sessions

```bash
curl -sS http://127.0.0.1:8080/sessions | jq .
```

### Delete A Session

```bash
curl -i -X DELETE http://127.0.0.1:8080/sessions/<session-id>
```

### Demo Chat Shortcut

This route is useful for the playground and quick manual checks:

```bash
curl -sS -X POST http://127.0.0.1:8080/demo/chat \
  -H 'content-type: application/json' \
  -d '{"user_message":"Explain what ctx-cache-compressor does."}' | jq .
```

## Deployment

Build a release package:

```bash
scripts/package_release.sh
```

Start a release binary with an explicit config:

```bash
cargo build --release
CONFIG_FILE=deploy/config/prod.toml scripts/run_release.sh
```

Start a dev instance in the background using `.env.local` automatically when present:

```bash
scripts/start_bg.sh
scripts/stop_bg.sh
```

Deploy with systemd:

1. Copy the packaged files or release build to `/opt/ctx-cache-compressor`
2. Copy `deploy/config/prod.toml` to `/etc/ctx-cache-compressor/prod.toml`
3. Copy `deploy/systemd/ctx-cache-compressor.env.example` to `/etc/ctx-cache-compressor/ctx-cache-compressor.env`
4. Fill in the provider API key in that env file
5. Install `deploy/systemd/ctx-cache-compressor.service`
6. Run `systemctl enable --now ctx-cache-compressor`

Build a Docker image:

```bash
docker build -t ctx-cache-compressor:local .
```

Run it with an env file:

```bash
docker run --rm -p 8080:8080 \
  --env-file .env.local \
  -v "$(pwd)/config.example.toml:/app/config.toml:ro" \
  ctx-cache-compressor:local
```

## Testing

Run the main suite:

```bash
cargo test
```

Project helper suites:

```bash
scripts/test_suite.sh quick
scripts/test_suite.sh load-1000
scripts/test_suite.sh all
```

Other useful checks:

- `scripts/smoke.sh`
- `scripts/load_test.sh`

Current automated coverage includes:

- role-sequence validation
- tool-call chain integrity
- turn-boundary detection
- successful compression
- graceful degradation on compression failure
- append/fetch during compression
- TTL cleanup
- concurrent session append scenarios

## Documentation

- [Project Overview](./docs/project-overview.md)
- [API & Observability Map](./docs/api-observability-map.md)
- [Chinese README](./README.zh-CN.md)
- [Chinese Project Overview](./docs/project-overview.zh-CN.md)

## Open-Source Notes

The repository is being prepared for public GitHub release. Current repo hygiene assumes:

- local secrets live in ignored files or environment variables
- generated replay/eval artifacts do not belong in Git history
- `AGENTS.md` is public and contributor-facing, not an internal workspace dump

One thing still requires an explicit maintainer decision before publishing:

- choose and add the final `LICENSE`
