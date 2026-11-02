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

- [ ] Stream one to five files to durable storage with authoritative inclusive per-file/combined byte counters.
- [ ] Use raw body cap 104857600 + proposed 1048576 overhead; bound part headers and multipart structure too.
- [ ] Reject sixth file, zero files, or combined excess as a whole; count rejected-file bytes toward total.
- [ ] On a file crossing its cap, stop storing it immediately and drain/count remaining bytes with bounded buffers so later valid files survive; record file-specific too-large/unsupported/empty errors.
- [ ] Return 202 after durable admission; clean all staged objects on request rejection or disconnect.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Cover exact 20971520/20971521 boundaries, five exact files, total excess, sixth/zero/single files, and incremental rejection without whole-body buffering.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.
