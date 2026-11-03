# Shared upload storage

Phase 12 implements `UploadStore` over an existing PostgreSQL pool and a shared MinIO bucket. It does not deploy storage or add HTTP routes. Apply `migrations/0001_application.sql` followed by `migrations/0002_uploads.sql` through the migration owner.

## Client selection

Selected [object_store 0.14.2](https://docs.rs/object_store/0.14.2/object_store/) with the AWS feature and no filesystem feature. Its low-level multipart API exposes upload IDs, explicit aborts, and sequential part writes; the S3 builder supports custom endpoints and path-style requests for MinIO. This avoids implementing signing, retries, or XML errors. [rust-s3 0.38.0](https://docs.rs/rust-s3/0.38.0/s3/) also supports custom endpoints and multipart operations; object_store's separate multipart and read interfaces fit the durable journal directly. The much larger AWS SDK is unnecessary for this storage-only interface.

Audit on November 3, 2026: object_store is Apache-2.0/MIT, MSRV 1.85, maintained under Apache Arrow governance. Crates.io reports 104.9 million total downloads and a September 15 release. Its repository was active November 1, with 173 open issues/PRs and recent closures [#898](https://github.com/apache/arrow-rs-object-store/pull/898) and [#897](https://github.com/apache/arrow-rs-object-store/issues/897). This shows activity, not a response-time guarantee. The exact version is pinned. Bytes reuses the existing transitive buffer library; Tokio adds I/O utilities and UUID adds secure random v4 identities. TLS uses the client's Rustls backend. `CDLA-Permissive-2.0` is allowed for the transitive Mozilla root-certificate data; it permits redistribution with its license notice. Dependency advisories, licenses, and registry sources are checked with cargo-deny.

## Upload and cleanup contract

- Construct one store per SQLx pool, then clone it. Use at least two connections; upload/reconciliation admission uses at most half the pool size so journal writes have a connection. Other application work must not retain pool connections while waiting for storage admission.
- Configure `capacity_limits` for `global` and each `client:<identity>`. Missing limits fail closed. Reserve one job and the full 20,971,520-byte allowance before reading any input. This conservative charge remains until object deletion, including after lease expiration; actual bytes are recorded separately. HTTP request totals and capacity calibration belong to phases 16–17.
- Keys are random UUIDs under `uploads/`. The database records owner, reservation, multipart ID, actual completed bytes and expiry. Job references contain only identity, key, length and expiry.
- One 5 MiB part is filled and sent at a time. Reject empty or oversized input. This layer stops reading on rejection; HTTP must perform the approved bounded draining and request counting. The caller applies an upload/request deadline; cancellation retains durable cleanup work.
- An advisory transaction lock on `hashtextextended(object_key, 0)` protects active uploads, including during S3 completion and journal updates. Admission must acquire the same lock, validate same-client ownership, completed bytes and a future lease, then attach the key to `batch_files` in that transaction. Never admit a key based only on caller-provided reference fields.
- Run `reconcile_one` repeatedly in a bounded maintenance loop. It skips active writers, rechecks eligibility after acquiring the lock, aborts recorded multipart IDs, deletes objects, then atomically deletes the journal and capacity reservation. Failed S3 deletion or abort retains the charged reservation. Queued and running batch-file references prevent deletion. Terminal file references preserve provenance but allow source deletion after the upload expires.
- A crash after object completion but before database admission leaves a journal entry that can be deleted safely after expiry. A crash after multipart creation but before its ID is journaled can leave an unknown incomplete upload. The bucket must have an `AbortIncompleteMultipartUpload` lifecycle rule; use one day unless deployment policy requires a shorter supported interval. Lifecycle cleanup covers only this unavoidable S3/database gap; application reconciliation handles known IDs and completed objects. Do not use whole-object age expiry that could delete referenced uploads.

## Deployment inputs

Production endpoint, region, bucket ownership, credentials and TLS policy still require deployment-specific values. `storage::s3::connect` reads the client's standard AWS environment credential configuration; deliver those values through the deployment secret manager. Require HTTPS in production and grant only the service's bucket/prefix read, write, delete and multipart permissions. Never serialize the client or credentials into a job. Bucket creation and lifecycle management are deployment responsibilities.

## Runtime verification

The integration test requires a fresh disposable PostgreSQL 18 database, a local MinIO endpoint, and an existing `uploads` bucket. It uses MinIO's default local-only test credentials, never production credentials:

```sh
WHORL_STORAGE_TEST_URL=postgresql://postgres@127.0.0.1:55439/whorl_storage \
WHORL_STORAGE_S3_ENDPOINT=http://127.0.0.1:19039 \
  cargo test --locked --test storage -- --ignored
```

The test covers independent-client reads, full-size multipart uploads, empty/oversized rejection, quota refusal, interrupted multipart recovery, active-writer exclusion, queued/running file protection, terminal-source cleanup, S3 failure accounting and idempotent reconciliation. Local verification used PostgreSQL 18.6 and MinIO RELEASE.2025-10-15T17-29-55Z. Memory is bounded structurally by a fixed part buffer and sequential part submission, independent of stream length; this is not a deployment-wide RSS claim.
