# Phase 6: Implement word shingles and short-text policy

Back to [[plans/01-document-similarity/overview]]

## Goal

Implement word shingles and short-text policy.

## Changes

- src/similarity/shingles.rs
- tests/shingles.rs

## Data structures

- ShingleSet, SourceProfile.

## Todos

- [ ] Use word k=3 by default with explicit set semantics.
- [ ] Implement the phase 1 approved short-text rule: below configured k words, normalized equality scores 1; unequal short or short/long pairs score 0. Empty extraction fails.
- [ ] Encode source-specific OCR policy in versioning and prevent incompatible comparisons.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Check repeated shingles, one/two-word inputs, empty text, and source-policy consistency.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.
