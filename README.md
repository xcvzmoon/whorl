# Whorl

Whorl is an asynchronous document similarity service built with Rust, Actix-web, PostgreSQL, and shared S3 storage. It compares uploaded PDF, DOCX, PPTX, XLSX, and image text using lexical Jaccard similarity over word shingles.

The API admits multipart uploads and serves polling results. An independent durable worker extracts text, verifies version-compatible candidates, and commits results and canonical ingests transactionally. Maintenance handles expiry and reindexing. See [configuration and operations](docs/configuration.md) and [acceptance evidence](docs/benchmarks.md).

## Plan and API approval

The [original specification](docs/original-specification.md) preserves the full initial prompt before the planning request. Later clarifications are recorded in the decisions and evidence gates.

The [phase overview](brain/plans/01-document-similarity/overview.md) tracks implementation and acceptance. The [API contract](docs/api-contract.md) and [decisions](docs/decisions.md) record approved behavior. Phase 21 collects end-to-end and capacity evidence; customer workload scale remains unspecified.

## Intended behavior

- Accept one to five files. Reject zero files and request-level limit violations clearly.
- Accept exactly 20,971,520 bytes per file (20 MiB) and at most 104,857,600 content bytes per request (100 MiB). Enforce both while streaming; one byte over either limit is too large.
- Stream to durable S3-compatible storage using bounded memory. An invalid file records its own failure while valid siblings continue.
- Return 202 after durable admission; clients poll batch status/results with ETag, 304, progress stages, and Retry-After.
- Use k=3 word shingles, stable persisted hashes, 125 MinHash values, and 25 bands of five values. PostgreSQL GIN array overlap retrieves version-compatible candidates.
- Short-circuit normalized-text duplicates through a unique content hash. Verify candidates before returning matches strictly above the configurable 0.75 threshold.
- Store metadata as JSONB with a path-ops GIN index; index frequent filters as real columns. Return separate metadata Jaccard scores excluding volatile values. Default qualification uses text similarity.
- Keep CPU work on a fixed, bounded pool. API and workers run as separate binaries; PostgreSQL-backed graphile_worker delivers jobs at least once.

LSH retrieval is probabilistic, as approved. Every retrieved candidate is verified exactly; MinHash estimates only order work. Band hash collisions add candidates for verification. See the approved decisions. Semantic embeddings, vector databases, frontend, SSE, and webhooks are outside version one.

## Development today

Install stable Rust with rustfmt and Clippy, then run:

```sh
cargo run --locked --bin whorl
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps --all-features
cargo build --locked --release
```

Run `cargo run --locked --bin whorl` with the database, S3, signature version, and trusted-proxy configuration described in [multipart submission](docs/uploads.md). Database and queue migrations run separately.

## Deployment and defaults

See [configuration and operations](docs/configuration.md) for environment variables, local `.env` loading, telemetry, alerts, and rollout procedures.

The initial target is two CPU cores and 4 GiB RAM, with at most two CPU workers and handler concurrency no greater than pool size. Use the latest stable PostgreSQL verified at implementation time and shared S3-compatible storage directly. The sibling upload service is not a storage dependency. Use MinIO for S3-compatible storage and run PostgreSQL in Docker. Corpus size/growth, peak concurrency, and instance count still need definition.

| Setting | Specified default / status |
| --- | --- |
| File count | 1..5 |
| File content bytes | 20,971,520 |
| Request content bytes | 104,857,600 |
| Multipart overhead allowance | 1,048,576 bytes; raw cap 105,906,176 |
| Text threshold | Strictly greater than 0.75 |
| Word shingle k | 3; short text uses normalized equality; OCR preserves verbatim text |
| Signature layout | 125 values, 25 bands x 5 rows |
| Metadata policy | Text-only qualification; separate metadata score |
| Completed results and job retention | 24 hours, configurable |
| Suggested polling interval | 2 seconds |
| Queue/pool/storage/client quotas | Determine from measured job times and workload |
| Extraction timeout and decoding/expansion limits | Determine in extraction spike, document before implementation |

Environment variables and fixed limits are documented in the configuration guide. Configure the reverse proxy body cap above the application's raw multipart cap; streaming file-content counters remain authoritative.

