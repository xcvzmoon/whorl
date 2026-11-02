# Phase 4: Create application migrations

Back to [[plans/01-document-similarity/overview]]

## Goal

Create application migrations.

## Changes

- migrations/0001_application.sql
- src/persistence/mod.rs
- tests/schema.rs

## Data structures

- Document, Signature, Batch, BatchFile, MatchResult, CapacityReservation.

## Todos

- [x] Create documents with normalized text content hash unique index, text_content, metadata JSONB path-ops GIN, and indexed file_type.
- [x] Store versioned 125-value signatures and 25 band hashes as integer arrays with GIN; retain an explicit u32-to-i32 bit mapping.
- [x] Create authoritative batch/file/result tables with errors, progress revision, deadlines, and retention timestamps; queue rows are not status records.
- [x] Model signatures separately per document/version to allow background re-indexing while queries select exactly one version.
- [x] Reserve cluster-wide and per-client pending jobs/bytes transactionally, with leases for crash cleanup.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Apply migrations on fresh PostgreSQL; assert indexes, constraints, version coexistence, and reservation races.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.

## Evidence

- Applied the transactional migration to fresh Docker PostgreSQL 18.6 databases on November 3, 2026. Image digest and reproduction instructions are in [persistence documentation](../../../docs/persistence.md).
- Database tests pass for canonical uniqueness, client boundaries, signature version coexistence, array/status/retention constraints, indexes, revision updates, reservation rollback, global/client quotas, and lease cleanup accounting.
- Twelve concurrent reservation transactions admit exactly three at the configured client limit. Expired leases retain accounting until cleanup explicitly releases them.
- Rust formatting, Clippy with warnings denied, tests, and warning-free documentation pass. No dependencies added. CI now explicitly runs the PostgreSQL test against the pinned service image.
- Deployment-specific persistent storage, connection/TLS settings, and migration privileges remain deployment configuration; no production database was deployed.
