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

- [x] Expose a small CPU-only interface testable without HTTP/database.
- [x] Use strict score > configurable 0.75; standard error at threshold is sqrt(0.75*0.25/125), about 0.0387.
- [x] Exactly verify every retrieved candidate as approved in phase 1; use estimates only to order work, never for acceptance or rejection.
- [x] Flatten metadata into deterministic key=value tokens; exclude creation/modification dates and size; define nested arrays/nulls and empty-set behavior.
- [x] Default text-only qualification; return metadata alongside. Implement the approved weighted formula and null handling in docs/decisions.md; validate and snapshot the configured weight.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Properties cover identical/disjoint texts and small/large edits; check exact threshold equality and metadata exclusions.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.

## Policy and evidence

- Completed November 3, 2026. `SimilarityEngine` verifies compatible `ShingleSet` pairs without HTTP, database access, or MinHash estimates. It returns exact component scores and a strict, unrounded threshold decision. Callers must run this synchronous work on the bounded CPU pool introduced in phase 11.
- `SimilarityPolicy` validates finite thresholds and optional text weights in [0,1]. Its default is text-only with threshold 0.75. Copying the immutable policy into an engine snapshots effective settings. Weighted mode applies the approved formula without a separate text gate; absent or undefined metadata contributes zero.
- `MetadataTokens` v1 emits case-sensitive JSON scalar leaves as escaped path=value tokens. Paths escape `~`, `/`, and `=`; JSON serialization distinguishes scalar types. Arrays ignore position and deduplicate repeated leaves, including nested arrays. Nulls and empty containers add no tokens. No string normalization is applied. Root scalars use an empty path.
- At every object level, ASCII case-insensitive `created`, `modified`, `created_at`, `modified_at`, `creation_date`, `modification_date`, `size`, `file_size`, and `size_bytes` exclude their entire subtree. Extractors must map format-specific date/size fields to these canonical names. Candidate indexing must use this same token builder; changing the policy requires rebuilding the metadata index.
- Metadata scoring returns null when disabled or both token sets are empty, zero when exactly one is empty, and exact Jaccard otherwise. Default qualification uses text alone while returning metadata alongside it.
- Added [serde_json](https://crates.io/crates/serde_json/1.0.154) 1.0.154 for its JSON value representation and scalar escaping rather than defining a second JSON model. `cargo info` confirms MIT OR Apache-2.0 and Rust 1.71. The [upstream repository](https://github.com/serde-rs/json) has 1,881 commits, 190 open issues, and 62 pull requests at review; these counts establish activity, not a guarantee of issue response time. Version 1.x follows its stable semver range; Cargo.lock fixes the tested dependency graph. Cargo deny passes advisories, bans, licenses, and sources, with only existing unused license allowances warning.
- Eight integration tests cover exact threshold equality, configuration validation and snapshots, deterministic metadata and exclusions, null/empty/disabled behavior, weighted endpoints, profile rejection, repeated shingles, short equality, symmetry, identical/disjoint texts, and a generated sweep of progressively larger edits. All target tests and 19 doctests pass. The unchanged PostgreSQL test remains explicitly ignored locally.
- Formatting, Clippy with warnings denied, warning-free rustdoc, release build, and `git diff --check` pass. No extraction tool selection or HTTP/worker behavior changed.
