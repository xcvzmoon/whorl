# Architecture review resolutions

The November 4, 2026 review identified signature rollout defects and two places where callers had to coordinate durable rules across modules. The implementation now addresses all three findings. The original matching guarantees remain: native/OCR comparisons stay separate, retrieval remains probabilistic, and every retrieved candidate is verified exactly.

## Signature preparation and registration

Previously, worker preparation always used k=3 even when the batch selected another shingle length. Native and OCR registrations created by maintenance also differed in their source field, while the worker required their complete configuration JSON to be equal.

[SignatureVersion](../src/similarity/signature.rs) now constructs and parses the supported canonical recipe, exposes its shingle length, and produces its registration configuration. Unsupported algorithms, sources, zero lengths, and noncanonical spellings fail validation. The worker reads k from the snapshot and validates each registered configuration against its own version. Native and OCR may share algorithm settings without sharing a comparison namespace.

`SignatureIndex` computes the persisted signed values and bands for both live preparation and reindexing, including null arrays for short text. This gives the signature module more depth: callers use one interface for recipe identity and storage representation, while version knowledge has locality. There is no new adapter abstraction at this in-process seam.

Regression coverage includes the two previously failing preparation/registration cases, invalid versions/configuration, live-versus-reindexed array equivalence, and Linux worker processing with actual maintenance-created native/OCR k=4 registrations. Existing deterministic vectors remain unchanged.

## Durable lifecycle ownership

The production worker already committed matches, canonical ingest, file completion, and batch aggregation atomically. A separate `PreparedDocument::ingest` method also completed files, without finishing their batches or applying the worker's processing fences.

Canonical ingestion now only writes corpus content and version membership inside the caller's transaction. It does not own file or batch state. The worker uses this same method in its final fenced transaction. The obsolete file-completion behavior and its `invalid_upload` error path are removed.

The [lifecycle module](../src/service/lifecycle.rs) owns validated retention, terminal aggregation, and bounded deadline recovery. Admission, processing, and maintenance share its interface. It locks a batch before evaluating its terminal outcome, uses one timestamp for completion and expiry, and preserves existing completion dates on repeated calls. Recovery respects live worker locks and increments abandoned attempt generations.

This seam concentrates rules rather than adding a pass-through SQL helper. Deleting the module would distribute aggregation, retention, and recovery knowledge across callers again. The concrete PostgreSQL adapter stays in place. Tests exercise the same interface callers use, improving locality and leverage across all three paths.

[Lifecycle tests](../tests/lifecycle.rs) cover all-invalid, all-success, mixed, unfinished, concurrent final siblings, immutable timestamps, deadline recovery, and live-lock exclusion. Candidate tests exercise canonical races without pretending to complete a durable file; worker tests retain full pipeline/recovery coverage.

## Staged-upload attachment

Previously, HTTP had to know the object-key advisory-lock recipe before attaching sources, while the queue adapter separately checked upload eligibility.

`AdmissionLease::admit` in the [admission module](../src/service/admission.rs) now owns the durable handoff. It validates the request lease and submitted outcomes, orders object locks, verifies journal identity/owner/bytes/completion/expiry and exclusive source ownership, inserts batch/file records, enqueues eligible files, and aggregates all-invalid completion in one transaction. HTTP retains multipart parsing, response construction, and request cleanup; it contains no attachment SQL or lock protocol.

The interface gives callers durable admission without requiring storage coordination knowledge. The concrete storage and queue adapters retain their recovery responsibilities. Transaction rollback, ambiguous commit protection, and charged-object cleanup remain part of the implementation. No interchangeable adapter was added merely for testing.

[Attachment tests](../tests/attachment.rs) exercise rejected references and admission racing actual S3 reconciliation, in addition to the existing upload, reservation, enqueue, and storage suites. Tests cross the admission interface rather than manually duplicating its attachment protocol. A further regression proved that cloned source references could otherwise create several jobs for one upload charge; admission now rejects duplicate references and prior claims under the object lock.

## Remaining performance limits

This review does not justify a performance rewrite. Candidate text is paged, but every qualifying score remains in memory until persistence. Common metadata tokens can broaden weighted retrieval, and telemetry snapshots scan retained state. These require representative corpus and arrival-rate measurements before changing indexes, concurrency, caching, or query structure. Fixture acceptance remains the scope of existing capacity evidence.

See [development and testing](development.md) for verification commands and [history](history.md) for the chronological implementation account.

## Verification

Completed November 4, 2026:

- The two original signature regressions failed before their fixes; preparation and source-specific registration checks now pass.
- A duplicate-source admission regression failed before exclusive-claim validation; repeated and previously claimed sources are now rejected without additional jobs.
- macOS default tests and the complete supported PostgreSQL/MinIO integration runner passed. The runner explicitly skips successful extraction acceptance on macOS.
- Linux release all-target/all-feature tests and all 14 external integration checks passed under a two-CPU, 4 GiB container limit. The worker check includes real extraction, cancellation/restart, shutdown, killed-worker recovery, concurrency, and maintenance-created native/OCR k=4 registrations. The end-to-end check covers HTTP admission through extraction and durable results to polling.
- Formatting, strict all-target/all-feature Clippy, 56 doctests, warning-free rustdoc, local documentation links, and diff whitespace checks passed.

Reproduce using the commands in [development and testing](development.md). The integration runner creates fresh disposable databases and now includes attachment and lifecycle checks explicitly. No production cleanup, deployment, or version cutover was performed.
