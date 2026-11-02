You are a senior Rust backend engineer. Design and implement a document similarity service using Actix-web and PostgreSQL. Work in small, reviewable steps, explain trade-offs when you make a non-obvious choice, and ask before deviating from anything specified here.

## 1. Goal

Users upload between one and five files per request (one is enough; five is the maximum, not a requirement). Supported types: PDF, DOCX, PPTX, XLSX, and images. A request with zero files is rejected with a clear error.

The server extracts text content and metadata from each file, compares them against documents already stored in the database (text_content and metadata columns), and returns the ids of every stored document whose similarity score is greater than 75%. Similarity is Jaccard similarity over word shingles. Results are returned per uploaded file.

Non-goals: semantic or embedding-based similarity, a separate vector database, a frontend, authentication (assume it is handled upstream unless I say otherwise).

## 2. Key design decisions (already made)

- Language and framework: Rust with Actix-web. CPU-heavy work (extraction, OCR, shingling, hashing) must never run on an async executor thread.
- Storage: PostgreSQL only. No vector database. Jaccard is a lexical set-overlap measure, so embeddings are the wrong tool.
- Candidate retrieval: MinHash signatures plus Locality-Sensitive Hashing (LSH banding), indexed with a GIN index over an array of band hashes. This avoids comparing an upload against every row.
- Verification: candidates are verified after retrieval, so LSH false positives never reach the response.
- Processing model: asynchronous, using a durable Postgres-backed job queue built on an existing library (the graphile_worker crate, subject to the spike in section 6). Not hand-rolled.
- Result delivery: polling in the first version.

## 3. Similarity design

Normalization (applied identically at ingest and query time):
- Unicode-normalize, lowercase, strip punctuation, collapse whitespace.
- Optionally remove lines that repeat across many pages (headers, footers, page numbers) before shingling.

Shingling:
- Word shingles with k = 3 by default, configurable.
- Documents with fewer than k words need a defined fallback (smaller k, then exact-match comparison). Document the behavior.
- OCR output is noisy; evaluate whether a smaller k or character shingles work better for image sources, and keep that decision consistent between ingest and query for the same source type.

MinHash and LSH:
- Signature of 125 hash values per document.
- LSH banding of 25 bands x 5 rows. Rationale: with this layout a pair at true Jaccard 0.75 becomes a candidate with probability above 99%, while pairs around 0.3 rarely do. A 16x8 layout would miss roughly one in five true matches at the threshold, so do not use it.
- Each band is hashed to a single 32-bit value with the band index mixed in, so different bands cannot collide with each other. Stored as an integer array.
- Hash functions used for persisted data must be stable and deterministic across processes, builds, and library upgrades. Do not use Rust's default hasher or any randomized-seed hasher for anything stored in the database.

Verification:
- Estimate Jaccard from signature agreement for each candidate.
- Discard candidates clearly below the threshold, accept those clearly above, and compute exact Jaccard (re-shingling the stored text) only for candidates near the 0.75 line. Choose the margin from the estimator's standard error and state it in the code comments.
- Threshold is strictly greater than 0.75 and must be configurable.

Exact duplicates:
- Store a content hash of the normalized text with a unique index and short-circuit identical documents before any MinHash work.

Signature versioning:
- Store a signature version on every row covering shingle size, normalization rules, seeds, and band layout. Queries only compare against rows with the same version. Provide a path for background re-indexing when the version changes.

## 4. Metadata handling

- Store metadata as JSONB with a GIN index using the path-ops operator class for containment queries.
- Promote frequently filtered fields (for example file type) to real columns with B-tree indexes.
- Metadata similarity is separate from text similarity: flatten metadata into a set of key=value tokens and compute plain Jaccard on those small sets.
- Exclude volatile fields (creation date, modification date, file size) from similarity tokens.
- Default policy: the 75% rule applies to text similarity; the metadata score is returned alongside it as supporting information and can be used as a tie-breaker. Make the policy configurable (text-only, or a weighted combination) and document the default.

