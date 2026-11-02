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

- [ ] Use deterministic persisted hashes and fixed seeds; never DefaultHasher or randomized hashing.
- [ ] Generate 125 values and 25 bands of five rows with the band index mixed into each 32-bit hash.
- [ ] Commit golden vectors covering normalization, shingle k, seeds, hash encoding, and band layout.
- [ ] Record theoretical retrieval probability 1-(1-s^5)^25 and empirically test known-Jaccard pairs near 0.75 and 0.3.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Golden vectors remain stable; seeded recall trials use statistical bounds; benchmark signing by size.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.
