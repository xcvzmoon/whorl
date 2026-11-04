# Whorl

Whorl is an asynchronous document similarity application built with Rust, Actix-web, PostgreSQL, and S3-compatible storage. Submit PDFs, Office documents, or images, then poll for lexical similarity matches against a client-scoped document corpus. Ingest submissions also add canonical documents to that corpus.

Text extraction runs in isolated Linux processes. Independent durable workers normalize text, retrieve likely candidates using MinHash/LSH, verify those candidates with exact Jaccard similarity, and persist results transactionally. The HTTP process handles streaming uploads and polling independently of CPU-heavy work.

## Capabilities

- PDF, DOCX, PPTX, XLSX, PNG, and JPEG extraction, including OCR for scanned PDF pages and images.
- One to five files per batch, with inclusive 20 MiB/file and 100 MiB combined content limits enforced during streaming.
- Separate check and ingest routes, durable per-file outcomes, partial failures, progress stages, and conditional polling with ETags.
- Client-scoped corpus, results, and quotas using identity supplied by an authenticated reverse proxy.
- Canonical normalized-text identity, first-ingest metadata preservation, versioned signatures, and exact verification of retrieved candidates.
- PostgreSQL-backed at-least-once jobs, bounded CPU concurrency, attempt fencing, journaled S3 cleanup, retention, and restartable reindexing.

Matching is lexical, not semantic. LSH retrieval is probabilistic and can miss qualifying corpus documents. HTTP currently uses text-only qualification strictly above 0.75 with a versioned word-shingle length (initially k=3), 125 MinHash values, and 25 bands of five. The Rust scoring module also supports weighted metadata policies. There is no HTTP policy selector, frontend, SSE, or webhook delivery.

## Documentation

Start with the [documentation index](docs/index.md). The main guides are:

- [Architecture and code map](docs/architecture.md)
- [Similarity algorithm](docs/similarity.md) and [API contract](docs/api-contract.md)
- [Configuration and operations](docs/configuration.md)
- [Development and testing](docs/development.md)
- [Maintenance and reindexing](docs/maintenance.md)
- [Acceptance measurements](docs/benchmarks.md)
- [Architecture review and recommended work](docs/architecture-review.md)

The [project history](docs/history.md) describes the implementation, decisions, and corrections in chronological order. [Approved decisions](docs/decisions.md) explain deviations from the [original specification](docs/original-specification.md).

## Run locally

Successful supervised extraction requires Linux, stable Rust, PostgreSQL, an S3-compatible bucket such as MinIO, and the native extraction tools/models listed in the [extraction guide](docs/extraction.md). The recorded deployment target is two CPU cores and 4 GiB RAM. The extraction image currently packages Linux ARM64 assets; check its architecture before using it on another host.

Provision PostgreSQL and MinIO, create a service-owned bucket, and configure its incomplete-multipart abort lifecycle rule. Install extraction assets and build all binaries together so the worker finds its sibling `whorl-extract` executable:

```sh
cargo build --locked --release --bins
```

Copy [`.env.example`](.env.example) to an untracked local `.env`, replace the credential placeholders, and configure the proxy confirmation and extraction assets:

```sh
cp .env.example .env
chmod 600 .env
```

Existing environment variables take precedence. The example keeps `WHORL_TRUSTED_PROXY_ONLY=false` until the proxy contract is configured. The equivalent environment settings are:

```sh
export DATABASE_URL=postgresql://localhost/whorl
export WHORL_S3_ENDPOINT=http://127.0.0.1:9000
export WHORL_S3_BUCKET=uploads
export WHORL_S3_REGION=us-east-1
export WHORL_CPU_WORKERS=2
# Supply AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY securely.
# Configure PDFium, OCR models, and Tesseract as documented in docs/extraction.md.
```

Use the deployment schema-owner role for migration, then the restricted runtime role for the other processes. On a new local database:

