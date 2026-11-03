# API contract

Status: approved by the user on November 3, 2026, together with the [contract decisions](decisions.md). Multipart submission and polling are implemented.

## Submission

`POST /v1/checks` compares uploads without adding corpus documents.
`POST /v1/ingests` compares uploads and persists each successful document.
Both accept multipart/form-data with repeated `files` parts (one to five).
Separate URLs make corpus writes explicit; both use one processing pipeline with a stored mode.
The configured policy/version is snapshotted on each batch, so retries use the same rules.
Version one has no request replay support. Each submission creates a new batch; clients must not automatically retry an ambiguous submission. Worker retries remain idempotent. Adding request replay requires a separate contract decision.

Successful durable admission returns `202 Accepted`, `Location: /v1/batches/{id}`, and `Retry-After: 2` (configurable seconds):

```json
{
  "batch_id": "819e3488-25be-4dc1-9b3f-d76e827e7a90",
  "mode": "check",
  "state": "queued",
  "status_url": "/v1/batches/819e3488-25be-4dc1-9b3f-d76e827e7a90",
  "files": [
    {"file_id": "7825e3cb-b431-48cd-b1bd-4fd56e9b646e", "name": "report.pdf", "state": "queued"},
    {"file_id": "9a5c4145-dc8d-427b-95b9-67b9f7dd3769", "name": "empty.pdf", "state": "failed", "error": {"code": "empty_file", "message": "File is empty.", "retryable": false}}
  ]
}
```

Every part gets a stable file id, including rejected parts. A nonempty request with all files individually rejected still creates a terminal failed batch and returns 202, with no jobs. Mixed valid/invalid files retain all individual statuses. Type validation can fail during streaming or extraction. Client filenames are display data, never storage paths.

## Polling and results

`GET /v1/batches/{id}` returns 200 with an ETag derived from a persisted monotonic batch revision, `Cache-Control: no-store`, and `Retry-After: 2` while unfinished. `If-None-Match` for the current representation returns 304 with ETag and the polling hint while unfinished. A revision changes for any represented status, progress, result, or error change.

```json
{
  "batch_id": "819e3488-25be-4dc1-9b3f-d76e827e7a90",
  "mode": "check",
  "state": "completed",
  "signature_version": "v1",
  "policy": {"kind": "text_only", "threshold": 0.75},
  "expires_at": "2026-11-04T00:00:00Z",
  "files": [{
    "file_id": "7825e3cb-b431-48cd-b1bd-4fd56e9b646e",
    "name": "report.pdf",
    "state": "completed",
    "progress": {"stage": "completed"},
    "document_id": null,
    "matches": [{"document_id": "f88d9547-1190-486a-aa92-6936130e4c80", "score": 0.91, "text_similarity": 0.91, "text_score_method": "exact", "metadata_similarity": 0.5}]
  }]
}
```

Scores use 0..1, not percentages. Include all qualifying ids sorted by policy score descending, then document id; no silent top-N truncation. Metadata score is null when disabled. Ingest returns its persisted document_id; check returns null. Text scores use `text_score_method: "exact"`. Version one returns all qualifying retrieved candidates without result pagination or silent truncation; implementation must stream large result representations with bounded memory. Retrieval remains probabilistic as described in the approved decisions.

Batch states: queued (no work started), running (some work started and unfinished), completed (all succeeded), partially_failed (terminal with successes and failures), failed (terminal with no successes). File states: queued, running, completed, failed. Running progress stages: extracting, signing, candidate_lookup, verifying, persisting. Retrying files return to queued with attempt and next_retry_at; status revisions preserve visibility. No percentage is reported without measurable units.

Unknown/inaccessible batches return 404. Expired batches return 410 while a bounded tombstone exists, then 404. Default result retention is 24 hours after terminal completion. No long-polling, SSE, or webhooks in version one. Jobs write results independently of result delivery.

## Errors and admission

Consistent envelope for request errors:

```json
{"error":{"code":"pending_bytes_exceeded","message":"Upload capacity is full.","retryable":true,"request_id":"request-reference"}}
```

| HTTP | Codes / behavior |
| --- | --- |
| 400 | no_files, too_many_files, malformed_multipart, unknown_part |
| 413 | total_too_large, body_too_large; reject whole batch and clean staged objects |
| 429 | client_quota_exceeded; Retry-After required |
| 503 | queue_full, cpu_capacity_exceeded, pending_bytes_exceeded, storage_unavailable; Retry-After required |
| 404 / 410 | batch_not_found / batch_expired |

Per-file failures use the same code/message/retryable fields within the file result, including file_too_large, unsupported_type, empty_file, malformed_file, extraction_limit, extraction_timeout, and no_extractable_text. They do not discard valid sibling files. More than five file parts, combined content over 104,857,600 bytes, and zero files reject the entire request. All received file bytes count toward the total, including rejected files. On an oversized individual file, storage stops at the crossing chunk; bounded draining/counting allows subsequent parts to survive without storing the oversized content.

Per-client admission requires a trustworthy identity supplied by upstream authentication. The identity/forwarding boundary is defined below; arbitrary public headers must not bypass quotas or expose other clients' batches. Rate limits and retry intervals are configuration values to calibrate during the load test.

## Identity and corpus boundary

The API is reachable only through the authenticated upstream proxy. The proxy strips incoming `X-Client-Id` and sets it to a stable, nonempty client identifier. Network policy must prevent direct access to the API. Missing identity is rejected with HTTP 401 and `client_identity_required`; an arbitrary public header is never sufficient authentication.

