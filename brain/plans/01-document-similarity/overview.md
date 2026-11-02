# Document similarity service

## Source

- [Original specification](../../../docs/original-specification.md), preserved verbatim before the planning request.
- Later deployment and storage clarifications are recorded in [decisions and evidence gates](../../../docs/decisions.md).

## Status and scope

- Phase 1 is complete and its API/contract decisions are approved. The application is currently a dependency-free Hello World binary.
- Phase 2 defines the extraction corpus; no service endpoints are implemented yet.
- Build lexical Jaccard similarity with Actix-web, PostgreSQL, library-backed durable jobs, and direct S3-compatible uploads.
- Exclude semantic embeddings, vector databases, frontend, authentication implementation, SSE, and webhooks.
- Target two CPU cores and 4 GiB RAM; two CPU workers maximum, with queue concurrency no greater than pool size.
- Use latest stable PostgreSQL verified at implementation time, with an explicit tested major version recorded then.
- Preserve 20 MiB/file and 100 MiB/request exact binary byte limits as approved.
- MinIO and Docker-hosted PostgreSQL are confirmed. Corpus size/growth, peak submissions, instance count, and representative real fixtures remain unknown.

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
- [ ] [[plans/01-document-similarity/phase-02-extraction-corpus]]
- [ ] [[plans/01-document-similarity/phase-03-extraction-spike]]
- [ ] [[plans/01-document-similarity/phase-04-schema]]
- [ ] [[plans/01-document-similarity/phase-05-normalization]]
- [ ] [[plans/01-document-similarity/phase-06-shingles]]
- [ ] [[plans/01-document-similarity/phase-07-signatures]]
- [ ] [[plans/01-document-similarity/phase-08-verification]]
- [ ] [[plans/01-document-similarity/phase-09-candidate-lookup]]
- [ ] [[plans/01-document-similarity/phase-10-queue-spike]]
- [ ] [[plans/01-document-similarity/phase-11-cpu-pool]]
- [ ] [[plans/01-document-similarity/phase-12-shared-storage]]
- [ ] [[plans/01-document-similarity/phase-13-extractors]]
- [ ] [[plans/01-document-similarity/phase-14-queue-adapter]]
- [ ] [[plans/01-document-similarity/phase-15-worker]]
- [ ] [[plans/01-document-similarity/phase-16-admission]]
- [ ] [[plans/01-document-similarity/phase-17-http-uploads]]
- [ ] [[plans/01-document-similarity/phase-18-polling]]
- [ ] [[plans/01-document-similarity/phase-19-retention-reindex]]
- [ ] [[plans/01-document-similarity/phase-20-operations]]
- [ ] [[plans/01-document-similarity/phase-21-acceptance]]
