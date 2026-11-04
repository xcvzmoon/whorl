# Application persistence

Application SQL lives in `database/migrations/0001_application.sql` and `0002_uploads.sql`. Deploy with `maintenance migrate`, which records application versions and then applies the queue library migrations. Direct SQL application is reserved for disposable tests; see [migration ownership](maintenance.md).

Tested with PostgreSQL 18.6, Docker image `postgres:18.6@sha256:3a4264520e121176619f54ff867d12774bc81e95e7b84db58d63bccdb6144b81`. PostgreSQL 18 is the latest stable major in the [official version policy](https://www.postgresql.org/support/versioning/) checked November 3, 2026. No Rust dependencies were added.

## Records and boundaries

- `documents` holds the canonical normalized text and first-ingest metadata. A 32-byte content hash is unique within a client. Upload provenance lives separately in `batch_files`; deleting a batch does not delete corpus documents.
- `signature_versions` records the algorithm configuration. `signatures` holds exactly 125 signature integers and 25 band hashes per long document/version. Short text retains version membership with both arrays null; a row cannot have just one null array. Versions coexist for background re-indexing. Queries must select one version and scope documents to their client.
- `src/persistence/mod.rs` preserves each unsigned hash's 32-bit pattern in PostgreSQL signed integers. For example, `0xffffffff` becomes `-1` and `0x80000000` becomes `-2147483648`. This conversion is reversible and independent of host endianness.
- `batches`, `batch_files`, and `match_results` are authoritative status records, independent of queue rows. Composite foreign keys prevent cross-client document/result references. Application queries must still enforce owner identity.
- Batch updates and file/result mutations increment the batch revision transactionally. The worker maintains aggregate states and set terminal/expiration/tombstone timestamps together. File records retain ordering, retry attempts, deadlines, storage references, and upload metadata.
- Metadata uses a JSONB path-ops GIN index and a separate token GIN index. Band hashes use an integer-array GIN index. Match results have an index for score-descending, document-id ordering.

## Capacity reservations

Configure `capacity_limits` with `global` and `client:<identity>` rows before admission. Quotas have no invented production defaults. Call `reserve_capacity` inside the same READ COMMITTED transaction that durably admits work. It returns false when either job or byte capacity is exhausted, and true after inserting the reservation. Invalid arguments raise SQLSTATE `22023`; another isolation level raises `25001`; absent quota rows raise `P0002`. Duplicate reservation ids raise `23505` and do not create another reservation. Rollbacks release the transaction lock and undo reservations.

A fixed advisory transaction lock serializes admission across instances; limits rows are also locked while checked. The SQL function supports schema-level reservation tests. Runtime `AdmissionLease::reserve` implements request reservation and upload-budget semantics under the same shared lock; see [admission](admission.md). Application roles and migration privileges must be configured at deployment; this schema does not grant untrusted callers database access.

Lease expiry identifies abandoned work but does not free its accounting. Cleanup must confirm that work has stopped and staged objects have been removed before deleting a reservation. Renew leases while work owns the reservation. Storage/admission implement reductions, releases, and object cleanup; do not delete reservations merely because their clock deadline passed. This avoids admitting new bytes while abandoned objects still occupy storage.

## Verification

Install `psql` and create a disposable, empty PostgreSQL 18 database. The integration test creates application tables and retains its evidence in that database; use a new database for each run.

```sh
WHORL_SCHEMA_TEST_URL=postgresql://postgres@localhost:5432/whorl_schema_test \
  cargo test --locked --test schema -- --include-ignored
```

Run the database test locally against a fresh PostgreSQL instance; CI does not provision a database or execute tests. Ordinary `cargo test` runs the hash mapping test and skips the database test. The database test checks migration application, indexes, canonical uniqueness, client boundaries, version coexistence, array constraints, status/retention constraints, revisions, reservation rollback, global/client byte/job limits, twelve competing admissions, and lease cleanup accounting.

Production database deployment remains separate: persistent storage, TLS/connection settings, credential delivery, and migration-role ownership still need deployment-specific values.

## Candidate lookup and canonical ingest

Phase 9 adds `PreparedDocument` and `Matcher` in `src/service/matching.rs`, backed by `src/persistence/documents.rs` and its parameterized candidate SQL. Prepare normalized text, SHA-256 identity, shingles, metadata tokens and optional MinHash on a CPU worker. Register `PreparedDocument::signature_version()` before admission. The application owns a bounded SQLx pool and supplies its PostgreSQL connection and TLS settings; this layer does not create another pool.

Each `Matcher` admits one check at a time with no waiting queue. The worker creates a matcher per file, supplying the shared CPU pool. It checks exact normalized-content identity first, then retrieves UUID keyset pages of 32 under the client's signature version. A read-only repeatable-read transaction fixes the corpus snapshot at lookup start. Band overlap and, when metadata weight is positive, token overlap form a SQL union. Every retrieved candidate is exactly verified through the supplied shared CPU pool. False positives are removed; qualifying results are sorted by score descending and UUID ascending on the same pool. Only a page's candidate text/metadata is retained; the returned vector contains all qualifying scores, so result memory still grows with the number of matches. No production corpus capacity claim follows from this fixture.

`PreparedDocument::ingest` resolves the client/content-hash conflict and adds the versioned signature if absent inside the caller's transaction. Conflicts leave first-ingest text, metadata, metadata tokens, type and timestamps intact. Per-upload metadata and object provenance remain on `batch_files`. The method does not mutate file or batch state. The production worker validates identity, attempt ownership, version compatibility, and deadlines, compares first, then commits results, canonical ingestion, file state, and shared lifecycle aggregation together. See [architecture review resolutions](architecture-review.md). A connection failure during commit requires rereading file state before retrying.

Verification uses the shared CPU executor. Cancellation stops new pages but cannot kill an already running Rust computation; that page retains its admission permit until it finishes. Extraction, signing, and verification share bounded admission, deadlines, and shutdown control with at most two CPU workers.

Run the candidate test in its own fresh disposable PostgreSQL 18 database:

```sh
WHORL_CANDIDATES_TEST_URL=postgresql://postgres@localhost:5432/whorl_candidates_test \
  cargo test --locked --test candidates -- --include-ignored
```

The test covers client/version isolation, duplicate lookup without skipping near matches, canonical metadata, unchanged corpus/file/batch snapshots after checks, short-text membership, metadata-only matches across three pages, union deduplication, false positives, deterministic ordering, eight duplicate-ingest races, and unchanged upload provenance. With 10,000 synthetic background documents, `ANALYZE` plus `EXPLAIN` selects both retrieval GIN indexes without planner overrides. The CPU unit test holds both workers and cancels a queued page to prove its admission slot survives until the page finishes.

## Phase 9 dependency audit

Checked November 3, 2026 against crates.io package metadata, repository activity and official API documentation. Exact algorithm and database-library versions are pinned; Tokio and UUID use stable major requirements with exact resolved versions in `Cargo.lock`. SQLx is pre-1.0. Only PostgreSQL, JSON, UUID, Tokio runtime and Rustls with native trust roots are enabled; query macros and other database drivers are not enabled.

| Dependency | Version | Purpose | License | MSRV |
| --- | --- | --- | --- | --- |
| [SQLx](https://crates.io/crates/sqlx/0.9.0) | 0.9.0 | Required pooled async PostgreSQL driver | MIT / Apache-2.0 | 1.94 |
| [Rayon](https://crates.io/crates/rayon/1.12.0) | 1.12.0 | Dedicated two-thread verification executor | MIT / Apache-2.0 | 1.80 |
| [SHA-2](https://crates.io/crates/sha2/0.11.1) | 0.11.1 | Canonical SHA-256 content identity | MIT / Apache-2.0 | 1.88 |
| [Tokio](https://crates.io/crates/tokio/1.54.1) | 1.54.1 | Async semaphore and oneshot delivery; runtime in tests | MIT | 1.85 |
| [UUID](https://crates.io/crates/uuid/1.29.0) | 1.29.0 | Typed corpus and upload identities | MIT / Apache-2.0 | 1.89 |
| [thiserror](https://crates.io/crates/thiserror/2.0.21) | 2.0.21 | Typed errors, including overload and invalid uploads | MIT / Apache-2.0 | 1.77 |

Crates.io reports recent updates and total downloads of 179.5M for SQLx (May 21), 639.3M for Rayon (April 14), 1.16B for SHA-2 (October 12), 1.14B for Tokio (October 21), 945.1M for UUID (October 23), and 1.76B for thiserror (September 23), all in 2026. These are package totals, not adoption counts for the pinned version.

The repository check found commits in October 2026 for all six projects and recently closed issues or pull requests: [SQLx](https://github.com/transact-rs/sqlx/issues/4482), [Rayon](https://github.com/rayon-rs/rayon/pull/1329), [SHA-2](https://github.com/RustCrypto/hashes/pull/939), [Tokio](https://github.com/tokio-rs/tokio/issues/7478), [UUID](https://github.com/uuid-rs/uuid/pull/923), and [thiserror](https://github.com/dtolnay/thiserror/pull/456). Open issue/PR counts were 779, 231, 38, 452, 10 and 32 respectively. Recent closures show maintenance activity; they do not establish a response-time guarantee. SQLx's repository redirects from `launchbadge/sqlx` to `transact-rs/sqlx`.

`cargo deny check advisories licenses sources` passes. PostgreSQL tests used the locally installed 18.6 server, separate from the developer's existing server. Schema and candidate integration tests run locally against fresh databases.
