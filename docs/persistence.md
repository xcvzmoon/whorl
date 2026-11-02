# Application persistence

Phase 4 defines the schema in `migrations/0001_application.sql`. Apply it once to a fresh database with `psql -X -v ON_ERROR_STOP=1 "$DATABASE_URL" -f migrations/0001_application.sql`. The migration is transactional and separate from any future queue library migrations. Application deployment and its migration runner come in later phases.

Tested with PostgreSQL 18.6, Docker image `postgres:18.6@sha256:3a4264520e121176619f54ff867d12774bc81e95e7b84db58d63bccdb6144b81`. PostgreSQL 18 is the latest stable major in the [official version policy](https://www.postgresql.org/support/versioning/) checked November 3, 2026. No Rust dependencies were added.

## Records and boundaries

- `documents` holds the canonical normalized text and first-ingest metadata. A 32-byte content hash is unique within a client. Upload provenance lives separately in `batch_files`; deleting a batch does not delete corpus documents.
- `signature_versions` records the algorithm configuration. `signatures` holds exactly 125 signature integers and 25 band hashes per document/version. Versions coexist for background re-indexing. Queries must select one version and scope documents to their client.
- `src/persistence/mod.rs` preserves each unsigned hash's 32-bit pattern in PostgreSQL signed integers. For example, `0xffffffff` becomes `-1` and `0x80000000` becomes `-2147483648`. This conversion is reversible and independent of host endianness.
- `batches`, `batch_files`, and `match_results` are authoritative status records, independent of queue rows. Composite foreign keys prevent cross-client document/result references. Application queries must still enforce owner identity.
- Batch updates and file/result mutations increment the batch revision transactionally. The worker will maintain aggregate states and set terminal/expiration/tombstone timestamps together. File records retain ordering, retry attempts, deadlines, storage references, and upload metadata.
- Metadata uses a JSONB path-ops GIN index and a separate token GIN index. Band hashes use an integer-array GIN index. Match results have an index for score-descending, document-id ordering.

## Capacity reservations

Configure `capacity_limits` with `global` and `client:<identity>` rows before admission. Quotas have no invented production defaults. Call `reserve_capacity` inside the same READ COMMITTED transaction that durably admits work. It returns false when either job or byte capacity is exhausted, and true after inserting the reservation. Invalid arguments raise SQLSTATE `22023`; another isolation level raises `25001`; absent quota rows raise `P0002`. Duplicate reservation ids raise `23505` and do not create another reservation. Rollbacks release the transaction lock and undo reservations.

A fixed advisory transaction lock serializes admission across instances; limits rows are also locked while checked. All admissions must use this function. Application roles and migration privileges must be configured at deployment; this schema does not grant untrusted callers database access.

Lease expiry identifies abandoned work but does not free its accounting. Cleanup must confirm that work has stopped and staged objects have been removed before deleting a reservation. Renew leases while work owns the reservation. Reductions/releases and object cleanup are implemented with the storage/admission phases; do not delete reservations merely because their clock deadline passed. This avoids admitting new bytes while abandoned objects still occupy storage.

## Verification

Install `psql` and create a disposable, empty PostgreSQL 18 database. The integration test creates application tables and retains its evidence in that database; use a new database for each run.

```sh
WHORL_SCHEMA_TEST_URL=postgresql://postgres@localhost:5432/whorl_schema_test \
  cargo test --locked --test schema -- --include-ignored
```

CI supplies a fresh pinned PostgreSQL service and explicitly runs the database test. Ordinary `cargo test` runs the hash mapping test and skips the database test. The database test checks migration application, indexes, canonical uniqueness, client boundaries, version coexistence, array constraints, status/retention constraints, revisions, reservation rollback, global/client byte/job limits, twelve competing admissions, and lease cleanup accounting.

Production database deployment remains separate: persistent storage, TLS/connection settings, credential delivery, and migration-role ownership still need deployment-specific values.
