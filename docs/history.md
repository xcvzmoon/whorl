# History

I built Whorl on November 3 and 4, 2026, starting with the document contract and extraction evidence, then adding persistence, similarity, durable execution, HTTP delivery, and operational verification. This account follows that order. The linked measurements describe the workloads I actually ran; they do not establish capacity for an unspecified customer corpus or arrival rate.

## November 3, 2026: establishing the contract and repository checks

I preserved the [original specification](original-specification.md) before changing its interpretation. I then wrote the [API contract](api-contract.md) for separate check and ingest submissions, durable batch status, per-file outcomes, conditional polling, expiration, and trusted upstream identity. I kept corpus writes explicit through separate URLs while sharing the processing pipeline.

I resolved several mathematical and behavioral conflicts before implementing the matcher. I retained 125 MinHash values and 25 bands of five, accepted probabilistic retrieval, and required exact verification of every retrieved candidate. Estimates could never accept or discard a candidate at the qualification threshold. For fewer than three words, I chose normalized full-text equality instead of inventing shingles. Empty extraction became an explicit failure. I also defined weighted metadata qualification without a separate text gate, preserved first-ingest canonical metadata, and recorded the limitations of 32-bit band hashes. I recorded the approvals and their consequences in [the decision record](decisions.md).

I confirmed a two-core, 4 GiB deployment target, direct MinIO storage, Docker-hosted PostgreSQL, and inclusive limits of 20 MiB per file and 100 MiB of combined file content. I left customer corpus size, growth, arrival rate, and replica count unspecified rather than deriving them from a public fixture collection.

I added formatting, Clippy, tests, warning-free rustdoc, release builds, dependency auditing, issue forms, and pull-request context checks. I corrected the license audit so private application packages did not require an invented application license while dependencies still received scrutiny. I replaced source compilation of the dependency auditor with a pinned, checksum-verified prebuilt release and recorded the measured setup costs in the [Actions audit](github-actions-audit.md). Tagged release checks verify that the tag agrees with Cargo's package version. I later added an explicit doctest command when Whorl gained a library, because all-target testing alone did not run those examples.

## Building a reproducible extraction corpus

I assembled 34 cases with licensed upstream documents and generated adversarial inputs. The collection included digital and scanned PDFs, Office documents, spreadsheets, photographed text, exact 20 MiB examples, malformed data, archive expansion probes, and an oversized image. I retained provenance, license notices, hashes, expected text, expected metadata, and regeneration instructions. The large spreadsheet contained 100,000 rows.

I separated quality evidence from limit testing. Real documents provided extraction and OCR comparisons; generated inputs established byte limits, parser behavior, and resource ceilings. Validator checks caught damaged inputs, damaged references, missing coverage, and missing licenses. I kept the [corpus protocol](../spikes/extraction/README.md), [manifest](../spikes/extraction/manifest.json), and [measurement records](../spikes/extraction/measurements.json) with the experiment so another run could reproduce its inputs.

I then ran 414 extraction trials across 69 candidate/configuration combinations on Linux ARM64 under a two-CPU quota and 4 GiB memory cap. Each combination had a warmup and five measured repetitions. I compared PDF text/rendering options, Office ZIP/XML readers, spreadsheet readers, image OCR, Tesseract, and alternative OCR shingling choices. I measured wall time, process memory, text coverage, and OCR word/character error rather than choosing tools from their advertised capabilities.

The evidence led me to PDFium for PDF text/rendering, Tesseract for scanned PDF pages, zip/quick-xml for DOCX and PPTX, calamine for XLSX, and image/Ocrs for PNG and JPEG. I retained single-character OCR lines and the word-shingle policy. I recorded model distribution terms and replaced the stale EXIF candidate with nom-exif after evaluating its maintenance and license. The [extraction report](extraction-spike.md), [summaries](../spikes/extraction/results/summary.md), and [raw artifacts](../spikes/extraction/results/raw-results.tar.gz) preserve the comparison.

## Creating durable application records

I implemented canonical documents, versioned signatures, batches, ordered file records, match results, and capacity reservations in PostgreSQL. I made normalized-text content hashes unique within a client and kept upload provenance separate from the canonical corpus. Composite foreign keys prevented cross-client document/result references. Status, deadline, array-shape, and retention constraints rejected invalid durable states.

I stored 125 signature values and 25 band hashes as signed integer arrays with reversible unsigned-bit mapping. I added GIN indexes for band overlap and metadata retrieval, plus a result index supporting score-descending and document-ID ordering. I kept signatures separate from documents so different algorithm versions could coexist. Revision triggers made batch changes visible to conditional polling.

