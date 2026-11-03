# Queue adapter

Phase 14 pins graphile_worker 0.14.1, the release evaluated in the [queue spike](queue-spike.md). Its published local sources confirm the transaction executor API and MIT license. The release was rechecked through the cached package; the crates.io API returned HTTP 403 during this phase, so no fresh maintenance claim is made. The lockfile records the added dependency graph. Implementation follows the [Rust guidelines](https://microsoft.github.io/rust-guidelines/).

`JobQueue` uses the fixed `whorl_queue` schema. Deploy `WorkerUtils::migrate` separately with the migration owner before starting API replicas. Construction does not migrate or initialize a worker. Use the runtime grants and RLS policies described in the spike, substituting this schema name.

Admission must insert the batch and valid file rows, then call `enqueue` once per file in the same PostgreSQL transaction. Commit only after all enqueues succeed. On any error or cancellation, roll back everything. The adapter locks each file and stored-upload row, checks tenant ownership, deadlines, queued state and complete uploaded bytes, and schedules `process_file_v1`. Payloads contain only tenant, batch and file ids. The worker loads source bytes, policy and mode from authoritative records. Batch policy validation and capacity admission belong to later phases.

Pending jobs use `file:<uuid>` keys with `PreserveRunAt`. Duplicate submissions retain the pending schedule. These keys do not fence execution; replacing a running job can create a successor. The [worker](worker.md) checks authoritative state, fences attempts and persists results transactionally before acknowledgment. S3 uploads are outside this database transaction and retain their existing orphan-recovery lifecycle.

The attempt budget is required at construction, includes the first attempt, and is capped at 32767 to fit the library's signed field. There is no production default. Tests use three attempts, without approving that value for deployment. Retry scheduling belongs to Graphile, using `exp(min(attempts, 10))` seconds. The configured attempt count bounds delivery retries; deadlines remain authoritative. Extraction failures are terminal. Storage/database failures may retry, with phase 15 responsible for persisting status and stopping at deadline. Do not enqueue a fresh key for a retry or reset an exhausted job's budget. Retained exhausted queue rows are separate from application result retention.

## Verification

Use a fresh disposable PostgreSQL database, with permission to create application and queue schemas:

```sh
WHORL_ENQUEUE_TEST_URL=postgresql://localhost/whorl_enqueue_test \
  cargo test --locked --test enqueue -- --include-ignored
```

The test applies migrations, verifies batch/job rollback after partial submission, concurrent pending-key deduplication, payload identity and attempt limits, tenant/upload/state checks, and rollback after an injected queue-table insertion error. It leaves the database intact for inspection. PostgreSQL 18.6 was used locally. No S3 network service is needed because this phase consumes the completed upload journal rather than uploading bytes.

On November 3, 2026, both enqueue tests passed against PostgreSQL 18.6, including the injected insertion failure. The full all-target/all-feature suite, 40 doctests, formatting, Clippy with warnings denied, and warning-free rustdoc passed. Dependency advisory, license and source checks passed with existing nom-exif license-metadata and unused-allowance warnings. Other opt-in integration tests retain their normal ignored status; this run explicitly enabled the enqueue integration test.
