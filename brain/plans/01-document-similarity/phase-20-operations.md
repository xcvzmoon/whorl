# Phase 20: Add configuration and observability

Back to [[plans/01-document-similarity/overview]]

## Goal

Add configuration and observability.

## Changes

- src/config.rs
- src/telemetry.rs
- docs/configuration.md

## Data structures

- ServiceConfig, StageMetrics.

## Todos

- [ ] Document every configurable default and validation range; unresolved extraction/queue values require benchmark-derived approval.
- [ ] Add typed environment configuration and local .env handling after dependency audit.
- [ ] Measure queue depth, oldest pending age, stage duration, retries/failures, pool saturation, and pending bytes without leaking document contents.
- [ ] Document logs, alerts, S3 cleanup, recovery, migrations, and rollout ordering.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Invalid config fails at startup; metrics reflect a real partial-failure batch and cleanup cycle.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.
