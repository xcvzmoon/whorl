# Microsoft Rust Guidelines implementation

Implemented the eight findings from the [November 4 audit](2026-11-04-microsoft-rust-guidelines.md). The original audit describes the earlier tree; this record describes the resulting behavior. Test execution remains local. The verification below was completed before publication; no deployment was performed.

## Changes

| Finding | Resolution |
| --- | --- |
| Panic continuation | One authoritative CPU state permanently poisons admission; a retained notification stops the queue, and the worker exits with failure after draining CPU work |
| Telemetry backpressure | Filtered structured events use a bounded 1,024-line lossy off-thread writer; operational snapshots expose queue-saturation drops |
| Lint coverage | Cargo enables the recommended compiler checks, Clippy categories including pedantic/cargo, and selected safety restrictions; intentional exceptions are documented |
| Public traits | All seven handles implement safe Debug; sensitive document/storage/database state is redacted; SignatureVersion implements canonical Display |
| Module documentation | Crate, processing, similarity, and S3 overviews explain entry points, ownership, and usage through compiling examples |
| Lock protocol values | Named shared constants preserve capacity, migration, upload, and attempt identities; database SQL retains the same capacity-lock value |
| Repeated query preparation | ShingleSet retains sorted unique ranges into canonical text; exact comparison merges them without rebuilding query sets |
| Per-result inserts | Typed-array UNNEST writes bounded chunks of 512 results in the existing final transaction, without truncation or partial commit |

## Panic and recovery behavior

[CpuPool](../../src/cpu_pool.rs) rejects work after a caught panic. Running contexts see the same stopped state. The panic marker is authoritative before the notification is published, and normal shutdown cannot clear poisoning. `failed()` retains the notification for late waiters; `has_failed()` reads the state directly.

The [pipeline](../../src/service/process_file.rs) does not turn poison-related failures into terminal file outcomes. The [worker](../../src/bin/worker.rs) connects the failure notification to Graphile's shutdown mechanism, drains CPU work, and returns failure when the pool is poisoned. Supervision must restart it. Normal extraction failures retain their per-file handling.

Regression coverage includes a test that failed before the fix, rejection after panic, visibility before notification, poison surviving shutdown, and real PostgreSQL queue shutdown with job-lock release/retry preservation. The queue test uses a controlled CPU-panicking handler; it does not inject a panic into ProcessFile or the production executable. Executable signal/error wiring was reviewed rather than introducing a production panic-injection switch.

## Diagnostics and dependency choices

[Logging](../../src/logging.rs) uses tracing 0.1.44, tracing-subscriber 0.3.23, and tracing-appender 0.2.5. Tracing was already in the resolved graph; the subscriber and appender provide filtering, JSON formatting, a bounded writer, and shutdown handling rather than a custom queue implementation. These versions are pinned and their dependency/license audit passes.

Executable startup installs the subscriber after local dotenv loading and retains its guard until runtime shutdown. `RUST_LOG` defaults to `info`; CPU admission events are debug-level. Queue saturation drops new diagnostics rather than blocking processing. `telemetry_dropped_lines` counts saturation losses per process; it does not report sink write failures. Shutdown makes a bounded flush attempt, so delivery is best effort.

Tests cover a blocked sink, failed writes during unwinding, filtering/JSON fields, and dropping the guard while the sink is stalled. Intentional CLI/protocol stdout remains separate. Default library use does not install a global subscriber.

A local release microbenchmark ran 2,000 sequential CPU admissions per sample, five samples. With filtering off, median elapsed time was 13,532 microseconds; with debug admission events enabled and stderr discarded to a fast sink, it was 22,358 microseconds. Both reported zero dropped lines. This measures diagnostic cost on tiny CPU operations under development-host load, not document processing or collector backpressure. The blocked-sink regressions establish the separate backpressure behavior.

## Performance evidence

All values below are five-sample medians from local development measurements. They establish the reason for these changes, not production capacity. The verification figures use the quiet paired run; earlier contention-affected results were discarded.

