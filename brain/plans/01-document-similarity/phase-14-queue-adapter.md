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

- [ ] Hide library utility APIs behind enqueue/retry operations and stable payloads referring to stored bytes.
- [ ] Create one self-contained job per valid file with job keys; coordinate batch transaction and enqueue atomically if supported.
- [ ] If atomic enqueue is unsupported, propose a transactional outbox for delivery only and obtain approval; never replace the queue with a hand-rolled job table.
- [ ] Separate permanent extraction failures from retryable storage/database failures; bound retry count/backoff.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Concurrent submissions and partial enqueue failures preserve one logical file operation with recoverable state.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.