## 5. Upload limits and extraction pipeline

### Upload limits (hard requirements)

- Per file: maximum 20 MiB, defined as exactly 20 x 1024 x 1024 = 20,971,520 bytes. A file of exactly 20,971,520 bytes is accepted; a file one byte larger is rejected. (Confirm with me if "20 MB" should instead mean 20,000,000 bytes.)
- Per request: between 1 and 5 files, and a combined total of at most 100 MiB (104,857,600 bytes) of file content. Enforce the total explicitly.
- Enforce limits while streaming the multipart body, counting bytes per file as they arrive, and abort as soon as a limit is crossed. Never buffer an entire oversized upload first, and never trust the Content-Length header alone.
- The raw request body is slightly larger than the file content because of multipart boundaries and part headers. Set the HTTP body cap to the 100 MiB content limit plus a small fixed overhead allowance, and let the per-file and total byte counters be the authoritative checks. Document that any reverse proxy in front (for example, its maximum body size setting) must be configured above this value.
- Per-file rejections (file too large, unsupported type, empty file) return a specific error and must not discard the other files in the request when they are valid. Request-level violations (total over 100 MiB, more than 5 files, no files) reject the whole request.
- Memory safety: do not hold up to 100 MiB per request in RAM. Stream uploads to durable shared storage and cap the total pending stored bytes (see backpressure in section 6).
- Decompression and decoding limits: cap the uncompressed size and compression ratio for DOCX, PPTX, and XLSX archives, cap the pages and sheets processed, and cap image pixel dimensions before decoding (a 20 MiB image can expand to hundreds of MiB as a bitmap).
- Extraction timeout per file, configurable.
- All values above are configurable, with these as defaults.

### Extraction

Per file type, extract plain text and metadata:
- PDF: text and document info metadata.
- DOCX and PPTX: these are zipped XML; extract body text (and slide text and notes for PPTX) and core document properties.
- XLSX: cell text across all sheets, plus workbook properties.
- Images: OCR for text, plus image metadata such as EXIF where present.

Requirements:
- Validate the real file type from content (magic bytes), not only the extension or declared MIME type.
- A failure on one file must not fail the others; record a per-file status.
- Treat uploads as untrusted: guard against zip bombs, malformed files, and pathological XML. Never write uploads to predictable paths; use secure storage and clean up on every path.
- Rust's document extraction ecosystem is thinner than JavaScript's. Begin with a short spike: test the candidate crates against a small corpus of real files (scanned and digital PDFs, Word and PowerPoint with tables and images, large spreadsheets up to the size limit, photos of text), report extraction quality and speed, and recommend a final choice per format before building on it. If a native Rust option is not good enough for a format, propose calling a maintained external tool as a subprocess with strict timeouts and resource limits.

## 6. Architecture

### Layers
- Actix-web HTTP layer: multipart upload handling, request validation, response shaping, health endpoint.
- A service layer with clear interfaces: extraction, normalization and shingling, signature generation, candidate retrieval, verification, persistence. The similarity logic must live behind a small trait or interface so it can be unit-tested without HTTP or a database.
- Database access through an async Postgres driver with a connection pool, migrations tracked in the repo, and queries that use the GIN array-overlap operator for candidate retrieval.
- Structured errors with a consistent JSON error format; no panics on user input.

### CPU work stays off the async executor
- Async runtimes schedule cooperatively, so a long CPU loop on an executor thread blocks every other task on that thread (other requests, database responses, timers). All extraction, OCR, shingling, and signing runs on a dedicated, fixed-size CPU thread pool sized to the number of CPU cores, bridged back to async code with a oneshot channel. Do not use the general blocking pool for CPU-heavy work, because it can spawn many threads and oversubscribe the cores.
- The pool has a bounded waiting list. When all workers are busy, jobs wait in the bounded list; when it is full, new work is rejected immediately (503 or 429 with a Retry-After header) instead of queueing without limit. Size the list from the formula: workers x (acceptable wait / typical job time). Stamp each unit of work with a deadline and skip work whose deadline has passed. Check capacity before accepting a large upload, so no work is done for a request about to be refused.