| Verification workload | Earlier implementation | Prepared ranges |
| --- | ---: | ---: |
| 32 candidates, comparison only | 1,884 µs | 182 µs |
| 32 candidates, candidate preparation and comparison | 14,173 µs | 12,669 µs |
| 1,000 candidates, comparison only | 58,401 µs | 6,059 µs |
| 1,000 candidates, candidate preparation and comparison | 433,310 µs | 403,762 µs |

Each document contains 1,000 generated words. Prepared ranges borrow canonical text, with no self-references or duplicate retained shingle strings. Candidate preparation is included separately so faster comparison does not conceal a slower complete path. No allocation-count or peak-memory measurement was made.

| Persisted results | Individual inserts | 512-row chunks |
| --- | ---: | ---: |
| 32 | 5,617 µs | 1,444 µs |
| 1,000 | 119,127 µs | 54,617 µs |
| 10,000 | 1,966,286 µs | 1,093,707 µs |

The persistence benchmark uses PostgreSQL 18.6 over loopback with the actual schema, foreign keys, and revision triggers. The largest measured case at 1,024 rows per chunk took 1,044,317 µs; 512 was selected to halve temporary array bounds for a small difference in that case. The helper reuses arrays across chunks. It does not cap total results or change caller transaction ownership.

Regression coverage crosses three chunks and checks exact f64/null metadata round trips, client/file/document scoping, precommit invisibility, explicit rollback, existing duplicates, and a duplicate in a later chunk rolling back all prior writes. Golden vectors, Unicode/duplicate shingles, short-text equality, source separation, and threshold tests preserve scoring behavior.

Reproduce CPU measurements with `cargo bench --locked --bench verification`. Set `WHORL_PERSISTENCE_BENCH_URL` to a fresh disposable database before `cargo bench --locked --bench persistence`; the benchmark applies the schema. Without that setting it explicitly skips, so ordinary all-target checks do not require a database. Use `cargo bench --locked --bench telemetry` with `RUST_LOG=off` or `debug` to measure diagnostic admission cost; route stderr to the sink being evaluated.

## Lint policy and compatibility

The compiler checks include missing Debug, redundant imports/lifetimes, unsafe operations, and numeric casts. Clippy enables correctness, complexity, performance, style, suspicious, pedantic, and cargo groups, plus explicit unsafe/documentation restrictions.

Numeric conversions now use checked conversions where useful. Narrow `#[expect]` annotations preserve intentional low-32-bit hash encoding, Jaccard floating-point conversion, exact-score regression comparisons, case-sensitive OOXML identifiers, and cohesive lifecycle/scenario functions. Multiple upstream major versions are the documented Cargo-level exception and remain covered by dependency auditing. This avoids changing persisted hashes, thresholds, or dependencies merely to silence a style warning.

Optional audit observations about allocator selection, deployment CPU flags, a shared research workspace, MSRV support, and canonical error redesign did not demonstrate defects. They remain decisions requiring workload/support constraints; no new allocator, speculative crate split, or compatibility layer was added.

## Final verification

Verified locally on November 4, 2026:

- macOS: 66 tests passed; 16 service-dependent checks were reserved for the integration runner. All 66 documentation examples passed.
- Linux release: 70 tests passed, followed by all 16 PostgreSQL/MinIO integration checks with no skips. The container was limited to two CPUs and 4 GiB of memory. This includes real extraction and HTTP admission through processing to polling.
- Strict all-target, all-feature Clippy passed on macOS and Linux. Formatting, warning-denied rustdoc, and whitespace checks passed.
- Dependency advisories, licenses, and sources passed cargo-deny. Its existing warnings remain: nom-exif lacks a manifest license field, and the BSD-2-Clause allowance is unused.
- All 201 local documentation links resolved, and the history wording check passed.

These checks were run locally; CI and release workflows still do not execute tests. Temporary verification services were stopped afterward.
