# Phase 8: Implement the similarity interface and metadata policy

Back to [[plans/01-document-similarity/overview]]

## Goal

Implement the similarity interface and metadata policy.

## Changes

- src/similarity/mod.rs
- src/similarity/verify.rs
- tests/verification.rs

## Data structures

- SimilarityEngine, SimilarityPolicy, MatchScore.

## Todos

- [ ] Expose a small CPU-only interface testable without HTTP/database.
- [ ] Use strict score > configurable 0.75; standard error at threshold is sqrt(0.75*0.25/125), about 0.0387.
- [ ] Exactly verify every retrieved candidate as approved in phase 1; use estimates only to order work, never for acceptance or rejection.
- [ ] Flatten metadata into deterministic key=value tokens; exclude creation/modification dates and size; define nested arrays/nulls and empty-set behavior.
- [ ] Default text-only qualification; return metadata alongside. Implement the approved weighted formula and null handling in docs/decisions.md; validate and snapshot the configured weight.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Properties cover identical/disjoint texts and small/large edits; check exact threshold equality and metadata exclusions.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.