I verified the schema on PostgreSQL 18.6, including rollback, canonical uniqueness, client isolation, version coexistence, revision changes, and reservation races. Twelve competing reservations admitted exactly three at the configured client limit. I treated an expired lease as evidence for cleanup, not permission to forget bytes that might still occupy storage. The [persistence guide](persistence.md) records the schema and its tests.

## Implementing canonical text, shingles, and signatures

I implemented versioned normalization with NFKC, scalar lowercase, Unicode letters/numbers/marks, punctuation deletion, and whitespace folding. I preserved accents and repeated lines. I deliberately did not add page-header removal, OCR correction, or line-break dehyphenation. Golden examples checked idempotence and fixed the Unicode-table versions so a toolchain or dependency update could not silently change persisted identity.

I added word shingles with set semantics and a default length of three. Short text used symmetric normalized equality; incompatible normalization, source, or shingle policies were rejected before comparison. Native and verbatim-OCR text had separate namespaces even though both used the same normalization and word rules. Shingles borrowed canonical text slices instead of copying every overlapping sequence.

I made signatures deterministic with fixed XXH3 seeds and little-endian encoding. Each signature had 125 values, and each of its 25 band hashes included the band's position. I preserved full [golden vectors](../tests/fixtures/vectors/) for Unicode, different word lengths, and a single shingle. Synthetic recall checks used 2,000 disjoint namespaces at each tested similarity: 1,997 pairs were retrieved at Jaccard 0.75 and 127 at 0.3, consistent with the stated statistical bounds. I also recorded signing benchmarks by document size. Those were algorithm checks and a local CPU baseline, not a production capacity measurement.

## Adding exact scoring and corpus retrieval

I added an exact similarity engine over compatible shingle sets, with strict unrounded qualification above the threshold. I validated finite thresholds and optional text weights, and copied immutable policy settings into the engine. Weighted scoring combined exact text and metadata scores; undefined metadata contributed zero.

I made metadata tokenization deterministic and case-sensitive. Escaped paths and JSON scalar representations preserved type distinctions, arrays ignored position and duplicate leaves, and nulls or empty containers contributed no tokens. Creation/modification dates and size fields excluded their entire subtree. I used the same token builder for indexing and scoring so candidate retrieval could not drift from qualification.

I then connected this work to PostgreSQL using SQLx. Exact-content lookup came first without suppressing other near matches. A read-only repeatable-read transaction held the corpus snapshot while UUID-keyset pages retrieved at most 32 candidates. Weighted retrieval unioned band overlap with metadata-token overlap and removed duplicates before verification. I sorted qualifying results by score descending and document ID ascending.

I tested tenant/version isolation, first-ingest metadata preservation, unchanged upload provenance, eight concurrent duplicate ingests, short-text membership, false-positive rejection, deterministic ordering, and 70 metadata-only matches across three pages. With 10,000 synthetic background documents and refreshed statistics, query plans used both retrieval GIN indexes without planner overrides. I retained all qualifying scores, so paging candidate text did not imply a constant-memory result set.

## Proving durable queue behavior and sharing CPU capacity

Before adopting graphile_worker, I built an isolated PostgreSQL probe for notifications, fallback polling, retry limits, retained failures, pending/running job keys, transactional enqueue, migration ownership, and restricted-role access. I checked 185-second live-job protection, recovery after SIGKILL, and cancellation/release after SIGTERM. The results supported retaining graphile_worker 0.14.1 under an explicit at-least-once delivery contract; I had no failed requirement that justified switching libraries. The [queue research](queue-spike.md) links the executable evidence and privilege checks.

I replaced the matcher's private executor with a shared bounded Rayon pool for the whole worker pipeline. I added immediate admission, measured waiting-capacity calculation, absolute deadlines, queued cancellation, running cancellation signals, panic containment, and drainable shutdown. Cancellation retained the occupied slot and its inputs until the running computation exited. I kept the distinction between cooperative Rust cancellation and hard native-process termination explicit in the [CPU executor guide](cpu-pool.md).

## Adding journaled S3 uploads and isolated extraction

I implemented bounded multipart uploads to random S3 keys with credential-free object references, durable journals, and reservation accounting. Uploads reserved capacity before reading bytes. Failed or interrupted writes retained enough state for abort/deletion and retry; failed cleanup retained its charge. Shared object-key advisory locks protected active writes and later attachment against reconciliation. I required a bucket lifecycle rule for a crash before the multipart ID reached the journal.

I verified real MinIO behavior for exact limits, empty/oversized/error streams, quota rejection, interruption, active-writer protection, and retained charges during storage outages. The [storage guide](storage.md) explains these ownership rules.

