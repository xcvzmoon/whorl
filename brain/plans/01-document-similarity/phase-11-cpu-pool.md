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

- [x] Extend the phase 9 matcher executor into the shared pipeline CPU pool; keep two workers total across matching, extraction, and signing. Use two fixed workers on the target, configurable to available cores, and oneshot result delivery.
- [x] Bound waiting slots by workers * acceptable_wait / measured_typical_job_time; reject immediately when full.
- [x] Skip expired work, support cancellation and shutdown, and never use the general blocking pool for CPU-heavy work.
- [x] Define a hard timeout strategy: a running Rust thread cannot be safely killed; isolate uninterruptible extraction in resource-limited subprocesses when required.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Saturation returns fast overload responses; expired jobs never start; timers and health/status remain responsive.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.

## Completion evidence

- `src/cpu_pool.rs` provides the shared dedicated Rayon pool, measured waiting-capacity calculation, immediate admission, oneshot delivery, absolute deadlines, cancellation, panic containment and drainable shutdown.
- `Matcher` accepts the existing pool and a per-file deadline; verification and result sorting use it without starting another pool.
- Lifecycle tests in `tests/cpu_pool.rs` cover saturation, queued expiration/cancellation, running cancellation, async timers, shutdown, panic recovery and concurrency limits.
- [Executor operation and timeout contract](../../../docs/cpu-pool.md) records sizing evidence and the subprocess hard-timeout requirements for phase 13. Production waiting capacity requires target-workload measurements; zero wait is supported.
- Validation: formatting, strict Clippy, all-target tests, doctests, warning-free rustdoc, dependency/license audit and real PostgreSQL integration tests. The audit reports one existing unused BSD-2-Clause allowance.
