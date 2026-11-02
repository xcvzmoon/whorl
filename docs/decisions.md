# Decisions requiring approval or evidence

## Confirmed deployment constraints

- Two CPU cores, 4 GiB RAM; latest stable PostgreSQL selected and pinned when implementation begins.
- Direct S3-compatible storage; do not use the sibling upload service.
- One through five files; preserve specified exact limits of 20,971,520 bytes/file and 104,857,600 bytes/request. The later use of "mb" is treated as shorthand; decimal limits require explicit confirmation.
- No real extraction corpus supplied. Corpus scale/growth and peak concurrency remain open.

## Mathematical constraints

- At true Jaccard 0.75, 25x5 LSH candidate probability is 1-(1-0.75^5)^25, approximately 0.99885. At 0.3 it is approximately 0.059. LSH cannot guarantee returning every match. Approve probabilistic recall or revise the exhaustive-result requirement.
- Mixing band index into a 32-bit value prevents identical bands at different positions from systematically producing identical hashes. It cannot guarantee cross-band collision freedom in a finite hash space. Preserve the required representation, treat collisions as candidates, and verify them, subject to approval of this interpretation.
- MinHash agreement has standard error sqrt(p*(1-p)/125), about 0.0387 at p=0.75. A three-SE boundary window is about 0.116. Accepting outside that window still has nonzero threshold error. Recommendation: use estimates to order candidates and verify all returned matches exactly; this deviates from the requested fast accept/reject policy and needs approval. Alternatively retain that policy and explicitly accept residual classification error.
- Short-document fallback changes the shingle space. Proposed one/two-word exact matching and effective k=min(3, words) needs a defined cross-length comparison policy, versioning, and candidate lookup for short inputs. Empty extraction should be a per-file failure, not an empty-set duplicate.
- Weighted metadata can qualify text-dissimilar documents that text LSH will never retrieve. Approve text-gated weighting/tie-breaking or design an additional metadata candidate strategy before enabling weights. Default remains text-only threshold.
- Content-hash identity deduplicates normalized text across source types/metadata; per-upload metadata and provenance must survive separately. Define whether multiple external document ids for identical content are required.

## Resource and operational choices

- Proposed HTTP overhead allowance: 1 MiB; raw body cap 105,906,176 bytes. Reverse proxy limit must exceed it. Per-file/total streaming counters remain authoritative.
- A running Rust thread cannot be forcibly terminated safely. Hard extraction timeouts need cooperative cancellation or subprocess isolation with process/resource limits; a timed-out future alone is insufficient.
- Per-file oversize recovery needs draining the rejected part, while whole-request overflow aborts immediately. Confirm this necessary reading of the two upload requirements.
- S3 objects and PostgreSQL commits are not atomic. Admission leases and orphan cleanup must cover disconnects/crashes without exceeding stored-byte caps.
- Queue execution must be idempotent; do not claim exactly-once execution from job keys. The queue spike must establish protection against concurrent reclaim of live long jobs.
- Worker pool waiting slots, queue depth/bytes quotas, per-client quotas, deadlines, retry/backoff, image/archive/XML/page/sheet limits, and extraction timeouts need measured defaults before production implementation.
- API proposal needs approval, including all-invalid 202 batches, cross-client access, check/ingest visibility, expiration, metadata defaults, and request replay semantics.

## Evidence still required

- Extraction quality/speed/RSS across real files, native versus external tool choices, OCR shingle choice.
- graphile_worker queue guarantees and maintenance health; fallback requires a separate report before switching.
- Each runtime dependency's current release, repository activity, downloads, issue responsiveness, license, MSRV, and deliberate version pinning.
