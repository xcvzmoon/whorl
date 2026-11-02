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

- [ ] Check current graphile_worker sqlx compatibility, release cadence, commits, open issues, downloads, license, and MSRV.
- [ ] Measure wake-up notifications/fallback polling, bounded retries/backoff, permanent failure recording, and idempotent job keys.
- [ ] Kill workers and run jobs for several minutes; prove lock protection, recovery window, shutdown behavior, and actual completed/failed retention.
- [ ] Determine schema migration owner and minimum privileges, API enqueue-only utility support, and application-transaction enqueue feasibility.
- [ ] Evaluate apalis-postgres only if requirements fail; report before switching.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Publish executable evidence for each queue requirement; block worker implementation on unresolved duplicate execution/reclaim behavior.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.
