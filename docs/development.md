# Development and testing

Use stable Rust with rustfmt and Clippy. The manifest enables Microsoft's recommended compiler checks and Clippy categories, including pedantic checks. Narrow `#[expect]` exceptions explain protocol encodings, numeric score conversion, or cohesive scenarios; upstream multiple-major dependencies are the documented graph-level exception. Cargo.lock records the tested dependency resolution; use `--locked` for reproducible checks. Linux is required to establish successful supervised extraction. PostgreSQL 18.6 and MinIO are the recorded integration environment. The [extraction guide](extraction.md) lists native libraries, models, attribution, and runtime isolation requirements.

## Local checks

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
cargo test --locked --doc --all-features
RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps --all-features
cargo build --locked --release --bins
cargo deny check advisories licenses sources
```

Install cargo-deny separately when running its audit locally. The default test command skips tests annotated with external-system requirements. Passing it alone does not establish database, S3, native extraction, or complete HTTP acceptance. Doctests have a separate command because all-target testing does not run them.

## Test map

| Area | Source and coverage |
| --- | --- |
| Public interfaces | `tests/public_interfaces.rs`; Debug availability, document/credential redaction, canonical version Display |
| Match persistence | `tests/match_persistence.rs`; bounded chunks, exact/null score round trips, client scope and rollback |
| Text and similarity | `tests/normalization.rs`, `shingles.rs`, `signatures.rs`, `verification.rs`; Unicode, short text, deterministic vectors, recall simulations, metadata/policy edge cases |
| CPU work | `tests/cpu_pool.rs`; saturation, cancellation, expiration, panic recovery, async responsiveness, draining |
| Schema | `tests/schema.rs`, `schema.sql`; constraints, indexes, revision triggers, capacity races and rollback |
| Candidates | `tests/candidates.rs`; client/version scope, duplicates, metadata unions, paging, GIN plans, canonical races |
| Extraction | `tests/extraction.rs`, internal module tests, extraction corpus; formats, malformed input, expansion, supervision, output limits |
| Shared storage | `tests/storage.rs`; real MinIO, interrupted upload, locks, retained accounting, reconciliation |
| Admission | `tests/admission.rs`; client/global quotas, races, expiry, idempotent release, budget transfer |
| Attachment | `tests/attachment.rs`; lease/source validation, atomic handoff, queue rollback, real S3 reconciliation race |
| Lifecycle | `tests/lifecycle.rs`; shared aggregation, immutable retention, deadline ownership and concurrent completion |
| Queue | `tests/enqueue.rs`; payload/retry rules, atomic writes, concurrent pending-key deduplication |
| Worker | `tests/worker.rs`; real extraction on Linux, retries, deadline/attempt fencing, repeated delivery, cancellation, SIGKILL, shutdown, concurrency |
| HTTP | `tests/uploads.rs`, `polling.rs`; streaming limits, staged cleanup, snapshot results, ETags, identity, expiration |
| Maintenance | `tests/maintenance.rs`; lock protection, deadlines, tombstones, bounded reindex, restart and namespace isolation |
| Operations | `tests/operations.rs`; startup validation/dotenv, durable telemetry and cleanup |
| Acceptance | `tests/end_to_end.rs`; HTTP admission through queue/extraction to durable corpus and polling |

## External-system integration

Use disposable, fresh databases; individual suites create schemas and leave records for inspection. Never point test settings at a production database. The aggregate runner requires a PostgreSQL administrative URL able to create databases, a running MinIO endpoint, credentials, and an existing `uploads` bucket:

```sh
export WHORL_ACCEPTANCE_ADMIN_URL=postgresql://postgres@localhost/postgres
export WHORL_STORAGE_S3_ENDPOINT=http://127.0.0.1:9000
# Supply AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY through your local environment.
bash scripts/acceptance.sh
```

The runner creates a timestamped database for each suite and retains it. It exports per-suite settings, runs ignored tests explicitly, and runs admission database/upload checks separately. On macOS it explicitly skips end-to-end successful extraction; Linux is needed for complete acceptance. Native extraction fixtures/assets must be prepared as described in [extraction](extraction.md) and the [corpus protocol](../spikes/extraction/README.md).

For one suite, use its documented `WHORL_*_TEST_URL` on a fresh database and `cargo test --locked --test <suite> -- --ignored`. S3 suites additionally need their endpoint/bucket/credential settings. Individual guides include exact commands. The acceptance runner is the source of truth for coordinated environment names.

## Measurements

```sh
cargo bench --locked --bench signatures
cargo bench --locked --bench batches
cargo bench --locked --bench verification
```

These measure CPU paths. `verification` separates repeated comparison from candidate preparation. For the database insert benchmark, use a fresh disposable `WHORL_PERSISTENCE_BENCH_URL` and run `cargo bench --locked --bench persistence`; it applies the application schema and compares per-row writes with bounded batches. The live-system load script additionally measures complete batch behavior and health/status responsiveness. Follow [benchmark reproduction](benchmarks.md) for corpus setup, runtime constraints, `scripts/load-acceptance.py`, and interpretation. Preserve the environment and raw output alongside results. Fixture timings do not establish customer-scale capacity.

## CI and releases

[CI](../.github/workflows/ci.yaml) runs formatting, Clippy, warning-free rustdoc, dependency/license/source audits, and release builds. The final CI job requires the checks job to pass. Tests, doctests, corpus verification, and the aggregate integration runner execute locally; CI does not provision PostgreSQL or MinIO. Clippy still checks all targets, including test code, without executing them.

[Release automation](../.github/workflows/) checks a `v*` tag against the Cargo package version and runs the same static, documentation, audit, and build gates before creating a release. It does not execute tests. Repository owner settings govern branch protection and publishing. No application license is declared in the repository; dependency license allowlists do not grant an application license. Preserve the separate fixture/model notices when distributing assets.

## Contributions

Follow [AGENTS.md](../AGENTS.md) and [engineering principles](engineering.md). Keep changes focused and provide evidence appropriate to the affected behavior. Rust public interfaces consumed across crates need accurate, compiling rustdoc examples. For runtime changes, include the relevant external integration verification rather than relying only on the default suite. Read [approved decisions](decisions.md) before changing matching or recovery guarantees.
