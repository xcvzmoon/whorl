# Phase 21: Prove end-to-end correctness and capacity

Back to [[plans/01-document-similarity/overview]]

## Goal

Prove end-to-end correctness and capacity.

## Changes

- tests/end_to_end.rs
- benches/batches.rs
- docs/benchmarks.md

## Data structures

- AcceptanceReport.

## Todos

- [ ] Run real PostgreSQL and S3 integration tests for ingest/check, duplicate writes, version mismatch, partial failure, and polling.
- [ ] Load one/five-file batches against a stated corpus on 2 cores/4 GiB; measure peak RSS, health/status latency, admission, and queue age.
- [ ] Benchmark extraction at 20 MiB per format, signing by text size, candidate lookup, and complete batch latency.
- [ ] Report the actual bottleneck and measured safe settings before suggesting optimization; update README with working setup/operations commands.
- [ ] Keep fmt/clippy/doc/doctests, dependency/license audits, deterministic/property/recall tests in CI; provision integration services only when those tests exist.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Publish reproducible measurements and all acceptance evidence; unresolved workload scale prevents a production capacity claim.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.
