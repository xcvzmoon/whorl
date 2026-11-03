# Maintenance and signature lifecycle

Run `cargo run --bin maintenance -- migrate` with `DATABASE_URL` set to the
schema owner before starting replicas. It serializes application upgrades,
records versions in `whorl_migrations`, and invokes the pinned queue library's
migrations in `whorl_queue`. A retry resumes after either schema's last commit.
Existing development databases created directly from SQL must be recreated;
there is no adoption or compatibility path.

The deployment role needs database CREATE on first installation and ownership
of both schemas and their tables/functions on upgrades. Use a separate runtime
role with schema USAGE, application table SELECT/INSERT/UPDATE/DELETE and
EXECUTE on application and queue functions, plus the queue table privileges
required by graphile_worker. Do not grant database or schema CREATE, ownership,
or membership in the deployment role. Restrict PUBLIC schema creation and
function execution before granting runtime privileges. Worker startup checks
library migrations; deploy them first so runtime never needs DDL rights.

## Cleanup

Schedule `maintenance sweep` every minute using `DATABASE_URL`,
`WHORL_S3_ENDPOINT`, `WHORL_S3_BUCKET`, `WHORL_S3_REGION`, and the same S3
credentials as workers. Each invocation handles at most 32 entries per class.
Run further passes when backlog exceeds this allowance. Alert on failed runs.
The command returns failure on database or object-store errors; retry it.

Expired work is failed only after acquiring the worker's advisory lock. A live
worker retains ownership until it finishes or disconnects. Terminal batches
keep results for 24 hours by default, then retain an empty tombstone for another
24 hours. Completion deadlines already recorded by workers remain authoritative.
File deletion cascades to results. Tombstone expiry deletes the batch. Journaled
terminal uploads and abandoned uploads are deleted through existing storage
reconciliation; deletion releases capacity only after successful S3 cleanup.
Expired request reservations without upload journals are reclaimed separately.
Unlocked queue jobs for terminal or removed files are completed through the
library API after 24 hours. The selection uses the pinned library payload table
to identify the file even if pending-key deduplication has cleared its key.
Keep the bucket's incomplete multipart abort lifecycle rule enabled to cover a
crash between multipart creation and journal update. Use a service-owned bucket;
untracked externally created keys are outside the journal's ownership boundary.

Cleanup is irreversible. Results, upload provenance, source objects, and expired
job records cannot be recovered through the API. Canonical corpus documents and
their stored text remain. No obsolete schema or signature compatibility layer is
created. A database backup alone cannot recover deleted S3 content.

## Reindex and switch

Run `maintenance reindex native 4` to build the supported four-word policy from
stored canonical text, or `maintenance reindex ocr 4` for the OCR namespace.
Choose an approved positive shingle length. Each call indexes at most 32 missing
documents, one at a time on a blocking CPU thread, fetching only one text at a
time. Repeat until `processed=0`. The missing-signature query is the durable
cursor: cancellation or restart skips committed documents. Conflicting concurrent
inserts are harmless. Existing source signatures determine namespace membership;
reindexing never guesses OCR provenance from a file extension.

The command prints the complete target version. Candidate lookup always filters
by the batch's full version, so old and new rows cannot enter the same candidate
set. Short documents receive a row with NULL signature arrays and continue to
use exact equality. This command supports the current normalizer/hash algorithm;
a future algorithm requires an implementation before it can be selected.

For cutover, pause new submissions and drain old-version ingest batches. Run the
final reindex passes for both source profiles. Check coverage for each target:

```sql
SELECT count(*) AS missing
FROM documents d
WHERE EXISTS (
  SELECT FROM signatures s WHERE s.document_id=d.id
  AND split_part(s.version,'/',2)='native-text-v1'
) AND NOT EXISTS (
  SELECT FROM signatures s WHERE s.document_id=d.id AND s.version='<target>'
);
```

Require zero missing rows while ingest remains paused. Set
`WHORL_SIGNATURE_VERSION` to the new native version on every API replica, restart
them, and resume submissions. Workers derive the shingle length from the batch
snapshot. Retain old rows while old-version checks remain; after all old batches
expire, delete obsolete signatures and then their unreferenced version records
under an explicit operator cleanup. Do not relabel existing signature arrays.

## Verification

Use a fresh disposable PostgreSQL database:

```sh
WHORL_MAINTENANCE_TEST_URL=postgresql://localhost/whorl_maintenance_test \
  cargo test --test maintenance -- --ignored
```

The test covers deadline ownership, terminal queue cleanup, reservation expiry,
result expiry, tombstones, source namespaces, short text, page bounds, and restart.
The deployment command was also run twice against a separate fresh database.
Storage deletion uses the existing [storage integration tests](storage.md);
this phase did not run MinIO locally.
