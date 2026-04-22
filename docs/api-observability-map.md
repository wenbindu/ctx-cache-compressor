# ctx-cache-compressor API & Observability Map

This document summarizes the current service API surface, the internal call paths behind each endpoint, and the information worth tracking in the UI for a `ctx-cache-compressor` playground or dashboard.

It is intentionally split into:

1. What the service already exposes today
2. What is worth adding next for a stronger operator UI

## 1. Current Route Map

The router currently exposes the following routes:

| Route | Method | Purpose |
| --- | --- | --- |
| `/` | `GET` | Dashboard page |
| `/ex/dashboard` | `GET` | Dashboard page |
| `/ex/playground` | `GET` | LiveKit-inspired reference page |
| `/compressor` | `GET` | Compression-focused playground page |
| `/health` | `GET` | Service health + session count + version |
| `/sessions` | `GET` | List active sessions |
| `/demo/config` | `GET` | Frontend-facing runtime/demo configuration |
| `/demo/config` | `PATCH` | Update frontend-facing runtime/demo configuration |
| `/demo/chat` | `POST` | Single-call demo chat flow: append user, run model, append assistant, return full context |
| `/sessions` | `POST` | Create a session |
| `/sessions/{session_id}` | `DELETE` | Delete a session |
| `/sessions/{session_id}/messages` | `POST` | Append a single message into a session |
| `/sessions/{session_id}/context` | `GET` | Fetch the merged context view |

