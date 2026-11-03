# Phase 3: Benchmark extraction candidates and recommend per format

Back to [[plans/01-document-similarity/overview]]

## Goal

Benchmark extraction candidates and recommend per format.

## Changes

- spikes/extraction/Cargo.toml
- spikes/extraction/src/main.rs
- docs/extraction-spike.md

## Data structures

- ExtractorTrial.

## Todos

- [x] Audit current candidate releases, activity, issues, downloads, licenses, and MSRV before adding dependencies; pin pre-1.0 versions deliberately.
- [x] Compare lopdf with pdfium-render; zip/quick-xml for DOCX/PPTX including notes/core properties; calamine for XLSX; image/EXIF plus ocrs versus Tesseract.
- [x] Measure OCR word and character error, text coverage, extraction speed, and peak memory on the two-core target.
- [x] Evaluate smaller word k versus character shingles for OCR consistently across ingest/query; propose subprocess tools with strict resource limits when needed.
- [x] Report evidence and get approval before selecting a fallback or changing requirements.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Publish raw results and recommendations; do not substitute generated fixtures for real-file quality evidence.

## Evidence and approval status

- [Extraction report](../../../docs/extraction-spike.md): per-format recommendations, maintenance/license findings, OCR/shingle results, limits, and unresolved production-adapter work.
- 414 trials cover all 34 cases and 69 candidate/configuration combinations on Linux ARM64 under a two-CPU quota and 4 GiB memory cap (the Docker VM has slightly less RAM available). Each combination has one warmup and five measured repetitions.
- [Raw measurements](../../../spikes/extraction/measurements.json), checksum-verified [raw artifacts](../../../spikes/extraction/results/raw-results.tar.gz), and [all summaries](../../../spikes/extraction/results/summary.md) are published. Real-file quality is separated from generated limit probes.
- Two Rust tests and fourteen Linux Python tests pass. Formatting, Clippy, tests, and warning-free rustdoc pass; RustSec reports no vulnerabilities or warnings. Both packages are binary-only with no library doctests.
- Benchmark and reporting work is complete. User approval on November 3, 2026 resolves the per-format tools, native-process limits and Ocrs model terms. Retain single-character OCR lines and evaluate a maintained EXIF alternative. See [approval record](../../../docs/decisions.md#phase-13-extraction-approval).

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.
