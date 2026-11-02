# Phase 12: Implement direct S3-compatible durable uploads

Back to [[plans/01-document-similarity/overview]]

## Goal

Implement direct S3-compatible durable uploads.

## Changes

- src/storage/mod.rs
- src/storage/s3.rs
- tests/storage.rs

## Data structures

- UploadStore, StoredObjectRef.

## Todos

- [ ] Evaluate maintained S3 client dependencies and provider compatibility before selection.
- [ ] Stream to random object keys with bounded buffers; abort failed multipart uploads and delete rejected objects.
- [ ] Record durable object references plus retention timestamps; define crash reconciliation between object completion and database admission.
- [ ] Use reservation accounting before accepting bytes, including per-client quotas; never store raw credentials in jobs.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Cross-instance read, interrupted multipart cleanup, storage failures, reservation cleanup, and bounded memory pass.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.
