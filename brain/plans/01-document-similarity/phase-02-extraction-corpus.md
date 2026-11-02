# Phase 2: Define a reproducible extraction corpus

Back to [[plans/01-document-similarity/overview]]

## Goal

Define a reproducible extraction corpus.

## Changes

- spikes/extraction/manifest.json
- spikes/extraction/README.md
- spikes/extraction/corpus.py and test_corpus.py
- spikes/extraction/fixtures/, expected/, and licenses/
- spikes/extraction/measurements.json

## Data structures

- CorpusCase, ExtractionMeasurement.

## Todos

- [x] Obtain licensed real digital/scanned PDFs, office documents with tables/images/notes, large spreadsheets, and photos of text.
- [x] Include a 20 MiB fixture per format, malformed files, zip bombs, huge images, and expected text/metadata.
- [x] Record fixture provenance, checksums, and machine specs; define wall-time, peak RSS, and accuracy records for the phase 3 extraction trials.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Verify fixture provenance, reproducibility, and coverage before claiming extraction results.

## Evidence

- [Corpus manifest and protocol](../../../spikes/extraction/README.md): 34 cases, including ten licensed upstream files, exact size boundaries for six format families, malformed inputs, archive expansion probes, and a huge image.
- Fixture, reference, generator, and license checksums pass; regeneration matches. Five validator tests pass, including damaged input/reference and missing coverage/license checks.
- Boundary PDF text and metadata match the source; boundary image dimensions and office ZIP/XML structure pass inspection. The large XLSX contains 100,000 rows.
- Preparation host specs and the extraction measurement record are in `spikes/extraction/measurements.json`. Wall-time, peak RSS, and accuracy fields are defined in the protocol; extraction trials remain empty until phase 3. No extraction quality or performance claim is made.
- Cargo formatting, Clippy, tests, and warning-free rustdoc pass; no Rust implementation changed.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.
