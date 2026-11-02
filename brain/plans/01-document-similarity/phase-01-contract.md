# Phase 1: Approve the API and resolve specification conflicts

Back to [[plans/01-document-similarity/overview]]

## Goal

Approve the API and resolve specification conflicts.

## Changes

- docs/api-contract.md
- docs/decisions.md

## Data structures

- BatchMode, BatchState, FileState.

## Todos

- [ ] Confirm corpus size/growth and peak submissions; record S3 provider, credentials delivery, and PostgreSQL deployment details.
- [ ] Approve endpoints, result semantics, limits, client identity, and the unresolved mathematical constraints.
- [ ] Choose exact verification policy, short-document cross-length behavior, and weighted-policy candidate semantics before implementing.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Review example success, partial failure, rejection, conditional polling, and expiration responses.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.