### Asynchronous processing with a job queue
- Use a durable, Postgres-backed job queue built on an existing library, not a hand-rolled job table. The chosen library is the graphile_worker crate with its sqlx driver. Requests are asynchronous: the API validates and streams uploads to durable shared storage, creates a batch and one job per file, and returns 202 Accepted with a batch id. One job per file lets a batch's files run in parallel across workers and lets one corrupt file fail alone.
- Durable shared storage is required because workers may run on other instances. Evaluate the existing RustFS-backed upload service (self-hosted S3-compatible storage) for this; the alternative is a shared volume. Stored uploads are deleted after their job finishes, or after a configurable retention period.
- Workers run as a separate binary from the HTTP server. The API enqueues through the library's utility interface without running a worker loop.
- Worker handlers run on the async runtime, so they must never perform extraction, OCR, shingling, or signing directly. Each handler hands that work to the dedicated CPU pool, awaits the result, then writes results to Postgres. The worker's configured concurrency must not exceed the CPU pool size.
- Before building on the library, run a short spike and report findings: (1) wake-up behavior and fallback polling; (2) retry and backoff configuration, and how permanent failures are distinguished from retryable ones and recorded; (3) how a long-running job (up to several minutes) is protected from being reclaimed while still running, and the dead-worker recovery window; (4) idempotent enqueue using job keys; (5) graceful shutdown behavior; (6) whether and for how long completed and failed jobs are retained; (7) the crate's release cadence, recent commit activity, open issues, and MSRV. If a requirement cannot be met, evaluate apalis-postgres next and report before switching.
- The library creates and migrates its own schema. Decide and document which process runs those migrations and what database privileges are required. Keep application tables (batches, per-file status, results, signatures, documents) in our own migrations, and do not treat the library's job rows as the system of record for batch status or results.
- Hide the queue behind a small interface so the library could be replaced later. Make each file's work a self-contained job (input: a reference to stored file bytes; output: text, metadata, signature, and match results), so the same core function is used by the worker and in tests.
- Reliability rules: bounded retries with backoff, retryable versus permanent failures, idempotent writes (upserts keyed by job or content hash), a failed state with recorded errors, a sweeper for abandoned work, and graceful shutdown on SIGTERM so in-flight jobs finish or are released.
- Backpressure applies at enqueue time: reject new batches with 503 or 429 and a Retry-After header when pending jobs or pending stored bytes exceed configured limits, with per-client limits so one caller cannot fill the queue.
- Retention: completed batch results and job rows expire after a configurable period (default 24 hours).
- Observability: queue depth, age of the oldest pending job, duration per stage (extraction, signing, candidate lookup, verification), retry and failure counts.

## 7. API contract (describe before implementing, and wait for my approval)

The API is asynchronous. Propose request and response shapes for:
(a) the submit endpoint (check and ingest as separate job types, or a mode flag; justify the choice), accepting one to five files, returning a batch id;
(b) the status and result endpoint, with batch state (queued, running, completed, partially failed, failed) and per-file state;
(c) delivery by polling in the first version: the status endpoint is the source of truth, supports ETag and 304, returns per-file progress, and includes a suggested poll interval via Retry-After. Optionally support short long-polling (holding the request open for a few seconds when nothing has changed). Do not implement SSE or webhooks now, but keep result delivery separate from job processing so either can be added later without changing the data model;
(d) error and rate-limit responses.

Completed results list, for each file, the matching document ids above the threshold in descending score order, with the text similarity score (and metadata score if enabled).

## 8. Dependency guidelines

Only add a crate if it is performant, actively maintained, and widely used. Before adding any dependency, check on crates.io and the repository: a release within roughly the last 12 months, recent commit activity, healthy download numbers, a responsive issue tracker, a compatible license, and a reasonable MSRV. Look up the latest versions at implementation time rather than relying on remembered version numbers. Run a vulnerability and license audit in CI. Prefer fewer dependencies, and note why each one is needed. Note that some recommended crates may be pre-1.0; call that out and pin versions deliberately.