Reference: [src/api/routes.rs](../src/api/routes.rs#L21)

## 2. Current Frontend Call Map

The current playground and dashboard do not call all session APIs directly for chat. They follow a lightweight operator loop:

1. On load:
   - `GET /health`
   - `GET /demo/config`
2. When creating a session manually:
   - `POST /sessions`
3. When deleting a session:
   - `DELETE /sessions/{id}`
4. When refreshing state:
   - `GET /sessions/{id}/context`
5. When sending a chat message:
   - `POST /demo/chat`

The `/compressor` page specifically performs those calls in browser code here:

- [static/ctx-cache-compressor-playground.html](../static/ctx-cache-compressor-playground.html#L1525)
- [static/ctx-cache-compressor-playground.html](../static/ctx-cache-compressor-playground.html#L1535)
- [static/ctx-cache-compressor-playground.html](../static/ctx-cache-compressor-playground.html#L1546)
- [static/ctx-cache-compressor-playground.html](../static/ctx-cache-compressor-playground.html#L1561)
- [static/ctx-cache-compressor-playground.html](../static/ctx-cache-compressor-playground.html#L1573)
- [static/ctx-cache-compressor-playground.html](../static/ctx-cache-compressor-playground.html#L1593)

## 3. Data Structures Exposed Today

### 3.1 Session / Message Model

The service currently exposes:

- `Role`: `system`, `user`, `assistant`, `tool`
- `MessageContent`: plain text or `parts`
- `ToolCall`, `ToolFunction`
- `Message`

Reference: [src/session/types.rs](../src/session/types.rs#L9)

Important semantic helpers already in the model:

- assistant message with no `tool_calls` is treated as a final assistant message
- system summaries are normal system messages prefixed with `[CONTEXT SUMMARY]`
- token estimate is based on message character length, including tool metadata

Reference: [src/session/types.rs](../src/session/types.rs#L60)

### 3.2 Trace Model

The service already records a bounded in-memory trace stream per session, with a maximum of 200 events.

Trace kinds currently available:

- `session_created`
- `system_message_appended`
- `user_message_appended`
- `assistant_message_appended`
- `tool_message_appended`
- `compression_triggered`
- `compression_succeeded`
- `compression_failed`
- `demo_chat_started`
- `demo_chat_completed`
- `demo_chat_failed`

Each trace event includes:

- `at`
- `kind`
- `message`
- `turn_count`
- `stable_count`
- `pending_count`

Reference: [src/session/types.rs](../src/session/types.rs#L129)

### 3.3 API DTOs

Current DTO coverage:

- `CreateSessionRequest` / `CreateSessionResponse`
- `AppendMessageRequest` / `AppendMessageResponse`
- `FetchContextResponse`
- `HealthResponse`
- `DemoConfigResponse`
- `DemoChatRequest` / `DemoChatResponse`

Reference: [src/api/dto.rs](../src/api/dto.rs#L6)

## 4. Per-Endpoint Call Paths

### 4.1 `GET /health`

Purpose:

- cheap service heartbeat
- global session count
- binary version

Current response:

- `status`
- `sessions`
- `version`

Reference: [src/api/handlers/health.rs](../src/api/handlers/health.rs#L6)

Worth showing in UI:

- service status badge
- active session count
- running version

### 4.2 `GET /demo/config`

Purpose:

- give the frontend runtime knobs and defaults without forcing it to duplicate config

Current response:

- `llm_model`
- `llm_base_url`
- `compression_every_n_turns`
- `keep_recent_turns`
- `llm_timeout_seconds`
- `max_retries`
- `session_ttl_seconds`
- `max_sessions`
- `default_system_prompt`
- `recommended_poll_interval_ms`

Reference: [src/api/dto.rs](../src/api/dto.rs#L74)
Reference: [src/api/handlers/demo.rs](../src/api/handlers/demo.rs#L19)

Worth showing in UI:

- model identity
- compression policy
- timeout / retry policy
- session lifetime
- default prompt currently driving demo chat

### 4.3 `POST /sessions`

Purpose:

- create a fresh session with an optional system prompt

Current behavior:

- accepts optional `system_prompt`
- allocates a UUID session id
- returns `session_id` and `created_at`

Reference: [src/api/handlers/session.rs](../src/api/handlers/session.rs#L16)
Reference: [src/session/store.rs](../src/session/store.rs#L32)

Worth showing in UI:

- session id
- created time
- whether a custom system prompt was used

### 4.4 `DELETE /sessions/{session_id}`

Purpose:

- delete session state from memory

Current behavior:

- always returns `204 No Content`
- does not distinguish between deleting an existing session and a missing session

Reference: [src/api/handlers/session.rs](../src/api/handlers/session.rs#L35)

Worth showing in UI:

- destructive action state
- currently selected session id

### 4.5 `POST /sessions/{session_id}/messages`

Purpose:

- low-level append endpoint for user / assistant / tool / system messages
- gateway into validation, turn counting, trace emission, and compression triggering

Current internal path:

1. `get_or_create_with_id(session_id)`
2. build merged existing view: `stable + pending`
3. validate role sequence and tool metadata
4. append into `stable` or `pending` depending on `is_compressing`
5. emit append trace
6. recompute merged view
7. check turn boundary
8. increment `turn_count` if this append finished a full assistant turn
9. if threshold reached, atomically mark `is_compressing = true`
10. emit `compression_triggered`
11. clone `stable` snapshot
12. schedule async compression

Reference: [src/api/handlers/append.rs](../src/api/handlers/append.rs#L23)

Validation rules are encoded here:

- [src/session/validator.rs](../src/session/validator.rs#L18)
- [src/session/turn.rs](../src/session/turn.rs#L5)

Important current behavior:

- this endpoint implicitly creates the session if the provided `session_id` does not exist
- that is different from `/demo/chat`, which returns `404` for an unknown `session_id`

Reference: [src/session/store.rs](../src/session/store.rs#L59)

Current response:

- `turn_count`
- `message_count`
- `compression_triggered`

Worth showing in UI:

- appended role
- append destination: `stable` vs `pending`
- whether this append crossed a turn boundary
- whether compression was triggered
- message count after append

### 4.6 `GET /sessions/{session_id}/context`

Purpose:

- fetch the latest merged context view for operator inspection and charting

Current internal path:

1. load session
2. build `messages = stable + pending`
3. copy traces
4. read `turn_count`, `compressed_turns`, `stable_message_count`, `pending_message_count`
5. estimate tokens from message characters
6. count summary messages
7. build `latest_summary_preview`
8. derive last compression trigger time from traces
9. derive last compression finish time from traces

Reference: [src/api/handlers/fetch.rs](../src/api/handlers/fetch.rs#L15)

Current response:

- `session_id`
- `messages`
- `turn_count`
- `is_compressing`
- `compressed_turns`
- `token_estimate`
- `stable_message_count`
- `pending_message_count`
- `summary_message_count`
- `latest_summary_preview`
- `last_compression_triggered_at`
- `last_compression_finished_at`
- `traces`

Reference: [src/api/dto.rs](../src/api/dto.rs#L50)

Worth showing in UI:

- transcript view
- compression status
- stable vs pending split
- token pressure
- number of summaries injected
- latest summary content preview
- trace timeline

### 4.7 `POST /demo/chat`

Purpose:

- convenience endpoint for the playground UI
- gives a single request that performs the common chat loop

Current internal path:

1. trim and validate `user_message`
2. resolve `session_id`
   - if provided, it must already exist
   - if absent, create a session and optionally apply a system prompt
3. append the user message through the same append pipeline as `/sessions/{id}/messages`
4. push `demo_chat_started`
5. build LLM chat input from the session
   - include `system`, `user`, final `assistant`
   - exclude `tool` messages
   - exclude assistant messages that contain `tool_calls`
6. call the chat LLM
7. measure `completion_latency_ms`
8. append the assistant response through the same append pipeline
9. push `demo_chat_completed` or `demo_chat_failed`
10. fetch full context via `/sessions/{id}/context`-equivalent logic
11. return session id, assistant text, append results, full context, and latency

Reference: [src/api/handlers/demo.rs](../src/api/handlers/demo.rs#L34)

Current response:

- `session_id`
- `assistant_message`
- `completion_latency_ms`
- `user_append`
- `assistant_append`
- `context`

Reference: [src/api/dto.rs](../src/api/dto.rs#L97)

Worth showing in UI:

- latest request latency
- latest assistant output
- whether user append triggered compression
- whether assistant append triggered compression
- the resulting full context snapshot

## 5. Compression Path

Compression is a background path that starts from append but completes later.

Current internal path:

1. append handler marks `is_compressing = true`
2. append handler clones current `stable` snapshot
3. scheduler spawns `compress_task`
4. compressor plans the split:
   - preserve initial non-summary system prompt
   - compress older completed turns
   - keep the most recent `keep_recent_turns`
5. compression LLM is called with generated system/user prompts
6. on success:
   - replace `stable` with `[preserved head] + [summary] + [recent tail]`
   - drain `pending` into `stable`
   - increment `compressed_turns`
   - move `next_compress_at`
   - emit `compression_succeeded`
7. on failure or timeout:
   - keep prior `stable`
   - drain `pending` into `stable`
   - move `next_compress_at`
   - emit `compression_failed`

References:

- [src/compression/compressor.rs](../src/compression/compressor.rs#L33)
- [src/compression/scheduler.rs](../src/compression/scheduler.rs#L27)
- [src/session/turn.rs](../src/session/turn.rs#L40)

What the UI can already observe indirectly:

- `is_compressing`
- `compression_triggered`
- `compressed_turns`
- last trigger / finish timestamps
- trace events
- stable vs pending counts
- summary message count and preview

What the UI cannot observe directly yet:

- compression attempt count
- compression latency
- snapshot size before compression
- token delta before vs after compression
- exact `next_compress_at`
- whether the last finish was success vs failure without reading traces

## 6. Tracking Model: What Matters Per Call

For a playground or operator console, the useful data naturally falls into six groups.

### 6.1 Request-Level

These answer: “what just happened?”

Already available:

- route invoked
- `session_id`
- `completion_latency_ms` for `/demo/chat`
- append outcome fields
- trace stream

Worth adding next:

- `request_id`
- request start / finish timestamps
- HTTP status code echoed into traceable state
- server-side latency for every endpoint, not just `/demo/chat`
- error code / error class for failed calls

### 6.2 Session-Level

These answer: “which conversation am I looking at?”

Already available:

- `session_id`
- `created_at` on creation response
- global active session count from `/health`

Worth adding next:

- `last_accessed`
- `expires_at`
- session age
- session source: created by `/sessions` vs auto-created by `/demo/chat`

### 6.3 Conversation / Turn-Level

These answer: “where am I in the dialogue?”

Already available:

- `messages`
- `turn_count`
- append role
- trace kinds for appended messages

Worth adding next:

- `last_appended_role`
- `turn_completed` for append responses
- `at_turn_boundary`
- unresolved tool call count
- tool call chain health summary

### 6.4 Compression-Level

These answer: “what is compression doing right now?”

Already available:

- `is_compressing`
- `compressed_turns`
- `stable_message_count`
- `pending_message_count`
- `summary_message_count`
- `latest_summary_preview`
- `last_compression_triggered_at`
- `last_compression_finished_at`
- compression-related traces

Worth adding next:

- `next_compress_at`
- `turns_until_compression`
- `last_compression_status`
- `last_compression_latency_ms`
- `last_compression_attempts`
- `last_compression_error`
- `snapshot_message_count`
- `snapshot_token_estimate`
- `post_compression_token_estimate`
- compression ratio

### 6.5 Runtime / Config-Level

These answer: “under what operating policy is this session running?”

Already available:

- `llm_model`
- `llm_base_url`
- `compression_every_n_turns`
- `keep_recent_turns`
- `llm_timeout_seconds`
- `max_retries`
- `session_ttl_seconds`
- `max_sessions`
- `default_system_prompt`
- `recommended_poll_interval_ms`

Reference: [src/api/handlers/demo.rs](../src/api/handlers/demo.rs#L19)
Reference: [src/config.rs](../src/config.rs#L13)

Worth adding next:

- token estimation divisor in the config response
- active compression prompt mode / prompt version
- whether language enforcement is enabled for compression

### 6.6 Trace / Timeline-Level

These answer: “how did we get here?”

Already available:

- per-session trace list
- event timestamp
- event kind
- human-readable message
- counts at event time

Worth adding next:

- monotonic event sequence id
- event duration where meaningful
- structured metadata payload per event
- correlation to request ids

## 7. Important Behaviors That Affect UI Design

### 7.1 `/demo/chat` and `/sessions/{id}/messages` do not treat missing sessions the same way

- `/demo/chat` returns `404` when a provided `session_id` is unknown
- `/sessions/{id}/messages` creates the session implicitly if it does not exist

This means the operator UI should avoid mixing those two paths casually without a clear session lifecycle model.

References:

- [src/api/handlers/demo.rs](../src/api/handlers/demo.rs#L46)
- [src/session/store.rs](../src/session/store.rs#L59)

### 7.2 Polling `/context` currently does not refresh `last_accessed`

`last_accessed` is updated in several write paths and during demo-chat message preparation, but not in the context fetch handler itself. A UI that only polls `/sessions/{id}/context` may therefore fail to keep a session alive under TTL cleanup.

Relevant references:

- TTL cleanup uses `last_accessed`: [src/session/store.rs](../src/session/store.rs#L97)
- fetch does not call `touch()`: [src/api/handlers/fetch.rs](../src/api/handlers/fetch.rs#L15)

This is important for playground behavior and likely worth fixing.

### 7.3 Trace data is already rich enough to drive a timeline-first UI

Even without adding new DTO fields, the trace stream already gives:

- state transition timing
- append buffer destination context
- compression trigger / finish points
- demo chat lifecycle markers

That is enough to support a lower timeline rail or event console now.

Reference: [src/session/types.rs](../src/session/types.rs#L208)

## 8. Recommended UI Information Architecture

For the next `ctx-cache-compressor` playground iteration, the current API shape suggests a three-layer operator layout.

### 8.1 Primary Pane: Chat + Active Session

Put these together because they form the main task loop:

- transcript
- composer
- latest assistant response
- session id
- latest request latency
- current turn count

### 8.2 Secondary Pane: Compression & Runtime State

This should sit in a right rail or side stack:

- `is_compressing`
- `compressed_turns`
- `stable_message_count`
- `pending_message_count`
- `summary_message_count`
- `latest_summary_preview`
- compression policy from `/demo/config`
- model / timeout / retry / TTL settings

### 8.3 Bottom Observability Rail: Metrics + Timeline + Summary

This works best as a dense operator strip:

- token estimate chart
- stable vs pending chart
- last compression trigger / finish timestamps
- trace timeline
- summary preview panel

## 9. Recommended Next API Additions For UI Work

If the goal is to make the playground feel like a real operator console rather than a static demo, these are the highest-value additions:

1. Add `next_compress_at` and `turns_until_compression` to `FetchContextResponse`
2. Add `last_compression_status`, `last_compression_latency_ms`, and `last_compression_error`
3. Add `last_accessed` and `expires_at`
4. Add request timing metadata to all JSON responses
5. Add an event sequence id to `SessionTraceEvent`
6. Add pre/post compression token metrics

Those six additions would materially improve layout clarity without requiring a large architecture change.
