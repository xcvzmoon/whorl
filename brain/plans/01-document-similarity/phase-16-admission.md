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

- [x] Reject with 503/429 plus Retry-After before reading a large body when pending jobs/bytes are exhausted.
- [x] Use authenticated upstream client identity, never an untrusted arbitrary header; require its forwarding contract.
- [x] Reserve a bounded upload budget before content arrives; reconcile actual bytes and file counts during streaming.
- [x] Expire abandoned leases and make release idempotent across every failure path.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Racing instances cannot over-admit; one client cannot fill capacity; expired reservations free space.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.

## Completion evidence

- `AdmissionLease` reserves a request budget under the shared PostgreSQL lock; client and global failures carry the approved HTTP codes and Retry-After hints for phase 17.
- Trusted-proxy forwarding is required explicitly; no public-header authentication is introduced.
- Uploads transfer reserved file/byte budgets and reconcile actual bytes without double charging. Zero-balance leases remain valid until release or expiry.
- PostgreSQL/MinIO integration tests cover racing instances, client isolation, expiration, repeated release, bounded file attempts, actual-byte accounting and cleanup. See [admission operations](../../../docs/admission.md).
- HTTP response and multipart wiring remain in phase 17. Production quotas require workload calibration during acceptance.
