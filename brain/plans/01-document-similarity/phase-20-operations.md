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

- [x] Document every configurable default and validation range; unresolved extraction/queue values require benchmark-derived approval.
- [x] Add typed environment configuration and local .env handling after dependency audit.
- [x] Measure queue depth, oldest pending age, stage duration, retries/failures, pool saturation, and pending bytes without leaking document contents.
- [x] Document logs, alerts, S3 cleanup, recovery, migrations, and rollout ordering.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Invalid config fails at startup; metrics reflect a real partial-failure batch and cleanup cycle.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.


## Evidence

- Shared typed startup settings validate database options, IP listen addresses and worker counts; local dotenv loading runs before async runtime creation and preserves existing environment values.
- Existing approved extraction, retry and resource defaults remain fixed. The configuration guide links their complete definitions and records the dotenvy audit.
- Fixed JSON telemetry covers stage duration/completion, CPU occupancy/overload, durable pending files/age, retained retries/failures, reserved bytes and expired upload journals.
- PostgreSQL 18.6 and local MinIO integration passed for a mixed-outcome database batch fixture and actual S3 upload cleanup; reserved bytes and expired journal counts fell to zero. This test does not execute Linux extraction.
- Startup subprocess tests passed for invalid worker limits, malformed dotenv input and environment precedence.
- Rustfmt, Clippy with warnings denied, all-target tests, 48 doctests and warning-free rustdoc passed. Existing external-service tests retain their opt-in requirements; Linux extraction was not rerun in this phase.
- Operations guide covers alerts, cleanup, recovery, migration privileges and rollout ordering. Phase 21 remains the deployment/acceptance gate.