```sh
./target/release/maintenance migrate
./target/release/maintenance reindex native 3
./target/release/maintenance reindex ocr 3
export WHORL_SIGNATURE_VERSION='nfkc-v1/native-text-v1/word-3/short-equality-v1/xxh3-low32-le-v1/125/25x5'
psql -X -v ON_ERROR_STOP=1 "$DATABASE_URL" -c "INSERT INTO capacity_limits VALUES ('global',100,1073741824),('client:acceptance',100,1073741824)"
```

The example quotas and `acceptance` identity are for disposable local verification. Each submission reserves five attempts and 100 MiB before reading content, so limits must accommodate that reservation. Production quotas require workload measurements.

The worker derives the shingle length from the selected signature version and uses the detected native/OCR namespace. Register both source profiles before processing mixed batches. For a different k, follow the coverage and cutover procedure in [maintenance](docs/maintenance.md).

Whorl requires an authenticated proxy to strip and replace `X-Client-Id`, with network policy preventing direct public access. For isolated loopback verification, set the explicit deployment confirmation and run the worker and HTTP process in separate terminals with the same environment:

```sh
export WHORL_TRUSTED_PROXY_ONLY=true
./target/release/worker
```

```sh
export WHORL_TRUSTED_PROXY_ONLY=true
./target/release/whorl
```

The API listens on `127.0.0.1:8080` by default. Verify readiness and submit a native-text document:

```sh
curl --fail http://127.0.0.1:8080/health/ready
curl --fail -H 'X-Client-Id: acceptance' \
  -F 'files=@spikes/extraction/fixtures/table.docx' \
  http://127.0.0.1:8080/v1/ingests
```

Poll the returned `status_url` with the same identity. Use `/v1/checks` to compare without adding corpus content. Schedule `maintenance sweep` every minute; workers also reconcile source uploads. See [configuration](docs/configuration.md) for all settings, startup validation, telemetry, alerts, and recovery responsibilities.

## HTTP routes

| Route | Purpose |
| --- | --- |
| `POST /v1/ingests` | Admit files for comparison and canonical ingestion |
| `POST /v1/checks` | Admit files for comparison without corpus writes |
| `GET /v1/batches/{id}` | Stream persisted status and matches; supports `If-None-Match` |
| `GET /health/live` | Process liveness without dependencies |
| `GET /health/ready` | PostgreSQL readiness with a two-second deadline |

Submissions return 202 after durable batch/file/job commit, including all-invalid batches. Valid siblings survive individual file failures. Terminal results expire after 24 hours, followed by another 24 hours of owner-visible tombstones. Canonical corpus documents remain independently. The [API contract](docs/api-contract.md) defines response fields and errors.

## Repository structure

```text
src/http/          Streaming submission, polling, health, errors
src/service/       Admission, matching, durable processing, maintenance
src/similarity/    Normalization, shingles, signatures, exact scoring
src/extraction/    Format adapters, limits, subprocess supervision
src/storage/       S3 uploads, journals, capacity, reconciliation
src/queue/         Transactional enqueue and durable job payload
src/persistence/   Canonical document and candidate queries
src/bin/           Worker, maintenance, extraction executables
database/          Schema migrations and application SQL queries
docker/            Production extraction images and verification builds
tests/             Deterministic and external integration suites
tests/fixtures/    SQL assertions and signature golden vectors
benches/           CPU benchmarks
scripts/           Acceptance and reproduction tools
spikes/            Extraction/queue research and fixture evidence
docs/              Architecture, operations, decisions, delivery history
```

## Development

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
cargo test --locked --doc --all-features
RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps --all-features
```

Tests run locally, including opt-in PostgreSQL/S3 suites. CI and release workflows run formatting, Clippy, documentation builds, dependency audits, and release builds without executing tests. Follow [development and testing](docs/development.md) for the full suite map, `scripts/acceptance.sh`, native extraction checks, dependency audits, and benchmark reproduction. Recorded acceptance demonstrates fixture behavior under stated constraints; representative customer corpus size and peak arrival rate remain unspecified.

Contributions should follow [AGENTS.md](AGENTS.md) and include focused verification. The package is marked non-publishable. No application license is currently declared; fixture, model, and dependency licenses are recorded separately and must be respected when redistributing those assets.