Starting suggestions (verify each against the criteria above):
- Web and runtime: actix-web, actix-multipart, tokio.
- Database: sqlx (PostgreSQL, with migrations, uuid and JSON support) with a pooled connection.
- Job queue: graphile_worker (fallback candidate: apalis-postgres).
- Serialization and errors: serde, serde_json, thiserror, and anyhow at application edges.
- CPU pool: rayon, or fixed threads fed by a bounded channel (for example crossbeam-channel), plus tokio's oneshot channel to return results to async code.
- Stable hashing for shingles and signatures: xxhash-rust (xxh3). For the whole-document content hash: blake3 or sha2.
- Text handling: unicode-normalization and unicode-segmentation.
- XLSX: calamine.
- DOCX and PPTX: zip plus quick-xml to read the XML parts directly.
- PDF: evaluate lopdf (pure Rust) against pdfium-render (higher fidelity, needs the PDFium library at runtime) in the spike; avoid less-maintained PDF text crates unless they pass the criteria.
- Images and OCR: image for decoding, kamadak-exif for EXIF, and for OCR compare native Tesseract (through a maintained binding or a subprocess) against the pure-Rust ocrs crate on accuracy and speed before choosing.
- Ids: uuid.
- Observability: tracing, tracing-subscriber, and tracing-actix-web.
- Configuration: a typed configuration approach (environment variables with a .env loader for local development).
- Testing and benchmarking: criterion for benchmarks, proptest for property tests, and testcontainers for integration tests against a real PostgreSQL.
- Optional: actix-governor for rate limiting, utoipa for OpenAPI documentation.

## 9. Quality bar

- Unit tests for normalization, shingling, signature generation, banding, and the estimator, including deterministic expected values so signature stability across builds is verified.
- Property tests: identical texts give Jaccard 1.0 and an identical signature; disjoint texts give a score near 0; a small edit to a large document stays above 0.75 while a large edit drops below it.
- A recall test: generate document pairs at known Jaccard values and confirm the LSH layout retrieves pairs at or above 0.75 with the expected probability.
- Upload limit tests: a file of exactly 20,971,520 bytes is accepted and 20,971,521 bytes is rejected; five files of that size are accepted (exactly 100 MiB total); a total over the limit, a sixth file, and a request with no files are rejected; one file as the sole upload works; limits are enforced while streaming without buffering the whole body.
- Queue tests: concurrent workers never process the same job twice; a killed worker's job is reclaimed after the recovery window; a job that outlives the recovery window is not processed twice; retries stop at the limit; permanent failures are not retried; shutdown during a job leaves it recoverable; handler concurrency never exceeds the CPU pool size; backpressure triggers at the configured limits.
- CPU pool tests: a flood of heavy requests leaves health checks and status polling responsive; a full waiting list produces fast 503 or 429 responses.
- Integration tests against PostgreSQL covering ingest, check, signature version mismatch, per-file failure handling, and status polling (including ETag behavior).
- Benchmarks for extraction per format (including a 20 MiB file of each type), signature generation per document size, and end-to-end latency for a five-file batch against a large corpus. Report results and identify the actual bottleneck before suggesting any optimization.
- Clippy and rustfmt clean, no unwrap or expect on user-controlled paths, and documentation for every configurable value and its default.

## 10. Deliverables and order of work

1. Confirm the plan, ask any clarifying questions (corpus size, expected concurrency, deployment target, MiB versus MB, durable storage choice), and propose the API contract.
2. Extraction spike with a written recommendation per format.
3. Database schema and migrations, then the similarity core with tests and benchmarks.
4. Queue library spike with findings, then the worker binary and queue tests.
5. HTTP layer, CPU pool with bounded waiting list, upload limit enforcement, and status endpoint.
6. Integration tests, load test, and a short README covering setup, configuration, operations (migrations, retention, re-indexing when the signature version changes), and monitoring.
