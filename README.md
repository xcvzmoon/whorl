# Whorl

Whorl is a planned asynchronous document similarity service built with Rust, Actix-web, and PostgreSQL. It will compare uploaded PDF, DOCX, PPTX, XLSX, and image text with stored documents using lexical Jaccard similarity over word shingles.

The repository currently contains a Rust starter binary, a phased implementation plan, and GitHub automation. Upload handling, extraction, persistence, queue workers, and HTTP endpoints are not implemented yet.

## Plan and API approval

The [original specification](docs/original-specification.md) preserves the full initial prompt before the planning request. Later clarifications are recorded in the open decisions.

Start with the [phase overview](brain/plans/01-document-similarity/overview.md), [todos](brain/todos.md), [API proposal](docs/api-contract.md), and [open decisions](docs/decisions.md). The API requires approval before implementation. Extraction and queue recommendations require measured spikes; no benchmark results or library choices are claimed yet.

Work proceeds through contract approval, extraction corpus and spike, schema and similarity core, queue spike, bounded CPU execution and S3 storage, extraction adapters, queue workers, admission and HTTP, polling, maintenance, and acceptance testing. Each phase contains reviewable checkbox tasks and verification criteria.

## Intended behavior

- Accept one to five files. Reject zero files and request-level limit violations clearly.
- Accept exactly 20,971,520 bytes per file (20 MiB) and at most 104,857,600 content bytes per request (100 MiB). Enforce both while streaming; one byte over either limit is too large.
- Stream to durable S3-compatible storage using bounded memory. An invalid file records its own failure while valid siblings continue.
- Return 202 after durable admission; clients poll batch status/results with ETag, 304, progress stages, and Retry-After.
- Use k=3 word shingles, stable persisted hashes, 125 MinHash values, and 25 bands of five values. PostgreSQL GIN array overlap retrieves version-compatible candidates.
- Short-circuit normalized-text duplicates through a unique content hash. Verify candidates before returning matches strictly above the configurable 0.75 threshold.
- Store metadata as JSONB with a path-ops GIN index; index frequent filters as real columns. Return separate metadata Jaccard scores excluding volatile values. Default qualification uses text similarity.
- Keep CPU work on a fixed, bounded pool. API and workers run as separate binaries; PostgreSQL-backed graphile_worker is provisional until its spike passes.

LSH retrieval is probabilistic. Estimate-only threshold decisions and collision-free 32-bit band claims need clarification before implementation; see the open decisions. Semantic embeddings, vector databases, frontend, SSE, and webhooks are outside version one.

## Development today

Install stable Rust with rustfmt and Clippy, then run:

```sh
cargo run --locked
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps --all-features
cargo build --locked --release
```

`cargo run` currently prints Hello, world. There is no database or S3 setup command yet. Implementation phases will add migrations, service configuration, and tested deployment instructions instead of documenting commands that do not exist.

## Deployment and defaults

The initial target is two CPU cores and 4 GiB RAM, with at most two CPU workers and handler concurrency no greater than pool size. Use the latest stable PostgreSQL verified at implementation time and shared S3-compatible storage directly. The sibling upload service is not a storage dependency. Corpus size/growth, peak concurrency, S3 provider, and instance count still need definition.

| Setting | Specified default / status |
| --- | --- |
| File count | 1..5 |
| File content bytes | 20,971,520 |
| Request content bytes | 104,857,600 |
| Multipart overhead allowance | Proposed 1,048,576 bytes; raw cap 105,906,176 |
| Text threshold | Strictly greater than 0.75 |
| Word shingle k | 3; short text and OCR policy pending approval/spike |
| Signature layout | 125 values, 25 bands x 5 rows |
| Metadata policy | Text-only qualification; separate metadata score |
| Completed results and job retention | 24 hours, configurable |
| Suggested polling interval | Proposed 2 seconds |
| Queue/pool/storage/client quotas | Determine from measured job times and workload |
| Extraction timeout and decoding/expansion limits | Determine in extraction spike, document before implementation |

These are planned configuration values, not implemented environment variables. Every implemented value will have typed validation and a documented default. Configure the reverse proxy body cap above the application's raw multipart cap; streaming file-content counters remain authoritative.

## Operations to deliver

A deployment migration step will own application and queue-library schema migrations, with documented privileges separate from runtime roles. Batch/file/results tables are the source of truth; library job rows are not. Retention and abandoned-work sweepers will delete expired results, jobs, objects, and capacity leases. Background re-indexing will populate a new signature version without mixing versions in comparisons.

Monitor queue depth, oldest pending age, pending stored bytes, CPU saturation, extraction/signing/lookup/verification durations, retries, and failures. Graceful SIGTERM handling and retry/reclaim behavior must be proven in queue tests. Authentication is upstream; trusted client identity is still required for access isolation and per-client admission.

## GitHub automation

CI and tag-release gates run formatting, Clippy, tests, warning-free rustdoc, release builds, and vulnerability/license checks through [cargo-deny](https://github.com/EmbarkStudios/cargo-deny). `deny.toml` permits a narrow set of common dependency licenses; new licenses require review. No runtime crates were added here.

The setup action installs stable Rust and cargo-deny. Audit tooling follows upstream releases; application dependencies will be reviewed and deliberately pinned at implementation time. The release workflow checks that a `v*` tag matches Cargo's package version before creating a GitHub release. The PR description check and templates are retained. Publishing and branch-protection configuration remain repository-owner actions; this change does not publish anything.

Integration services and corpus benchmarks will be added as the corresponding code exists. Passing the current starter checks does not validate the proposed service.
