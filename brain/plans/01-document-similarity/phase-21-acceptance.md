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

- [x] Run real PostgreSQL and S3 integration tests for ingest/check, duplicate writes, version mismatch, partial failure, and polling.
- [x] Load one/five-file batches against a stated corpus on 2 cores/4 GiB; measure peak RSS, health/status latency, admission, and queue age.
- [x] Benchmark extraction at 20 MiB per format, signing by text size, candidate lookup, and complete batch latency.
- [x] Report the actual bottleneck and measured safe settings before suggesting optimization; update README with working setup/operations commands.
- [x] Keep fmt/clippy/doc/doctests, dependency/license audits, deterministic/property/recall tests in CI; provision integration services only when those tests exist.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Publish reproducible measurements and all acceptance evidence; unresolved workload scale prevents a production capacity claim.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.

## Acceptance evidence

- Completed November 4, 2026. See [measurements and reproduction](../../../docs/benchmarks.md).
- Real PostgreSQL 18.6/MinIO integration tests passed on macOS and Linux; the Linux run includes successful extraction, worker recovery, and HTTP-to-queue-to-polling acceptance.
- Independent API/worker smoke load completed 20 one/five-file batches under two CPUs and 4 GiB, with aggregate memory, health/status latency, admission, and pending age recorded.
- CPU signing/batch benchmarks and the unchanged extractor's six exact 20 MiB probes are documented with their workload limits.
- CI runs every integration suite and retains deterministic/recall tests, fmt, Clippy, rustdoc, doctests, and dependency/license gates. Hosted CI was not run locally.
- Customer corpus size and peak arrivals remain unspecified. Completion establishes fixture acceptance, not production capacity.
