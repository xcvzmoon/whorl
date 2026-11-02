# Phase 2: Define a reproducible extraction corpus

Back to [[plans/01-document-similarity/overview]]

## Goal

Define a reproducible extraction corpus.

## Changes

- spikes/extraction/manifest.json
- spikes/extraction/README.md

## Data structures

- CorpusCase, ExtractionMeasurement.

## Todos

- [ ] Obtain licensed real digital/scanned PDFs, office documents with tables/images/notes, large spreadsheets, and photos of text; no corpus is supplied yet.
- [ ] Include a 20 MiB fixture per format, malformed files, zip bombs, huge images, and expected text/metadata.
- [ ] Record fixture provenance, checksums, machine specs, wall time, peak RSS, and accuracy metrics.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Verify fixture provenance, reproducibility, and coverage before claiming extraction results.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.