Batch ownership, polling, quotas, and expiration tombstones are scoped to this identity. The corpus is also client-scoped: checks and ingests compare only against documents belonging to that client. Cross-client document ids are never returned. A shared corpus requires explicit approval of its disclosure boundary.

Each file compares against committed corpus documents visible when its candidate lookup starts. Files in the same batch may observe sibling ingests that have already committed; the batch does not promise a shared corpus snapshot. An ingest compares before persisting its document, so a newly created document cannot match itself; a canonical document that already existed may be returned as an exact duplicate. A check may return an existing identical document. Repeated ingests reuse the canonical id for identical normalized text within the client and preserve each upload's metadata and provenance separately. Metadata similarity uses the metadata of the canonical document's first successful ingest.

## Response invariants

- `BatchMode`: `check` or `ingest`.
- `BatchState`: `queued`, `running`, `completed`, `partially_failed`, or `failed`.
- `FileState`: `queued`, `running`, `completed`, or `failed`.
- Files retain multipart order. Stable ids and original display names appear in every response.
- A batch is `running` once any attempt has started and until all files are terminal, including while retries wait. Admission-time file failures alone do not count as starting work.
- `matches` and `document_id` appear only on completed files; `error` appears only on failed files. A completed file with no matches has `matches: []`.
- `attempt` starts at zero before execution and increments when an attempt starts. `next_retry_at` appears only while a retry is scheduled.
- `expires_at` is null until the batch becomes terminal, then is fixed to terminal time plus the configured retention. Polling does not extend retention.
- Threshold comparison uses the unrounded score and is strictly `score > threshold`. Exactly 0.75 does not qualify at the default threshold.
- Metadata scoring is enabled by default as supporting information. Two empty metadata token sets yield null; one empty set yields zero.
- Expiration takes precedence over conditional polling. A matching ETag cannot return 304 for an expired batch.

## Review examples

The completed example shows a separate one-file submission. The two-file submission above finishes as shown in the mixed result below; a batch containing a failed file cannot become `completed`.

A terminal mixed result returns HTTP 200:

```json
{
  "batch_id": "819e3488-25be-4dc1-9b3f-d76e827e7a90",
  "mode": "check",
  "state": "partially_failed",
  "signature_version": "v1",
  "policy": {"kind": "text_only", "threshold": 0.75},
  "expires_at": "2026-11-04T00:00:00Z",
  "files": [
    {"file_id": "7825e3cb-b431-48cd-b1bd-4fd56e9b646e", "name": "report.pdf", "state": "completed", "document_id": null, "matches": []},
    {"file_id": "9a5c4145-dc8d-427b-95b9-67b9f7dd3769", "name": "empty.pdf", "state": "failed", "error": {"code": "empty_file", "message": "File is empty.", "retryable": false}}
  ]
}
```

A request exceeding the combined content limit returns HTTP 413, creates no batch or jobs, and schedules cleanup of staged objects:

```json
{"error":{"code":"total_too_large","message":"Combined file content exceeds 104857600 bytes.","retryable":false,"request_id":"request-reference"}}
```

Conditional polling of an unchanged unfinished batch:

```http
GET /v1/batches/819e3488-25be-4dc1-9b3f-d76e827e7a90 HTTP/1.1
If-None-Match: "batch-819e3488-25be-4dc1-9b3f-d76e827e7a90-r3"

HTTP/1.1 304 Not Modified
ETag: "batch-819e3488-25be-4dc1-9b3f-d76e827e7a90-r3"
Cache-Control: no-store
Retry-After: 2
```

The 304 response has no body. Terminal 200 and 304 responses omit `Retry-After`.

Expired results return HTTP 410 to their owner during the 24-hour tombstone retention, with `Cache-Control: no-store`:

```json
{"error":{"code":"batch_expired","message":"Batch results have expired.","retryable":false,"request_id":"request-reference"}}
```

After tombstone deletion, or for another client, the response is HTTP 404 with `batch_not_found`. Corpus documents survive batch result expiration; source upload objects follow the separate job cleanup policy.

## Matching policy

The approved [decisions](decisions.md) define candidate recall, short-document matching, and weighted scoring. Default policy is `{"kind":"text_only","threshold":0.75}`. Weighted policy is `{"kind":"weighted","threshold":0.75,"text_weight":0.8}` for an example configured weight; there is no implicit weighted default. The server configures the policy rather than accepting per-request overrides. Threshold and weight must be finite and in [0,1]. The batch snapshots all effective settings.

Candidate lookup combines the exact-content lookup and text LSH. Weighted mode additionally retrieves documents sharing metadata tokens when the metadata weight is positive. Queries stay within the client's corpus and signature version. Exact duplicate lookup avoids MinHash work for that pair, but does not skip lookup of other qualifying documents. Every retrieved pair is verified exactly under its versioned text policy before threshold filtering.

Text with fewer than configured k words matches only identical normalized text with text score 1; unequal short texts and short/long pairs have text score zero. Empty extraction fails. Metadata can still qualify a short document under weighted policy. Undefined metadata contributes zero to weighted scores and remains null in the response.

## Health

`GET /health/live` returns 200 while the HTTP runtime can respond. `GET /health/ready` returns 200 when PostgreSQL answers `SELECT 1`, or 503 with the standard retryable error envelope after a database error or two-second timeout. Both use `Cache-Control: no-store` and require no client identity. Neither waits for CPU workers or extraction. Readiness does not probe S3 or certify that migrations have run; deployment must complete migrations before routing traffic.

Polling streams matches in batches of at most 128 rows from a read-only database snapshot. A response holds a database connection until delivery ends or the client disconnects. Midstream database errors terminate delivery; clients should retry the GET. Polling does not invoke worker processing or alter stored results.
