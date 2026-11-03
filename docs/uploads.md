# Multipart submission

The `whorl` binary serves `POST /v1/checks` and `POST /v1/ingests`. Both accept repeated `files` parts and return 202 only after the batch, ordered file records, and graphile_worker jobs commit together. Polling is the next phase; the returned status URL is reserved but not served yet.

Run application and queue migrations first, register the extraction/signature versions as described in [worker setup](worker.md), and configure `capacity_limits` for `global` and each client as described in [admission](admission.md). Start the independent worker to process admitted files. The HTTP binary does not migrate the database.

Required environment:

- `DATABASE_URL`
- `WHORL_S3_ENDPOINT`, `WHORL_S3_BUCKET`, `WHORL_S3_REGION`, and storage credentials from [S3 setup](storage.md)
- `WHORL_SIGNATURE_VERSION`, matching a registered version compatible with worker output
- `WHORL_TRUSTED_PROXY_ONLY=true`, after configuring the authenticated proxy to strip and replace `X-Client-Id` and blocking direct network access

`WHORL_LISTEN` defaults to `127.0.0.1:8080`. Start with `cargo run --locked --bin whorl`. Missing proxy confirmation fails startup. Missing identity returns 401 before reading content. Identity comes from the authenticated proxy, not public client input.

Each request reserves five attempts and 100 MiB before reading content. Upload leases last 120 seconds; the complete submission has a 110-second timeout. Content counters accept exactly 20 MiB per file and 100 MiB combined. A crossing chunk never reaches storage. Oversized files are drained and counted without storage so later siblings survive. Empty files fail individually; unsupported formats fail during the existing content-detecting extraction stage. Filenames are display data only.

The raw body cap is 105,906,176 bytes, including trailing data. Multipart boundaries are at most 70 bytes, preambles and each header block at most 8 KiB, and the parser permits at most 32 headers with a 64 KiB internal buffer. Storage buffers one 5 MiB S3 part. The handler retains at most five file descriptors and never buffers the complete request.

Request rejection rolls back admission and discards completed staged objects. Cancellation schedules cleanup of completed objects and releases unused capacity. Interrupted uploads, process death, or failed deletion retain journaled charges until storage reconciliation; keep the worker maintenance loop running and configure the S3 incomplete-multipart lifecycle rule. Cleanup checks for committed file references under the admission lock, protecting objects if a database commit outcome was ambiguous.

Version one snapshots text-only scoring at 0.75, permits three worker attempts, uses one-hour processing deadlines, retains terminal results and tombstones for 24 hours each, and returns a two-second retry hint. Deployment calibration and broader configuration remain in phase 20.

## Verification

The integration test requires a fresh PostgreSQL database and local MinIO at `127.0.0.1:9000`, using the development `minioadmin` credentials and an existing `uploads` bucket:

```sh
WHORL_UPLOAD_TEST_URL=postgres://postgres@localhost/whorl_upload_test \
  cargo test --locked --test uploads -- --ignored
```

It streams 16 KiB chunks through Actix and exercises exact file/combined limits, oversize draining with a valid sibling, empty/mixed batches, sixth/zero/single-file requests, unknown parts, oversized headers, truncated content, identity rejection, raw-body excess, atomic rejection, injected queue failure, and staged-object cleanup. A separate unit test splits multipart delimiters one byte at a time and checks header/preamble bounds.
