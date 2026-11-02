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

- [x] Choose and document Unicode normalization form, lowercase semantics, punctuation handling, and whitespace rules.
- [x] Use the same canonicalization at ingest and check; any change increments signature version.
- [x] Keep repeated-line removal disabled until corpus evidence supports a deterministic page-aware rule.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Golden normalization fixtures and Unicode/punctuation/empty-text edge cases pass.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.

## Policy and evidence

- Completed November 3, 2026. `NormalizationVersion::V1` is the single CPU-only entry point for future ingest and check callers. Signature versions must include it; phase 7 adds that signature type.
- Apply NFKC to fold compatibility forms such as full-width text and ligatures. Lowercase each scalar without locale or word context, then apply NFKC again. This is lowercase, not case folding: `ß` stays distinct from `ss`, and final sigma stays distinct from sigma.
- Retain Unicode alphanumerics and combining marks so accents and scripts such as Hindi and Arabic survive. Delete punctuation, symbols, and non-whitespace controls. Deletion joins adjacent text (`co-operate` becomes `cooperate`). Collapse Unicode whitespace to one ASCII space and trim its edges. A final NFKC pass composes sequences joined by deletion.
- Preserve repeated lines. Do not infer page boundaries or dehyphenate line breaks. Empty canonical output remains empty for the future extraction pipeline to reject.
- V1 records Unicode 18.0.0 normalization/mark tables and Rust's Unicode 17.0.0 lowercase/alphanumeric/whitespace tables. Golden checks reject table upgrades until a new normalization/signature version is chosen. Any rule change also requires a new version and re-indexing.
- Audited [unicode-normalization](https://crates.io/crates/unicode-normalization) and its [upstream repository](https://github.com/unicode-rs/unicode-normalization). Version 0.1.26 was released October 16, 2026, declares Rust 1.36, and has about 654 million total downloads (136 million recent). Pin `=0.1.26` deliberately because Unicode data affects persisted identity. Its `(MIT OR Apache-2.0) AND Unicode-3.0` license is allowed; the lockfile also pins tinyvec 1.13.3. Cargo deny passes advisories, licenses, and sources; only existing unused license allowances warn.
- Fifteen golden cases cover canonical/compatibility equivalence, marks, punctuation, controls, casing, repeated lines, whitespace, and empty output. Each checks idempotence. A separate test fixes Unicode table versions; the public API doctest passes.
- Formatting, Clippy with warnings denied, all target tests, doctests, warning-free rustdoc, and release build pass on Rust 1.98.1. The unchanged PostgreSQL integration test remains explicitly ignored locally. CI and release checks now run doctests because this phase introduces the library API.
- The extraction experiment remains separate evidence: its NFC-based measurement helper is unchanged. These canonicalization choices make no new OCR quality or recall claim.
