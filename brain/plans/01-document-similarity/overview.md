# Document similarity service

## Source

- [Original specification](../../../docs/original-specification.md), preserved verbatim before the planning request.
- Later deployment and storage clarifications are recorded in [decisions and evidence gates](../../../docs/decisions.md).

## Status and scope

- Phase 1 is complete and its API/contract decisions are approved. Phase 4 completed the application schema and PostgreSQL integration tests; phase 5 completed the versioned CPU-only text normalizer. Phase 6 completed word shingles, short-text equality, and versioned native/verbatim-OCR comparison policies. Phase 7 completed deterministic versioned MinHash signatures, 25×5 banding, golden vectors, synthetic recall checks, and local signing benchmarks. Phase 8 completed exact candidate verification, validated policy snapshots, deterministic metadata tokens, and weighted scoring. Phase 9 completed client/version-scoped paged candidate retrieval, dedicated CPU verification, and race-safe canonical ingests with preserved upload provenance. Phase 10 completed the graphile_worker lifecycle/dependency spike and retained it under an explicit at-least-once contract; phase 11 completed the shared bounded CPU executor, deadline/cancellation controls and shutdown tests; phase 12 completed shared S3 uploads, durable quota/journal accounting and orphan reconciliation; phase 13 completed the approved extraction adapters, Linux subprocess supervision and bounded corpus verification; phase 14 completed transactional library enqueue, stored-file payloads, pending-key deduplication and explicit bounded retry policy; phase 15 completed independent durable workers, attempt-fenced processing, transactional results/ingests, recovery/shutdown verification and source retention cleanup; phase 16 completed durable cross-instance admission leases, upload budget transfer and quota/expiry verification; phase 17 completed bounded multipart checks/ingests, durable transactional admission, request/file byte limits, staged-object cleanup and PostgreSQL/MinIO integration verification; phase 18 completed snapshot-consistent streamed polling, revision ETags, expiration/ownership checks and independent health endpoints; phase 19 completed bounded retention/deadline cleanup, deployment migrations and restartable version-isolated reindexing; phase 20 (configuration/observability) is next.
- Phase 2 is complete. Phase 3 benchmark/reporting work is complete: 414 trials and per-format recommendations are available in `spikes/extraction/` and `docs/extraction-spike.md`. Extraction tools, model terms, subprocess limits and retention of single-character OCR lines are approved; nom-exif replaces the stale EXIF candidate. Submission, polling and health endpoints are implemented.
- Build lexical Jaccard similarity with Actix-web, PostgreSQL, library-backed durable jobs, and direct S3-compatible uploads.
- Exclude semantic embeddings, vector databases, frontend, authentication implementation, SSE, and webhooks.
- Target two CPU cores and 4 GiB RAM; two CPU workers maximum, with queue concurrency no greater than pool size.
- Use latest stable PostgreSQL verified at implementation time, with an explicit tested major version recorded then.
- Preserve 20 MiB/file and 100 MiB/request exact binary byte limits as approved.
- MinIO and Docker-hosted PostgreSQL are confirmed. Corpus size/growth, peak submissions, and instance count remain unknown; the public corpus does not establish a representative customer workload.

## Approval gates

- [API contract](../../../docs/api-contract.md) approved November 3, 2026.
- Resolve [decisions and evidence gates](../../../docs/decisions.md) before the affected core implementation.
- Approve extraction recommendations after measured corpus trials; no quality/speed claims yet.
- Complete graphile_worker spike before worker implementation; report before switching libraries.
- Preserve the requested order: extraction spike, schema/core, queue spike/worker, HTTP, acceptance.
- Queue execution depends on the bounded CPU pool and durable storage steps before HTTP.

## Design choices and principles

- [[principles/small-reviewable-changes]] requires focused changes; each phase has checkbox-sized review steps.
- [[principles/bounded-resources]] governs admission, decoding, deadlines, and cluster-wide reservations.
- Separate check/ingest URLs share one pipeline; this makes writes explicit without duplicating processing.
- Prefer S3 to a shared volume for cross-instance access, per user clarification; do not reuse sibling storage.
- Use graphile_worker first; apalis-postgres is an evaluated fallback requiring approval, never an automatic switch.
- Exactly verify all retrieved candidates; MinHash estimates only order work. This deviation from estimator acceptance is approved.
- LSH is probabilistic: the 25x5 layout has about 99.885% retrieval at Jaccard 0.75, not exhaustive recall.
- Audit each dependency at implementation time using crates.io and repository evidence; add no runtime crates in this planning change.
- Applicable skills: brain, todo, plan, unslop; testing/debugging/quality during implementation, k6 for load scripts.

## Phases

- [x] [[plans/01-document-similarity/phase-01-contract]]
- [x] [[plans/01-document-similarity/phase-02-extraction-corpus]]
- [x] [[plans/01-document-similarity/phase-03-extraction-spike]]
- [x] [[plans/01-document-similarity/phase-04-schema]]
- [x] [[plans/01-document-similarity/phase-05-normalization]]
- [x] [[plans/01-document-similarity/phase-06-shingles]]
- [x] [[plans/01-document-similarity/phase-07-signatures]]
- [x] [[plans/01-document-similarity/phase-08-verification]]
- [x] [[plans/01-document-similarity/phase-09-candidate-lookup]]
- [x] [[plans/01-document-similarity/phase-10-queue-spike]]
- [x] [[plans/01-document-similarity/phase-11-cpu-pool]]
- [x] [[plans/01-document-similarity/phase-12-shared-storage]]
- [x] [[plans/01-document-similarity/phase-13-extractors]]
- [x] [[plans/01-document-similarity/phase-14-queue-adapter]]
- [x] [[plans/01-document-similarity/phase-15-worker]]
- [x] [[plans/01-document-similarity/phase-16-admission]]
- [x] [[plans/01-document-similarity/phase-17-http-uploads]]
- [x] [[plans/01-document-similarity/phase-18-polling]]
- [x] [[plans/01-document-similarity/phase-19-retention-reindex]]
- [ ] [[plans/01-document-similarity/phase-20-operations]]
- [ ] [[plans/01-document-similarity/phase-21-acceptance]]
