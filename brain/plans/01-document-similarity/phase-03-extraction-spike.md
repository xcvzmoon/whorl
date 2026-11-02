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

- [ ] Audit current candidate releases, activity, issues, downloads, licenses, and MSRV before adding dependencies; pin pre-1.0 versions deliberately.
- [ ] Compare lopdf with pdfium-render; zip/quick-xml for DOCX/PPTX including notes/core properties; calamine for XLSX; image/EXIF plus ocrs versus Tesseract.
- [ ] Measure OCR word and character error, text coverage, extraction speed, and peak memory on the two-core target.
- [ ] Evaluate smaller word k versus character shingles for OCR consistently across ingest/query; propose subprocess tools with strict resource limits when needed.
- [ ] Report evidence and get approval before selecting a fallback or changing requirements.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Publish raw results and recommendations; do not substitute generated fixtures for real-file quality evidence.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.
