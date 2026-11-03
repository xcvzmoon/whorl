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

- [x] Implement one format adapter per separate reviewable follow-up using spike-approved tools; keep adapters modular.
- [x] Validate magic and archive structure, not just extension/MIME; reject malformed and unsupported input.
- [x] Bound archive uncompressed bytes and ratio, XML parsing, pages/slides/sheets/cells, image pixels before decoding, OCR resources, and total output text.
- [x] Apply configurable per-file timeout and enforce native-tool process termination/cleanup where needed.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Run approved corpus and adversarial fixtures with measured resource ceilings; split each adapter implementation into its own small change.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.

## Completion evidence

- User approval is recorded in [decisions](../../../docs/decisions.md#phase-13-extraction-approval). Single-character OCR lines remain; nom-exif 3.8.0 replaces the stale EXIF candidate after evaluation.
- Modular Office and native adapters validate content signatures/package relationships and apply parsing, expansion, image, output and process ceilings. Linux supervision kills the entire process group on timeout or cancellation and removes temporary files.
- Host full suite, rustfmt, Clippy with warnings denied, warning-free rustdoc and all 37 doctests passed. Existing PostgreSQL/MinIO integration tests remain explicitly excluded; those components are unchanged.
- Linux image tests passed: eight library tests and six extraction integration tests, including deadlines, cancellation, descendant cleanup, temporary cleanup and output overflow.
- The production Linux ARM64 image passed all 34 corpus cases plus a mixed native/scanned PDF, under two CPUs, 4 GiB RAM/no swap and 128 MiB temporary storage. Ordered text and asserted metadata checks pass; OCR quality is recorded without filtering single-character lines. [Measurements](../../../docs/extraction-results.json), [runtime/reproduction instructions](../../../docs/extraction.md), and [dependency audit](../../../docs/extraction-dependencies.json).
- All native/model asset hashes verify; RustSec reports no advisories or warnings. Public GitHub API rate limiting prevented an issue-response metric for nom-exif; no responsiveness claim is made.
- Runtime deployment still must provide the documented cgroup/filesystem boundaries and trusted asset paths. This phase does not wire queue jobs or HTTP endpoints.
