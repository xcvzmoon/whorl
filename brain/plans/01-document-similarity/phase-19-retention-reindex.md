# Phase 19: Add maintenance and signature lifecycle

Back to [[plans/01-document-similarity/overview]]

## Goal

Add maintenance and signature lifecycle.

## Changes

- src/bin/maintenance.rs
- src/service/maintenance.rs
- tests/maintenance.rs

## Data structures

- RetentionPolicy, ReindexCursor.

## Todos

- [x] Choose a deployment migration step for application and queue schemas with documented elevated privileges; runtime roles stay restricted.
- [x] Sweep expired results/job rows after default 24h, terminal objects, orphan objects, and abandoned reservations/work.
- [x] Re-index stored text in bounded background pages to a new version; queries remain version-isolated and switch only when coverage is ready.
- [x] Specify irreversible cleanup behavior and do not add backward compatibility layers or migrations for obsolete paths.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Restart sweeps/re-index jobs safely, verify expiry, and prove old/new signatures are never mixed.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.

## Completion evidence

- `maintenance migrate` serializes and records application schema versions, then runs the pinned queue migrations. Repeated deployment runs passed on fresh PostgreSQL 18.
- `maintenance sweep` bounds work to 32 entries per class, fences deadline recovery with worker locks, removes expired results and tombstones, completes unlocked expired terminal queue jobs, reclaims abandoned reservations, and invokes journaled S3 reconciliation.
- `maintenance reindex <native|ocr> <k>` builds supported policy versions from stored text, one CPU task at a time. Missing signatures form the restart cursor. Namespace membership comes from existing signatures.
- PostgreSQL integration verification passed for lock exclusion, deadline recovery, expired request reservations, terminal queue deletion, tombstone retention/deletion, restart safety, source isolation, short-text NULL arrays, and 32+1 page boundaries.
- Full `cargo test` (including 48 doctests), Clippy with warnings denied, rustfmt, and rustdoc with warnings denied passed. Existing external-service tests remain opt-in; MinIO was unavailable locally, so this phase reused the previously tested S3 reconciliation path.
- Deployment privileges, irreversible deletion and paused-ingest coverage/cutover procedure are documented in `docs/maintenance.md`. No production cleanup or cutover was executed.
