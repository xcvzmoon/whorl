# Phase 9: Retrieve and verify version-compatible candidates

Back to [[plans/01-document-similarity/overview]]

## Goal

Retrieve and verify version-compatible candidates.

## Changes

- src/persistence/documents.rs
- src/service/matching.rs
- tests/candidates.rs

## Data structures

- CandidateRepository, DocumentIdentity.

## Todos

- [ ] Short-circuit normalized content hash duplicates before MinHash; compute metadata score for each upload independently.
- [ ] Query signature_version plus band_hashes && query_bands using GIN; page candidates rather than loading an unbounded corpus.
- [ ] Verify candidates on the CPU pool and order qualifying results by score descending with document id tie-break.
- [ ] Handle concurrent identical ingests with content-hash upsert; define check mode as read-only for corpus documents.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: PostgreSQL integration proves version isolation, duplicate races, query plans, and exclusion of retrieved false positives.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.
