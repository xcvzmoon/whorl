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

- [x] Resolve same-client normalized content hash duplicates before MinHash for that pair, without skipping other candidates; preserve upload provenance and score against first-ingest canonical metadata.
- [x] Query client identity and signature_version plus band_hashes && query_bands using GIN; page candidates rather than loading an unbounded corpus. Weighted mode unions metadata-token intersection candidates as approved in phase 1.
- [x] Verify candidates on the CPU pool and order qualifying results by score descending with document id tie-break.
- [x] Handle concurrent identical ingests with content-hash upsert; define check mode as read-only for corpus documents.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: PostgreSQL integration proves version isolation, duplicate races, query plans, and exclusion of retrieved false positives.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.

## Completion evidence

- Completed November 3, 2026. Application queries use SQLx 0.9.0 with pooled PostgreSQL connections; check mode holds a read-only repeatable-read snapshot.
- Exact-content lookup precedes band/metadata lookup. UUID keyset pages contain at most 32 candidates. A SQL union deduplicates overlapping retrieval paths before exact verification; canonical duplicates are excluded from later pages.
- Short text has a signature-version row with both arrays null. Long-text rows retain the existing array constraints. No upgrade migration or compatibility path was added.
- The shared matcher runs verification and final ordering on two dedicated Rayon threads, admits one request without a waiting queue, and retains admission while cancelled CPU work finishes. Phase 11 extends this executor for the full extraction/signing pipeline; it must not add a second independent CPU pool.
- PostgreSQL 18.6 integration passed: client/version isolation, canonical first-ingest metadata, preserved per-upload provenance, eight concurrent duplicate ingests, retrieved false-positive exclusion, read-only corpus snapshots, short-text lookup, weighted union deduplication, 70 metadata-only matches across three pages, deterministic ties, and check/foreign upload rejection.
- EXPLAIN with 10,000 synthetic background documents and refreshed statistics selected both `signatures_bands` and `documents_metadata_tokens` GIN indexes without disabling sequential scans. This proves index use for that fixture, not production throughput.
- CI explicitly runs the candidate integration test in a separate fresh database. Runtime and dependency details are in `docs/persistence.md`.
