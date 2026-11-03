# Phase 10: Verify graphile_worker before adopting it

Back to [[plans/01-document-similarity/overview]]

## Goal

Verify graphile_worker before adopting it.

## Changes

- spikes/queue/Cargo.toml
- spikes/queue/src/main.rs
- docs/queue-spike.md

## Data structures

- QueueCapabilities.

## Todos

- [x] Check current graphile_worker sqlx compatibility, release cadence, commits, open issues, downloads, license, and MSRV.
- [x] Measure wake-up notifications/fallback polling, bounded retries/backoff, permanent failure recording, and idempotent job keys.
- [x] Kill workers and run jobs for several minutes; prove lock protection, recovery window, shutdown behavior, and actual completed/failed retention.
- [x] Determine schema migration owner and minimum privileges, API enqueue-only utility support, and application-transaction enqueue feasibility.
- [x] Evaluate apalis-postgres only if requirements fail; report before switching.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Publish executable evidence for each queue requirement; block worker implementation on unresolved duplicate execution/reclaim behavior.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.

## Evidence

- Completed November 3, 2026. [Queue spike report](../../../docs/queue-spike.md) records dependency audit, lifecycle behavior, migration ownership, enqueue privileges and adoption constraints.
- The isolated `spikes/queue/` executable pins graphile_worker 0.14.1 and SQLx 0.9.0. PostgreSQL 18.6 assertions passed for notifications, polling, bounded retries, failed retention, pending/running job keys, transactional enqueue, 185-second live-job protection, SIGKILL recovery and SIGTERM cancellation/release.
- Measured output and dependency API snapshots are in `spikes/queue/evidence/`. Restricted-role grants/RLS and typed utility rollback passed separately and are included in the reproduction runner.
- Retain graphile_worker; no failed requirement triggered apalis-postgres evaluation or a library switch. Delivery remains at least once. Heartbeat recovery is opt-in; application state transitions must fence repeated side effects. Queue cancellation cannot terminate a CPU thread or extraction process.
- Formatting, Clippy with warnings denied, warning-free rustdoc, the application test suite and 24 doctests pass. Existing PostgreSQL application integration tests remain ignored in the ordinary suite; this phase executed its own queue integration probe.
