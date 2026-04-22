# Project Overview

[中文版本](./project-overview.zh-CN.md)

This document is the high-level map for understanding `ctx-cache-compressor` as a project, not just as a codebase.

## 1. Product Boundary

The core product is:

`session-scoped context cache + asynchronous history compression`

The service accepts messages over time, detects safe turn boundaries, compresses older turns in the background, and returns a merged context view that downstream applications can continue to use for inference.

Another useful shorthand is:

`Redis-like context cache + compressor`

The core product is not “chat completion”. Chat completion is only included through the demo helper route.

## 2. Project Layers

### Layer A: Core Compression Service

Responsible for:

- storing messages in memory
- validating role transitions
- detecting completed turns
- deciding when compression may run
- scheduling compression asynchronously
- returning the current merged context

Key modules:

- `src/session/`
- `src/compression/`
- `src/llm/`

### Layer B: Demo Runtime Layer

Responsible for:

- runtime config inspection and patching
- convenience chat flow for demos
- exposing a simpler playground-oriented surface

Key modules:

- `src/runtime.rs`
- `src/api/handlers/demo.rs`

### Layer C: Demo UIs

Responsible for:

- making the service observable
- showing context growth and compression behavior
- offering lightweight operator controls

Key files:

- `static/ctx-cache-compressor-playground.html`
- `static/dashboard.html`
- `static/playground-example.html`

## 3. The Most Important Internal Model

Each session is split into:

- `stable`: confirmed current context
- `pending`: messages appended while compression is in flight

The invariant is:

`full context = stable + pending`

That design allows:

- non-blocking `append`
- non-blocking `fetch`
- background compression without losing in-flight messages

## 4. Route Groups

### Core routes

- `POST /sessions`
- `GET /sessions`
- `DELETE /sessions/{session_id}`
- `POST /sessions/{session_id}/messages`
- `GET /sessions/{session_id}/context`
- `GET /health`

### Demo routes

- `GET /demo/config`
- `PATCH /demo/config`
- `POST /demo/chat`

### UI routes

- `/compressor`
- `/ex/dashboard`
- `/ex/playground`

## 5. Recommended Mental Model

If you are integrating `ctx-cache-compressor` into another app, think about it like this:

1. your application owns the product behavior and final model response
2. `ctx-cache-compressor` owns conversation state and compression

That means the cleanest production flow is:

1. append user message
2. fetch current context
3. call your own LLM with that context
4. append assistant message

## 6. Current Open-Source Readiness

The repository is already strong in these areas:

- working implementation
- strong integration coverage
- deploy and packaging helpers
- multiple demo pages
- operator-facing documentation

The main gaps to close before wider open-source adoption are:

- license selection and repository metadata
- API versioning guidance
- benchmark reporting
- example integrations from external apps

## 7. Documentation Guide

Start here:

- [README](../README.md)
- [API & Observability Map](./api-observability-map.md)

Use this document when you need the conceptual map of the whole project.
