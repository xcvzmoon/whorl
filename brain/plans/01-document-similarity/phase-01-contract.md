# Phase 1: Approve the API and resolve specification conflicts

Back to [[plans/01-document-similarity/overview]]

## Status

- Completed November 3, 2026. The user approved the API contract and recommendations, then confirmed continuation after clarification of their scope.
- Confirmed MinIO, Docker-hosted PostgreSQL, and the two-core, 4-GiB target.
- Workload estimates remain unknown; phase 16 calibrates admission and phase 21 establishes capacity. Secret delivery belongs to phase 12; database deployment details belong to phase 4. See [decision gates](../../../docs/decisions.md).
- Documentation verification: JSON examples parse, local Markdown links resolve, and `git diff --check` passes. No Rust changes or runtime claims.

## Goal

Approve the API and resolve specification conflicts.

## Changes

- docs/api-contract.md
- docs/decisions.md

## Data structures

- BatchMode, BatchState, FileState.

## Todos

- [x] Record confirmed deployment constraints and explicitly assign unconfirmed workload, secret delivery, and deployment details to their implementation gates.
- [x] Approve endpoints, result semantics, limits, client identity, and the unresolved mathematical constraints.
- [x] Choose exact verification policy, short-document cross-length behavior, and weighted-policy candidate semantics before implementing.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Review example success, partial failure, rejection, conditional polling, and expiration responses.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.
