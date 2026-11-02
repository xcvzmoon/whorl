# Phase 19: Add maintenance and signature lifecycle

Back to [[plans/01-document-similarity/overview]]

## Goal

Add maintenance and signature lifecycle.

## Changes

- src/bin/maintenance.rs
- src/service/maintenance.rs
- tests/maintenance.rs

## Data structures

- RetentionPolicy, ReindexCursor.

## Todos

- [ ] Choose a deployment migration step for application and queue schemas with documented elevated privileges; runtime roles stay restricted.
- [ ] Sweep expired results/job rows after default 24h, terminal objects, orphan objects, and abandoned reservations/work.
- [ ] Re-index stored text in bounded background pages to a new version; queries remain version-isolated and switch only when coverage is ready.
- [ ] Specify irreversible cleanup behavior and do not add backward compatibility layers or migrations for obsolete paths.

## Verification

- Static: rustfmt, Clippy, rustdoc and relevant tests pass when Rust is added; review docs-only changes against the requirements.
- Runtime: Restart sweeps/re-index jobs safely, verify expiry, and prove old/new signatures are never mixed.

## Constraints

- Follow [[principles/small-reviewable-changes]] and [[principles/bounded-resources]].
- Each checkbox is a reviewable step; split format adapters and failure scenarios into separate changes when needed.
- Apply testing and unslop skills; consult current official Rust guidelines before implementation.
