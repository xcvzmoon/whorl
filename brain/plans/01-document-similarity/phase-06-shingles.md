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

- [x] Use word k=3 by default with explicit set semantics.
- [x] Implement the phase 1 approved short-text rule: below configured k words, normalized equality scores 1; unequal short or short/long pairs score 0. Empty extraction fails.
- [x] Encode source-specific OCR policy in versioning and prevent incompatible comparisons.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Check repeated shingles, one/two-word inputs, empty text, and source-policy consistency.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.

## Implementation status

- Implemented November 3, 2026: canonical word shingles with default k=3, configurable nonzero k, unique set membership, empty-text rejection, and symmetric short-text equality.
- `SourceProfile` distinguishes native text from verbatim OCR. Both retain lines and characters before the same normalization and word shingling; no OCR correction, single-character-line filter, or character-shingle fallback is enabled. Source profiles are separate comparison namespaces. This completes the similarity policy without selecting production extraction tools or resolving phase 3 adapter approvals.
- `version_key()` exposes stable normalization/source/k/short-equality policy identity for phase 7 to include alongside hash seeds and band layout. `ensure_compatible()` rejects policy differences for any input length; short-text scoring invokes it automatically. Future exact verification must invoke it before comparing raw shingle sets. Extraction adapters must separately record engine/model revisions.
- Shingles borrow canonical text slices, avoiding copies of overlapping word sequences. Construction and set collection are CPU work for the future bounded worker pool.
- Six integration tests cover repeated triples, Unicode byte boundaries, one/two-word equality, unequal short and short/long pairs, exactly k words, custom k, incompatible k, empty canonical text, k larger than the input, native/OCR incompatibility at every input length, stable policy keys, and preservation of single-character OCR lines.
- `cargo test`, `cargo clippy --all-targets -- -D warnings`, and warning-free `cargo doc --no-deps` pass, including five new compiling doctests. The release build also passes. The existing database integration test remains ignored because it requires disposable PostgreSQL 18.
