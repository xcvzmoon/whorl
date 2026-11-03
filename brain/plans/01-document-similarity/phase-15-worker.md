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

- [x] Run the core per-file function in jobs/tests; extraction, hashing, shingling, and verification all use the CPU pool.
- [x] Set queue handler concurrency <= two CPU workers; persist stage progress and idempotent results before success acknowledgment.
- [x] Compare against stored corpus then ingest on ingest mode; document concurrent batch ordering and document visibility.
- [x] On SIGTERM drain or release work according to measured library guarantees; delete inputs after terminal completion or retention.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Kill/restart, long-job reclaim protection, retry exhaustion, permanent failure, shutdown recovery, and handler concurrency pass in focused changes.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.

## Evidence

- Added the shared `ProcessFile` pipeline and independent `worker` binary. Per-file advisory locks and persisted attempt generations fence overlapping execution; matches, canonical ingest and terminal state commit together.
- Extraction supervision, normalization/hashing/shingling and candidate verification share one bounded CPU pool. Three queued jobs reached exactly two concurrent handlers in the Linux integration test.
- Batch algorithm snapshots derive the detected native/OCR namespace with identical registered configuration; a mixed-source batch passes without cross-namespace matches. Committed sibling ingests remain visible according to the existing API contract.
- Linux/PostgreSQL 18.6 integration passed in 9.76 seconds: real DOCX ingest/check/duplicate, tenant isolation, immutable repeated delivery, storage retry/exhaustion, expired deadline, permanent rejection, cancellation/restart, graceful queue release, healthy long work under competing stale sweeps, separate-process SIGKILL/recovery, and handler concurrency.
- Real MinIO tests preserve expired queued/running references and reclaim terminal references. Successful multipart completion must clear its journal marker before queue admission.
- Full host suite, 44 doctests, Clippy with warnings denied, warning-free rustdoc, release build and cargo-deny advisories/licenses/sources pass. Integration tests run explicitly on disposable databases; CI now includes the worker gate.
- See [worker operation and lifecycle](../../../docs/worker.md). No HTTP/admission endpoint was added; phase 16 is next. Changes remain uncommitted.
