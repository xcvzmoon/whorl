# Phase 13: Implement the approved extraction pipeline

Back to [[plans/01-document-similarity/overview]]

## Goal

Implement the approved extraction pipeline.

## Changes

- src/extraction/mod.rs
- src/extraction/limits.rs
- tests/extraction.rs

## Data structures

- Extractor, ExtractionLimits, ExtractedDocument.

## Todos

- [ ] Implement one format adapter per separate reviewable follow-up using spike-approved tools; keep adapters modular.
- [ ] Validate magic and archive structure, not just extension/MIME; reject malformed and unsupported input.
- [ ] Bound archive uncompressed bytes and ratio, XML parsing, pages/slides/sheets/cells, image pixels before decoding, OCR resources, and total output text.
- [ ] Apply configurable per-file timeout and enforce native-tool process termination/cleanup where needed.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Run approved corpus and adversarial fixtures with measured resource ceilings; split each adapter implementation into its own small change.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.