## Operations to deliver

The deployment migration step owns application and queue-library schema migrations, with documented privileges separate from runtime roles. Batch/file/results tables are the source of truth; library job rows are not. Retention and abandoned-work sweepers delete expired results, jobs, objects, and capacity leases. Background re-indexing populates a new signature version without mixing versions in comparisons.

Monitor queue depth, oldest pending age, pending stored bytes, CPU saturation, extraction/signing/lookup/verification durations, retries, and failures. Graceful SIGTERM handling and retry/reclaim behavior must be proven in queue tests. Authentication is upstream; trusted client identity is still required for access isolation and per-client admission.

## GitHub automation

CI and tag-release gates run formatting, Clippy, tests, warning-free rustdoc, release builds, and vulnerability/license checks through [cargo-deny](https://github.com/EmbarkStudios/cargo-deny). `deny.toml` permits a narrow set of common dependency licenses; new licenses require review. Whorl is non-publishable, and private workspace packages are excluded from license-expression checks without changing dependency checks or choosing an application license. No runtime crates were added here.

The setup action installs stable Rust and a pinned, checksum-verified prebuilt cargo-deny binary, with source-build fallback disabled. See the [Actions audit](docs/github-actions-audit.md) for measured setup costs. Audit tool upgrades are explicit; application dependencies will be reviewed and deliberately pinned at implementation time. The release workflow checks that a `v*` tag matches Cargo's package version before creating a GitHub release. The PR description check and templates are retained. Publishing and branch-protection configuration remain repository-owner actions; this change does not publish anything.

CI provisions PostgreSQL and builds the pinned MinIO source for integration tests. Acceptance measurements and their limits are recorded separately from CI checks.

## Run and verify

On Linux, install the [extraction runtime and assets](docs/extraction.md), start PostgreSQL and MinIO, and create the S3 bucket. Set `DATABASE_URL`, `WHORL_S3_ENDPOINT`, `WHORL_S3_BUCKET`, `WHORL_S3_REGION`, and AWS credentials as described in the configuration guide. Then:

```sh
cargo build --locked --release --bins
./target/release/maintenance migrate
./target/release/maintenance reindex native 3
./target/release/maintenance reindex ocr 3
export WHORL_SIGNATURE_VERSION='nfkc-v1/native-text-v1/word-3/short-equality-v1/xxh3-low32-le-v1/125/25x5'
export WHORL_TRUSTED_PROXY_ONLY=true
psql -X -v ON_ERROR_STOP=1 "$DATABASE_URL" -c "INSERT INTO capacity_limits VALUES ('global',100,1073741824),('client:acceptance',100,1073741824)"
./target/release/worker
```

Run `./target/release/whorl` in another terminal with the same environment. The `acceptance` identity and example quotas are for isolated local verification. Production identity must come from the authenticated trusted proxy.

```sh
curl --fail http://127.0.0.1:8080/health/ready
curl --fail -H 'X-Client-Id: acceptance' -F 'files=@spikes/extraction/fixtures/table.docx' http://127.0.0.1:8080/v1/ingests
```

Poll the returned `status_url` with the same identity. Schedule `maintenance sweep` every minute. Use the maintenance guide for version cutover and cleanup responsibilities.

For disposable integration databases, set `WHORL_ACCEPTANCE_ADMIN_URL` and `WHORL_STORAGE_S3_ENDPOINT`, create the `uploads` bucket, and supply the local MinIO credentials. Run `bash scripts/acceptance.sh`. Each invocation creates fresh databases and retains them for inspection. Full acceptance requires Linux; macOS runs print an explicit extraction-test skip.

Run `cargo bench --locked --bench signatures` and `cargo bench --locked --bench batches` for CPU microbenchmarks. Against a running API and worker, run `python3 scripts/load-acceptance.py --url http://127.0.0.1:8080 --file spikes/extraction/fixtures/table.docx --database-url "$DATABASE_URL" --container whorl-acceptance-runtime`. This sends checks under the `acceptance` identity; ingest the stated corpus first. Record the corpus, service limits, queue metrics, and aggregate memory alongside the resulting JSON.
