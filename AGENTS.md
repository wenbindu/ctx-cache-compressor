# AGENTS.md

Public agent and automation guide for `ctx-cache-compressor`.

This file is intentionally repository-facing and tool-agnostic. It should help automated contributors and human maintainers work safely in this codebase without depending on private local workflows.

## Project Summary

`ctx-cache-compressor` is a Rust service for:

- storing conversation messages per session
- returning a merged live context view
- compressing older turns asynchronously through an OpenAI-compatible model

The product boundary is best understood as:

`context cache + turn-aware compressor`

Think of it as a Redis-like session cache for LLM context, with a built-in background summarizer.

## Architecture Invariants

Preserve these invariants unless the change explicitly redesigns the system:

1. `full context = stable + pending`
2. `append` must not block on compression
3. `fetch` must not block on compression
4. compression may run only at completed turn boundaries
5. compression failure must degrade gracefully without losing messages
6. demo routes may be convenience layers, but they should not redefine the core service contract

## Key Areas

- `src/session/`
  - session model
  - role validation
  - turn-boundary logic
  - session store and TTL cleanup
- `src/compression/`
  - compression planning
  - prompt construction
  - scheduler
  - message replacement logic
- `src/api/`
  - HTTP DTOs
  - handlers
  - routes
- `static/`
  - `/compressor`
  - `/ex/dashboard`
  - `/ex/playground`

## Configuration Rules

- Do not commit real API keys.
- Keep local secrets in `config.toml`, `config.local.toml`, env files, or environment variables, and keep those files ignored.
- Use `config.example.toml` as the public copyable baseline.
- Prefer environment variables for provider credentials:
  - `OPENAI_API_KEY`

## Editing Guidance

- Keep API shape and README/docs aligned.
- If a route changes, update:
  - `README.md`
  - `README.zh-CN.md`
  - relevant docs under `docs/`
  - integration tests when needed
- If a change affects the playgrounds, remember that HTML files under `static/` are compiled into the binary through `include_str!`.
- If you change `static/` content, rebuild or rerun the service before validating the page.

## Validation

Run these after meaningful Rust or route changes:

```bash
cargo fmt
cargo test
```

Useful supporting checks:

```bash
scripts/smoke.sh
scripts/test_suite.sh quick
```

## Documentation Expectations

For public-facing changes, prefer:

- English-first top-level docs
- Chinese mirrors where already present
- relative repository links, not machine-local absolute paths
- concise explanations of route purpose, config loading, and operator flow

## Open-Source Hygiene

Before publishing or tagging a release:

1. verify no secrets are present in tracked files
2. verify ignored local artifacts stay out of Git
3. ensure the screenshot and README reflect the current UI
4. ensure Docker and release packaging still include required assets
5. confirm a final `LICENSE` file is present
