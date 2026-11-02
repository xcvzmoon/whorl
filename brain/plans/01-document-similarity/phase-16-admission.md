# Phase 16: Add cross-instance backpressure

Back to [[plans/01-document-similarity/overview]]

## Goal

Add cross-instance backpressure.

## Changes

- src/service/admission.rs
- tests/admission.rs

## Data structures

- AdmissionLease, ClientQuota.

## Todos

- [ ] Reject with 503/429 plus Retry-After before reading a large body when pending jobs/bytes are exhausted.
- [ ] Use authenticated upstream client identity, never an untrusted arbitrary header; require its forwarding contract.
- [ ] Reserve a bounded upload budget before content arrives; reconcile actual bytes and file counts during streaming.
- [ ] Expire abandoned leases and make release idempotent across every failure path.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Racing instances cannot over-admit; one client cannot fill capacity; expired reservations free space.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.
