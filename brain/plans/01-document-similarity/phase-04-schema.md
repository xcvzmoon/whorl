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

- [ ] Create documents with normalized text content hash unique index, text_content, metadata JSONB path-ops GIN, and indexed file_type.
- [ ] Store versioned 125-value signatures and 25 band hashes as integer arrays with GIN; retain an explicit u32-to-i32 bit mapping.
- [ ] Create authoritative batch/file/result tables with errors, progress revision, deadlines, and retention timestamps; queue rows are not status records.
- [ ] Model signatures separately per document/version to allow background re-indexing while queries select exactly one version.
- [ ] Reserve cluster-wide and per-client pending jobs/bytes transactionally, with leases for crash cleanup.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Apply migrations on fresh PostgreSQL; assert indexes, constraints, version coexistence, and reservation races.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.
