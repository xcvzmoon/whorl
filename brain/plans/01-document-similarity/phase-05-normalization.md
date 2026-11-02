# Phase 5: Define canonical text normalization

Back to [[plans/01-document-similarity/overview]]

## Goal

Define canonical text normalization.

## Changes

- src/similarity/normalize.rs
- tests/normalization.rs

## Data structures

- NormalizationVersion.

## Todos

- [ ] Choose and document Unicode normalization form, lowercase semantics, punctuation handling, and whitespace rules.
- [ ] Use the same canonicalization at ingest and check; any change increments signature version.
- [ ] Keep repeated-line removal disabled until corpus evidence supports a deterministic page-aware rule.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Golden normalization fixtures and Unicode/punctuation/empty-text edge cases pass.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.
