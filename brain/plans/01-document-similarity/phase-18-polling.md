# Phase 18: Implement status, results, and health endpoints

Back to [[plans/01-document-similarity/overview]]

## Goal

Implement status, results, and health endpoints.

## Changes

- src/http/status.rs
- src/http/health.rs
- tests/polling.rs

## Data structures

- BatchRevision, PollResponse.

## Todos

- [ ] Serve authoritative persisted batch/per-file states and named stage progress; avoid invented percentage completion.
- [ ] Support ETag/If-None-Match and 304, suggested Retry-After, Location, no-store, and expiration semantics.
- [ ] Keep polling delivery independent of worker processing; defer optional short long-polling, SSE, and webhooks.
- [ ] Expose liveness and readiness without CPU-pool dependence; scope batch visibility to trusted upstream client identity.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Check every aggregate state, concurrent progress revisions, ETag changes, 304 behavior, unknown/expired ids, and responsive health.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.