I implemented format detection from content and Office package relationships rather than trusting extensions. Office readers enforced archive/XML bounds; PDF and image readers applied page, pixel, output, and allocation limits. The Linux supervisor ran `whorl-extract` in a process group with a wall deadline and CPU/address-space/output limits, killed descendants on cancellation or timeout, reaped the child, and removed temporary files. I packaged trusted PDFium, OCR models, and Tesseract assets with their notices.

The Linux ARM64 extraction image passed all 34 corpus cases plus a mixed native/scanned PDF under the stated CPU, memory, and temporary-storage limits. I retained the [recorded extraction output](extraction-results.json) and [runtime reproduction instructions](extraction.md). I corrected CI and release checks to generate fixtures before tests, added Rust dependency caching, and required passwords for the disposable PostgreSQL service. I also corrected a descendant-cleanup assertion to wait for termination rather than assume it was instantaneous.

## Connecting uploads to durable processing

I added transactional enqueue of identifier-only file jobs. Batch/file writes and queue insertion shared one transaction, so enqueue failures rolled everything back and no separate outbox was needed. Pending keys deduplicated repeated enqueue while preserving the original schedule. I kept execution fencing separate from key deduplication and bounded delivery attempts using the queue library's retry policy.

I fixed completed uploads to clear their multipart journal marker before queue eligibility checks. I then added the independent worker pipeline with per-file advisory ownership, persisted attempt generations, named stages, and shared CPU execution. A retry restarted extraction; it did not reuse partial computation. The final transaction wrote comparison results, resolved canonical ingest, completed the file, and aggregated its batch before acknowledgment.

I tested real DOCX ingest/check/duplicate behavior, tenant isolation, immutable repeated delivery, storage retries/exhaustion, deadlines, permanent rejection, cancellation/restart, queue shutdown, long-running ownership, separate-process SIGKILL recovery, and handler concurrency. Three queued files reached exactly two concurrent handlers. I fixed the worker to release its attempt lock before returning retry errors, preventing retry handling from retaining ownership longer than intended.

## Reserving request budgets and admitting multipart batches

I added durable request leases under the shared PostgreSQL admission lock. The API reserved attempts and content bytes before reading a large body, distinguished global and client exhaustion, and returned the corresponding retry hint. Each upload transferred part of that request allowance into its own journaled charge, then returned unused bytes. I corrected the schema to allow a fully consumed lease to hold zero jobs and bytes until release or expiry.

I kept authentication upstream. Whorl required confirmation that a trusted proxy replaced client identity and that network policy blocked direct public access. The [admission guide](admission.md) records the checks, ownership transfer, expiry, and cleanup behavior.

I added separate multipart check/ingest routes with a shared receiver. I enforced inclusive per-file and combined limits while streaming, counted drained rejected content, bounded multipart structure and raw bytes, and preserved valid siblings after individual file failures. I returned 202 only after durable batch/file/job commit, including a failed batch when every file was individually invalid. I added explicit staged-object discard and cancellation cleanup without treating an ambiguous database commit as proof that an object was unclaimed.

I checked exact 20 MiB and one-byte-over boundaries, five exact files, combined excess, empty/mixed input, zero/six files, malformed structure, unknown parts, raw-body excess, identity rejection, and injected enqueue failure. PostgreSQL/MinIO assertions verified rollback and cleanup together.

## November 4, 2026: polling, maintenance, and operating visibility

I added independent liveness and PostgreSQL readiness routes, then implemented persisted batch polling. A read-only repeatable-read snapshot covered the batch, files, and matches. A server cursor fetched at most 128 results at a time. ETags used the persisted revision and supported weak tags, lists, and `*`; ownership and expiration were checked before conditional responses. Terminal polling did not extend retention.

I tested all aggregate states, progress/retries, concurrent revisions, 300 streamed matches, empty results, conditional responses, client isolation, expiration, and tombstones. Health checks stayed independent of extraction and CPU admission.

I added an operator-run migration command that serialized application upgrades, recorded applied versions, and then ran the pinned queue migrations. I separated elevated deployment ownership from runtime access. I added bounded cleanup for deadline-expired work, terminal results, tombstones, old queue jobs, abandoned request reservations, and journaled sources. Live worker locks protected active attempts; unsuccessful S3 deletion retained accounting.

I implemented restartable reindexing over stored canonical text. Missing signatures formed the durable cursor, each invocation handled a bounded page, and existing signatures supplied source membership. Tests covered restart safety, namespace isolation, short-text null arrays, and 32-plus-one page boundaries. I documented paused-ingest coverage checks and version cutover without claiming a production cutover had been performed.

