# Phase 15: Run independent durable workers

Back to [[plans/01-document-similarity/overview]]

## Goal

Run independent durable workers.

## Changes

- src/bin/worker.rs
- src/service/process_file.rs
- tests/worker.rs

## Data structures

- ProcessFile, WorkerConfig.

## Todos

- [ ] Run the core per-file function in jobs/tests; extraction, hashing, shingling, and verification all use the CPU pool.
- [ ] Set queue handler concurrency <= two CPU workers; persist stage progress and idempotent results before success acknowledgment.
- [ ] Compare against stored corpus then ingest on ingest mode; document concurrent batch ordering and document visibility.
- [ ] On SIGTERM drain or release work according to measured library guarantees; delete inputs after terminal completion or retention.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Kill/restart, long-job reclaim protection, retry exhaustion, permanent failure, shutdown recovery, and handler concurrency pass in focused changes.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.
