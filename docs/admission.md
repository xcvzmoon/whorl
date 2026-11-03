# Request admission

`AdmissionLease::reserve` runs before reading multipart content. The caller must confirm the approved forwarding contract: an authenticated proxy strips and replaces `X-Client-Id`, and network policy blocks direct API access. Passing an arbitrary public header does not establish identity. Missing identity produces `Identity` (401, `client_identity_required`); an unconfirmed forwarding contract fails closed.

Configure `capacity_limits` for `global` and each `client:<identity>`. Missing rows fail closed. Set each client below global capacity so a single client cannot occupy the service. Values require deployment workload calibration; the public corpus supplies no arrival-rate evidence. Each request explicitly supplies its maximum file attempts, content bytes (at most 104,857,600), upload lifetime and positive retry interval. No production quota defaults are inferred.

Reservations serialize through the existing PostgreSQL advisory lock under READ COMMITTED. Global job exhaustion produces `Unavailable` (503, `queue_full`), global byte exhaustion produces 503 `pending_bytes_exceeded`, and client exhaustion produces `ClientQuota` (429, `client_quota_exceeded`). Both capacity errors carry required Retry-After seconds. Phase 17 wires these outcomes into HTTP responses before body consumption.

Use `UploadStore::upload_admitted` for each file. It atomically transfers one attempt and up to 20,971,520 bytes from the request to a journaled object before reading the stream. Successful uploads return unused bytes to the request and retain their actual byte charge. File attempts cannot be reused. Empty, oversized and failed streams trigger storage cleanup; failed attempts conservatively consume their transferred allowance, and cleanup failures retain the durable object charge. HTTP must count and bound drained content, including rejected files, and enforce request deadlines.

Call `release` on every request exit path; it is idempotent and releases only unused capacity. Cancellation or process death leaves a finite lease. Each subsequent reservation deletes expired request budgets, while stored-upload reservations remain charged until S3 reconciliation succeeds. A fully consumed lease may contain zero jobs and bytes until release or expiry; `reserve_capacity` still rejects new empty reservations. The application schema allows this consumed state directly, with no compatibility migration.

# Verification

PostgreSQL 18 and local MinIO integration tests exercise concurrent client/global reservations, client isolation, byte/job rejection codes and retry hints, expiry reclamation, repeated release, file-attempt exhaustion, exact byte reconciliation, oversized-stream cleanup and retained object charges after request release. The existing storage recovery suite also covers cancellation, failed cleanup and active-writer exclusion.

Use fresh disposable databases and an existing `uploads` bucket:

```sh
WHORL_ADMISSION_TEST_URL=postgres://postgres@localhost/admission \
WHORL_ADMISSION_UPLOAD_TEST_URL=postgres://postgres@localhost/admission_upload \
WHORL_STORAGE_S3_ENDPOINT=http://localhost:9000 \
  cargo test --locked --test admission -- --ignored
```
