# Phase 11: Build a fixed-size bounded CPU executor

Back to [[plans/01-document-similarity/overview]]

## Goal

Build a fixed-size bounded CPU executor.

## Changes

- src/cpu_pool.rs
- tests/cpu_pool.rs

## Data structures

- CpuPool, WorkDeadline.

## Todos

- [ ] Extend the phase 9 matcher executor into the shared pipeline CPU pool; keep two workers total across matching, extraction, and signing. Use two fixed workers on the target, configurable to available cores, and oneshot result delivery.
- [ ] Bound waiting slots by workers * acceptable_wait / measured_typical_job_time; reject immediately when full.
- [ ] Skip expired work, support cancellation and shutdown, and never use the general blocking pool for CPU-heavy work.
- [ ] Define a hard timeout strategy: a running Rust thread cannot be safely killed; isolate uninterruptible extraction in resource-limited subprocesses when required.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Saturation returns fast overload responses; expired jobs never start; timers and health/status remain responsive.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.
