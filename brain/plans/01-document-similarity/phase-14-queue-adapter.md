# Phase 14: Add library-backed enqueue and status persistence

Back to [[plans/01-document-similarity/overview]]

## Goal

Add library-backed enqueue and status persistence.

## Changes

- src/queue/mod.rs
- src/queue/graphile.rs
- tests/enqueue.rs

## Data structures

- JobQueue, FileJobPayload.

## Todos

- [x] Hide library utility APIs behind enqueue/retry operations and stable payloads referring to stored bytes.
- [x] Create one self-contained job per valid file with job keys; coordinate batch transaction and enqueue atomically if supported.
- [x] If atomic enqueue is unsupported, propose a transactional outbox for delivery only and obtain approval; never replace the queue with a hand-rolled job table.
- [x] Separate permanent extraction failures from retryable storage/database failures; bound retry count/backoff.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Concurrent submissions and partial enqueue failures preserve one logical file operation with recoverable state.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.

## Completion evidence

- Completed November 3, 2026. Adapter, transaction contract and repeatable PostgreSQL test commands are in [queue adapter](../../../docs/queue-adapter.md).
- Atomic library enqueue is supported; no outbox is required. Batch/file insertion and queue errors roll back together, and concurrent pending submissions return one job id.
- Attempt budget is explicit and library backoff is retained. Production calibration remains an operations gate. Worker status transitions and retry-safe side effects remain phase 15.
- Full suite, 40 doctests, fmt, Clippy, warning-free rustdoc and dependency/license/source audits passed. Enqueue integration tests ran against PostgreSQL 18.6.
