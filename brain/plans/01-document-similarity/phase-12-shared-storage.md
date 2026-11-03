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

- [x] Evaluate maintained S3 client dependencies and provider compatibility before selection.
- [x] Stream to random object keys with bounded buffers; abort failed multipart uploads and delete rejected objects.
- [x] Record durable object references plus retention timestamps; define crash reconciliation between object completion and database admission.
- [x] Use reservation accounting before accepting bytes, including per-client quotas; never store raw credentials in jobs.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Cross-instance read, interrupted multipart cleanup, storage failures, reservation cleanup, and bounded memory pass.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.

## Completion evidence

- Added `UploadStore`, credential-free `StoredObjectRef`, the S3 client builder and `0002_uploads.sql`; details and deployment requirements are in [storage](../../../docs/storage.md).
- Selected pinned object_store 0.14.2 after checking current client APIs, crates.io metadata, Apache repository activity, license and MSRV. Existing Bytes, Tokio and UUID dependencies provide buffers, async input and random keys.
- Local PostgreSQL 18.6 and MinIO integration passed cross-client reads, exact-limit uploads, empty/oversized/input-error rejection, quota denial, interrupted multipart cleanup, active-writer protection and retained reservations during S3 outages.
- Uploads hold an advisory transaction lock; admission must use the same lock when attaching a completed same-client key. Reconciliation rechecks references after locking. A required bucket lifecycle rule covers unknown multipart IDs from a crash during creation.
- Endpoint, bucket ownership and secret delivery remain deployment inputs; no production deployment was attempted. Request admission and source-retention scheduling remain phases 16–19.
