# Decisions and evidence gates

## Phase 1 approval

Approved by the user on November 3, 2026: the [API contract](api-contract.md) and the recommendations below. The user confirmed continuation after clarification that these resolve the original plan's open decisions, including exact verification replacing estimator-based acceptance/rejection. The [original specification](original-specification.md) remains unchanged as the source record.

## Confirmed deployment constraints

- Two CPU cores, 4 GiB RAM; at most two CPU workers, with queue handler concurrency no greater than the CPU pool size.
- Direct MinIO S3-compatible storage; do not use the sibling upload service.
- PostgreSQL runs in Docker. Select, pin, and test the latest stable major version when database implementation begins.
- One through five files; default exact limits are 20,971,520 bytes/file and 104,857,600 content bytes/request.

## Approved contract decisions

| Topic | Approved decision | Consequence |
| --- | --- | --- |
| Retrieval | Keep 125-value signatures and 25x5 LSH; accept probabilistic recall | Return every qualifying retrieved candidate, but cannot promise every qualifying corpus document. |
| Verification | Exactly verify every retrieved candidate; use MinHash only to order work | Removes estimator threshold errors, at additional CPU cost. Deviates from the requested fast acceptance/rejection. |
| Band collisions | Keep indexed 32-bit band hashes and verify collisions | Band index mixing reduces systematic collisions; it cannot guarantee collision freedom. |
| Short documents | For fewer than configured k words, use normalized full-text equality only; retrieve through content hash | Identical short texts match with score 1; unequal short texts and short/long pairs have text similarity zero (metadata may still qualify them under weighted mode). Record this rule in the signature version. |
| Empty text | Fail with `no_extractable_text` | Empty extraction never becomes an exact duplicate. |
| Weighted policy | Keep default text-only; weighted mode uses the union of text LSH and all same-client, same-version documents with a nonempty metadata token intersection | Any positive metadata Jaccard has a shared token. This adds a metadata-token candidate index and may be expensive for common tokens. Text-only contribution still has probabilistic recall. |
| Weighted score | `text_weight * text_similarity + (1 - text_weight) * metadata_similarity`, with weight in [0,1]; undefined metadata contributes zero | Validate configuration before admission and snapshot the weight. Exactly verify both component scores for the candidate union. No text threshold gate in weighted mode. |
| Content identity | One canonical document per client and normalized text, with separate upload provenance | Repeated ingests reuse its id; canonical metadata comes from its first successful ingest. |
| Identity and visibility | Trusted proxy identity, client-scoped corpus and batches | Requires stripping/replacing the identity header and preventing direct API access. |
| Submission and polling | Use the linked API contract, including all-invalid 202, no request replay, per-file corpus visibility, and expiration | An ambiguous submission retry may create another batch. |
| Upload recovery | Stop storing an oversized part, continue bounded draining/counting; abort on whole-request limit | Preserves valid siblings while counting all received file content. |
| Body allowance | 1,048,576 bytes beyond content cap | Default raw cap is 105,906,176 bytes; proxy cap must exceed it. |
| Retention | Results expire 24 hours after terminal completion; owner-visible tombstones last another 24 hours | Corpus retention is independent. |

Weighted candidate retrieval must use canonical metadata tokens consistent with scoring. Zero-weight components need no candidate source. An empty/empty metadata score is null in responses and contributes zero in weighted scoring. This policy preserves weighted qualification without inventing a text gate; its index and query cost must be evaluated before enabling weighted mode.

## Mathematical limits

- LSH candidate probability is `1 - (1 - p^5)^25`: approximately 0.99885 at Jaccard 0.75 and 0.059 at 0.3. Probabilistic retrieval can miss qualifying corpus documents; exact verification prevents threshold false positives among retrieved candidates.
- Mixing band position into a 32-bit hash cannot guarantee collision freedom. Hash collisions add candidates that exact verification filters.
- MinHash standard error at 0.75 with 125 values is about 0.0387. Estimates may order work but must never accept or discard candidates at the threshold. The estimator margin is no longer a classification policy.

## Later implementation and deployment gates

These unknowns are recorded rather than invented. They do not prevent contract approval.

- Corpus count, typical extracted text size, growth, peak batch submissions, and instance count: establish the measured workload in phase 21 before making capacity claims; calibrate admission quotas in phase 16.
- MinIO endpoint, bucket ownership, and secret delivery method: resolve in phase 12 before storage deployment. Never put credentials in this repository or queue payloads.
- Docker PostgreSQL persistent storage, connection/TLS requirements, and migration privileges: resolve in phase 4 before database deployment.
- Real extraction fixtures, extraction quality/speed/RSS, native versus external tool choices, OCR shingle policy, expansion limits, and timeouts: measure in phases 2 and 3 before adapters are built.
- A running Rust thread cannot be forcibly terminated safely. Hard extraction timeouts require cooperative cancellation or subprocess isolation with process/resource limits.
- S3 objects and PostgreSQL commits are not atomic. Admission leases and orphan cleanup must cover disconnects/crashes while retaining stored-byte accounting (phases 12 and 16).
- Queue retries, live-job reclaim protection, migration ownership, shutdown, and retention: establish in the phase 10 graphile_worker spike. Evaluate apalis-postgres only after reporting a failed requirement and obtaining approval to switch. Do not claim exactly-once execution from job keys.
- Worker waiting slots, queue depth/byte quotas, per-client quotas, deadlines, and retry/backoff values require measured defaults before production use.
- Audit runtime dependencies at their implementation phase for releases, activity, downloads, issue responsiveness, license, MSRV, and deliberate version pinning.
