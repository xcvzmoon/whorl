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

- [x] Serve authoritative persisted batch/per-file states and named stage progress; avoid invented percentage completion.
- [x] Support ETag/If-None-Match and 304, suggested Retry-After, Location, no-store, and expiration semantics.
- [x] Keep polling delivery independent of worker processing; defer optional short long-polling, SSE, and webhooks.
- [x] Expose liveness and readiness without CPU-pool dependence; scope batch visibility to trusted upstream client identity.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Check every aggregate state, concurrent progress revisions, ETag changes, 304 behavior, unknown/expired ids, and responsive health.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.

## Evidence

- `GET /v1/batches/{id}` reads batch, files, and matches from one read-only repeatable-read snapshot. A server cursor fetches at most 128 matches at a time in score/id order. Disconnecting drops the transaction and cursor.
- ETags use the persisted batch revision; weak tags, lists, and `*` support conditional GET. Expiration and ownership checks precede conditional handling. Terminal polling omits Retry-After and never extends retention.
- `/health/live` requires neither identity nor dependencies. `/health/ready` checks PostgreSQL with a two-second timeout; neither uses extraction or CPU workers.
- PostgreSQL 18 Docker integration passed with all aggregate states, named stages, queued retries, changing revisions, concurrent result changes during response delivery, 300 streamed matches, empty results, conditional responses, expiration/tombstones, and client isolation.
- Run the integration on a fresh database with `WHORL_POLL_TEST_URL=postgres://... cargo test --test polling -- --include-ignored`. It creates the application schema and needs no S3 service or running worker.
- Retention deletion and reindexing remain phase 19 work; polling already enforces persisted expiration and bounded tombstone visibility.
- Final checks passed: rustfmt, strict all-target/all-feature Clippy, all-target/all-feature tests, 48 doctests, warning-free rustdoc, release build, and diff whitespace checks. Existing opt-in integrations remained ignored in the general suite; the new polling integration ran explicitly against PostgreSQL 18.