I then added shared typed startup settings, validated database/listen/worker inputs, and loaded an optional local `.env` before starting threads. Existing environment values won; malformed local configuration failed startup. I added fixed JSON telemetry for stage duration, CPU admission, pending age, retained failures/retries, reserved bytes, and expired journals without exposing document data or tenant identifiers. Integration checks exercised actual upload cleanup and showed retained bytes/journals falling to zero. The [configuration guide](configuration.md) records recovery, rollout, alerts, and fixed settings.

## Establishing complete acceptance and correcting CI limits

I exercised admission through HTTP, durable queue delivery, Linux extraction, canonical ingestion, and polling with real PostgreSQL and MinIO. I ran independent API and worker smoke measurements for 20 one/five-file batches under two CPUs and 4 GiB, recorded aggregate memory and health/status latency, and published CPU signing/batch benchmarks alongside the six exact-size extraction probes.

I added the aggregate integration runner and wired every external suite into CI instead of relying on ignored local checks. I corrected the Linux test image to include migrations, then moved the MinIO build into a separate CI job so Rust compilation did not compete with it for runner disk space. I retained [acceptance measurements and reproduction](benchmarks.md) and the [raw acceptance evidence](acceptance/) with explicit workload limits. I did not infer customer-scale throughput from those results or claim hosted CI had been run locally.

## Replacing internal notes with repository documentation

I consolidated the original implementation records and engineering constraints into repository documentation, rewrote the README as an introduction and setup guide, and added the architecture/code map, similarity explanation, development guide, and documentation index. I removed the internal notes directory after checking coverage and links. I then replaced the copied delivery-record directory with this chronological account so the history describes decisions, implementation, and corrections in ordinary prose.

During the architecture review, I found that the deployment command and worker disagreed about signature configuration. Worker preparation always selected the default word length, and maintenance-generated native/OCR configurations did not satisfy the worker's equality check. Existing mixed-source tests used synthetic equal configurations and therefore missed that mismatch. I also identified competing file-completion ownership and upload attachment rules spread across HTTP, storage, and queue code. I recorded the evidence in the [architecture review](architecture-review.md); that document tracks the corrections and verification against the current implementation.

## Correcting signature rollout and durable ownership

I first reproduced both signature failures with focused regression tests. A four-word snapshot still produced a three-word signature, and valid native/OCR registrations failed compatibility because their source fields differed. I moved canonical version parsing, registration configuration, and persisted hash encoding into the signature module. The worker now reads the word length from its snapshot and validates each detected source against its own registration. I kept deterministic hash vectors and source-separated matching unchanged.

I changed canonical ingestion to write only corpus content and signature membership inside the caller's transaction. I removed its competing file-completion behavior and kept the worker responsible for attempt-fenced result, ingest, and file writes. I introduced shared terminal aggregation and deadline recovery so admission, processing, and maintenance apply one retention policy. I used one completion timestamp for expiry dates and verified that repeated completion leaves those dates unchanged.

I moved staged-source locking, validation, attachment, and queue insertion into request admission. HTTP now handles multipart reception, response construction, and cleanup through that interface. I tested actual admission while it held its source lock and confirmed that S3 reconciliation skipped the protected object. When the source expired before validation, admission rolled back and later reconciliation removed both the object and its charge.

The final code review exposed another accounting problem: cloned references could schedule several files against one upload charge. I reproduced it, then rejected repeated source references and previously claimed sources while holding the source lock. I retained tests for forged identity/ownership/bytes, incomplete uploads, expired leases, queue rollback, all-invalid batches, and committed-source protection. The [resolution record](architecture-review.md) explains the resulting module ownership and regression coverage.

I verified the corrections with the full macOS test suite, PostgreSQL/MinIO integrations, strict Clippy, formatting, warning-free rustdoc, and 56 compiling documentation examples. I then ran the Linux release suite and all 14 external integration checks under a two-CPU, 4 GiB container limit. Those checks included real extraction, worker cancellation/restart and recovery, maintenance-created native/OCR four-word registrations, and HTTP admission through durable processing to polling. I also checked every local documentation link and verified the wording of this account.

## Organizing runtime code, database files, and packaging

I moved the production extraction Dockerfile into `docker/`, grouped application migrations and queries under `database/`, and gathered SQL assertions and signature vectors in `tests/fixtures/`. I updated compile-time includes, image build inputs, and documented commands so the same source and evidence remained usable from their new locations. I kept the self-contained extraction experiment's Dockerfile beside its research code.

## Keeping test execution local

I removed test and documentation-example execution from CI and tagged releases, along with corpus preparation and the PostgreSQL/MinIO setup used only by those checks. I kept formatting, all-target Clippy, warning-free documentation builds, dependency audits, release compilation, and release-version validation in automation. I retained the local commands and integration runner so I can verify behavior against disposable databases and storage before publishing changes.
