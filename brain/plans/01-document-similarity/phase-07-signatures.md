# Phase 7: Implement stable signatures and banding

Back to [[plans/01-document-similarity/overview]]

## Goal

Implement stable signatures and banding.

## Changes

- src/similarity/signature.rs
- tests/signatures.rs
- benches/signatures.rs

## Data structures

- SignatureVersion, MinHashSignature, BandHashes.

## Todos

- [x] Use deterministic persisted hashes and fixed seeds; never DefaultHasher or randomized hashing.
- [x] Generate 125 values and 25 bands of five rows with the band index mixed into each 32-bit hash.
- [x] Commit golden vectors covering normalization, shingle k, seeds, hash encoding, and band layout.
- [x] Record theoretical retrieval probability 1-(1-s^5)^25 and empirically test known-Jaccard pairs near 0.75 and 0.3.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Golden vectors remain stable; seeded recall trials use statistical bounds; benchmark signing by size.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.

## Completion evidence

- Completed November 3, 2026. `MinHashSignature` binds 125 rows to a `SignatureVersion`; `BandHashes` contains 25 indexed five-row bands. Short inputs return `None` and retain content-hash retrieval with exact equality.
- V1 uses pinned `xxhash-rust` 0.8.19 XXH3-64, truncating to low 32 bits. Row seed r is `0x9e3779b97f4a7c15 * (r + 1)` modulo 2^64. Band input is its u32 index followed by five u32 rows, all little endian; band seed is `0x6a09e667f3bcc909`. Any hash, seed, encoding, normalization, source, k, or layout change requires a new version.
- Full golden rows and bands in `tests/vectors/` cover Unicode normalization, k=1/k=3, and a single shingle. Tests also cover set order/duplicates, native/OCR namespace separation, short inputs, band-index separation with identical rows, and PostgreSQL signed-bit round trips.
- Deterministic trials use 2,000 disjoint token namespaces per similarity, k=1, and assert exact set intersection/union sizes. At Jaccard 0.75, 1,997 pairs retrieved; theory `1-(1-s^5)^25` gives 0.99885508. At 0.3, 127 pairs retrieved; theory gives 0.05901110. Both fall within six binomial standard errors plus 0.005. Row agreement is also within 0.01 of Jaccard. These are synthetic checks, not customer-workload recall evidence.
- `cargo bench --locked --bench signatures` on Apple M1 arm64, Rust 1.98.1, release build, ten iterations per size: 100 words 34.041 µs; 1,000 words 334.254 µs; 10,000 words 3.321 ms; 100,000 words 33.197 ms. Measures signing, unique-shingle collection, and banding after normalization. This is a local baseline, not the two-core deployment acceptance measurement.
- Dependency audit: [crate documentation](https://docs.rs/xxhash-rust/0.8.19/xxhash_rust/xxh3/fn.xxh3_64_with_seed.html), `cargo info`, and [repository](https://github.com/DoumanAsh/xxhash-rust) checked. Repository was active September 28, 2026, unarchived, with three open issues. BSL-1.0 is explicitly allowed; crate has no declared MSRV and compiles on the project's Rust 1.98.1. crates.io API statistics were unavailable (HTTP 403), so no download-based claim is made.
- Verification: formatting, Clippy with warnings denied, all-target tests, doctests, warning-free rustdoc, release benchmark, and `cargo deny check advisories licenses sources` pass. The existing PostgreSQL integration test remains ignored without its disposable database; no schema or SQL behavior changed. Cargo deny reports only existing unused license allowances.
- Consulted the current [Microsoft Rust guidelines](https://microsoft.github.io/rust-guidelines/) and applied unslop to public rustdoc. Quality review found no blocking scope or correctness issues.
