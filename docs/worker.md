# Durable worker

`worker` runs `process_file_v1` jobs independently of the API. Payloads contain only tenant, batch and file ids. The worker reads source bytes from the shared service bucket and validates the stored upload owner and length before supervised extraction.

## Processing and visibility

- One or two handlers share the same number of dedicated CPU threads. Extraction supervision, normalization, hashing, shingling and exact verification all run through that pool. Extraction itself remains an isolated Linux process. The deployment must enforce the aggregate two-core, 4 GiB and temporary-filesystem limits described in [extraction](extraction.md).
- A transaction-scoped advisory lock prevents overlapping handlers for the same file. The persisted attempt number fences stage changes and final writes if a connection dies while CPU work continues.
- Stage and attempt updates survive process death. A retry restarts extraction; it does not reuse partially computed output. The queue's own attempt budget controls infrastructure retries. Permanent extraction/policy errors become durable file failures and acknowledge the job. The last infrastructure attempt records `retry_exhausted` before acknowledgment.
- Candidate lookup uses the committed corpus snapshot established by `Matcher`. An ingest compares before inserting its own document. Sibling files can see earlier committed ingests; neither batch order nor one shared batch corpus snapshot is promised.
- Matches, canonical ingest and terminal file state commit in one transaction. Repeated terminal delivery acknowledges without recomputing or changing results. Batch completion fixes result expiry and tombstone expiry; the executable uses 24 hours for each.
- The batch signature key snapshots algorithm parameters. Version one derives the native or verbatim-OCR namespace from extraction. All other key components must match, including the shingle length derived from the snapshot. Both source versions must be registered with the canonical configuration for their respective source. The worker validates each independently, so source-specific configuration fields do not prevent OCR processing. Register both with maintenance before admitting mixed batches. Matching never crosses those source namespaces.
- Source keys remain as upload provenance. Expired sources with only terminal file references are reclaimed by the journal reconciler. Queued/running references protect their objects. The executable checks up to 32 uploads each minute; failed cleanup retains the reservation and journal for retry.

A caught CPU panic permanently poisons the shared pool and signals the queue's custom shutdown path. New CPU work is rejected, interrupted jobs remain retryable, and the executable exits with failure after draining CPU work. Supervise the process with restart-on-failure. Normal extraction rejections remain per-file failures and do not request a process restart.

A database outage can prevent the final attempt from recording a terminal result. Operations must inspect exhausted queue rows and reconcile pending application files after recovery. The library provides at-least-once execution, not exactly-once delivery.

## Run

Apply application migrations and run `WorkerUtils::migrate` under the deployment migration role first. The runtime role needs existing queue/application access, not schema ownership. Library initialization checks migrations and fails if it needs privileges it does not have; do not grant runtime schema creation to work around a missing deployment migration.

Build both executables and install them together so `worker` can find the trusted sibling `whorl-extract`:

```sh
cargo build --locked --release --bin worker --bin whorl-extract
DATABASE_URL=postgresql://localhost/whorl \
WHORL_S3_ENDPOINT=https://minio.example.com \
WHORL_S3_BUCKET=uploads WHORL_S3_REGION=us-east-1 \
WHORL_CPU_WORKERS=2 target/release/worker
```

S3 credentials use the existing process-delivered AWS credential configuration. `WHORL_CPU_WORKERS` defaults to two; set one on a single-core host. The CPU pool rejects a count above available parallelism or two. PDFium, OCR assets, native tools and their environment variables remain required as documented in [extraction](extraction.md).

Recovery uses 30-second heartbeats, 60-second sweeps, a five-minute stale threshold and a 30-second retry delay. SIGTERM stops admission and allows five seconds to drain. The library then cancels unfinished handlers and releases their jobs for delayed retry. CPU closures observe cancellation within a 50 ms polling interval during extraction and retain their slots until they exit. The executable drains the CPU pool before exit. SIGKILL cannot run destructors; deployment process/cgroup supervision must also clean surviving extraction descendants.

## Verification

Use a fresh PostgreSQL 18.6 database and generated extraction fixtures:

```sh
WHORL_WORKER_TEST_URL=postgresql://localhost/whorl_worker_test \
  cargo test --locked --test worker -- --ignored --test-threads=1
```

On Linux this exercises real DOCX extraction, check/ingest ordering, canonical duplicates, tenant checks, attempt fencing, immutable terminal results, deadline rejection, permanent extraction rejection, storage retries and exhaustion. Controlled extraction executables additionally exercise mixed native/OCR namespaces, cancellation/restart, graceful queue shutdown/release, healthy long work under competing recovery sweeps, and SIGKILL/restart of a separate queue worker process. The in-memory object-store implementation supplies deterministic shared-storage failure cases; [storage tests](storage.md) separately cover actual MinIO and terminal-source cleanup. macOS exercises database failure/state transitions but cannot establish successful supervised extraction.
