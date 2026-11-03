# Phase 17: Implement the approved multipart submission

Back to [[plans/01-document-similarity/overview]]

## Goal

Implement the approved multipart submission.

## Changes

- src/http/submit.rs
- src/http/errors.rs
- tests/uploads.rs

## Data structures

- Submission, ApiError.

## Todos

- [x] Stream one to five files to durable storage with authoritative inclusive per-file/combined byte counters.
- [x] Use raw body cap 104857600 + proposed 1048576 overhead; bound part headers and multipart structure too.
- [x] Reject sixth file, zero files, or combined excess as a whole; count rejected-file bytes toward total.
- [x] On a file crossing its cap, stop storing it immediately and drain/count remaining bytes with bounded buffers so later valid files survive; record file-specific too-large/unsupported/empty errors.
- [x] Return 202 after durable admission; clean all staged objects on request rejection or disconnect.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Cover exact 20971520/20971521 boundaries, five exact files, total excess, sixth/zero/single files, and incremental rejection without whole-body buffering.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.

## Completion evidence

- Implemented Actix submission routes, pre-body admission, bounded multipart streaming, transactional batch/file/job writes and cancellation cleanup. Setup and limits are in `docs/uploads.md`.
- PostgreSQL 18.6 and local MinIO integration passed exact 20 MiB/20 MiB + 1 boundaries, five exact files, combined excess, mixed/empty files, zero/sixth files, malformed content/headers, unknown parts, raw-body excess, identity rejection and injected enqueue failure with rollback/cleanup.
- Unsupported types remain individual extraction failures, as permitted by the approved contract. HTTP does not trust filename extensions or supplied MIME types.
- rustfmt, strict Clippy, the full default test suite, all 48 doctests and warning-free rustdoc passed. Existing external-service tests remain opt-in; the new upload integration suite was run explicitly.
- Phase 18 (polling) is next. Submission returns its reserved status URL; that route is not implemented in this phase.
