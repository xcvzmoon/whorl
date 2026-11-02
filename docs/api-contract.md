# Proposed API contract

Status: awaiting user approval. Endpoints below are not implemented.

## Submission

`POST /v1/checks` compares uploads without adding corpus documents.
`POST /v1/ingests` compares uploads and persists each successful document.
Both accept multipart/form-data with repeated `files` parts (one to five).
Separate URLs make corpus writes explicit; both use one processing pipeline with a stored mode.
The configured policy/version is snapshotted on each batch, so retries use the same rules.
An optional `Idempotency-Key` is scoped to the trusted upstream client and mode; replay and payload-conflict semantics must be approved before implementation.

Successful durable admission returns `202 Accepted`, `Location: /v1/batches/{id}`, and `Retry-After: 2` (proposed configurable seconds):

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

Every part gets a stable file id, including rejected parts. A nonempty request with all files individually rejected still creates a terminal failed batch and returns 202, with no jobs; this is a proposal requiring approval. Mixed valid/invalid files retain all individual statuses. Type validation can fail during streaming or extraction. Client filenames are display data, never storage paths.

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

Scores use 0..1, not percentages. Include all qualifying ids sorted by policy score descending, then document id; no silent top-N truncation. Metadata score is null when disabled. Ingest returns its persisted document_id; check returns null. The response must distinguish estimated from exact text scores if estimator acceptance is approved. Large result sets may require approved pagination; corpus sizing is still unknown.

Batch states: queued (no work started), running (some work started and unfinished), completed (all succeeded), partially_failed (terminal with successes and failures), failed (terminal with no successes). File states: queued, running, completed, failed. Running progress stages: extracting, signing, candidate_lookup, verifying, persisting. Retrying files return to queued with attempt and next_retry_at; status revisions preserve visibility. No percentage is reported without measurable units.

Unknown/inaccessible batches return 404. Proposed expired batches return 410 while a bounded tombstone exists, then 404. Default result retention is 24 hours after terminal completion. No long-polling, SSE, or webhooks in version one. Jobs write results independently of result delivery.

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
| 409 | idempotency_conflict, if request replay support is approved |

Per-file failures use the same code/message/retryable fields within the file result, including file_too_large, unsupported_type, empty_file, malformed_file, extraction_limit, extraction_timeout, and no_extractable_text. They do not discard valid sibling files. More than five file parts, combined content over 104,857,600 bytes, and zero files reject the entire request. All received file bytes count toward the total, including rejected files. On an oversized individual file, storage stops at the crossing chunk; bounded draining/counting allows subsequent parts to survive without storing the oversized content.

Per-client admission requires a trustworthy identity supplied by upstream authentication. Define and approve that identity/forwarding boundary; arbitrary public headers must not bypass quotas or expose other clients' batches. Rate limits and retry intervals are configuration values to calibrate during the load test.
