# Acceptance evidence

Phase 21 verifies the service against an explicit fixture workload. It does not establish customer capacity: corpus growth, submission peaks, and replica count are still unspecified.

## Reproduce

Run the static gates in the README, generate and verify the extraction corpus, and run `bash scripts/acceptance.sh` against disposable PostgreSQL and MinIO. Supply the admin database URL, S3 endpoint, and local MinIO credentials documented in the README. Each suite gets a fresh database; the runner never drops a database. Linux is required for successful supervised extraction and worker recovery.

For a Linux acceptance environment on ARM64:

```sh
docker build --target acceptance -f src/extraction/Dockerfile -t whorl-acceptance .
docker run --name whorl-acceptance-integration --cpus 2 --memory 4g --memory-swap 4g \
  -e WHORL_ACCEPTANCE_ADMIN_URL -e WHORL_STORAGE_S3_ENDPOINT \
  -e AWS_ACCESS_KEY_ID -e AWS_SECRET_ACCESS_KEY \
  whorl-acceptance bash scripts/acceptance.sh
```

The endpoints must be reachable from the container. The acceptance image includes build tools and fixtures; it is a test environment. The extraction runtime target remains separate. PostgreSQL and MinIO run outside the application cgroup, and their memory is excluded from application measurements.

Start the API and independent worker with the README commands, ingest the stated corpus, then run:

```sh
python3 scripts/load-acceptance.py --url http://127.0.0.1:8080 \
  --file spikes/extraction/fixtures/table.docx --batches 10 --concurrency 2 \
  --database-url "$DATABASE_URL" --container whorl-acceptance-runtime > load.json
```

The load runner checks every admission and terminal file, samples database pending batch count and oldest pending batch age, reads the shared application cgroup's aggregate peak memory, and probes readiness and polling while work is pending. It records each batch rather than estimating a percentile from a handful of samples. Completion observations include the client's 100 ms polling interval. That interval is measurement overhead, not processing time. Peak memory includes application page cache and all child processes; it is not process RSS. Worker stage-duration logs isolate extraction, signing, candidate verification, and persistence. Retain those logs alongside the JSON.

## Host verification

The November 4, 2026 host is an Apple M1 with 8 GiB RAM, Rust 1.98.1, PostgreSQL 18.6 in Docker, and MinIO RELEASE.2025-10-15T17-29-55Z. Host CPU microbenchmarks are unconstrained macOS runs and must not be presented as two-core Linux capacity results.

All eleven existing PostgreSQL/S3 integration tests passed across ten suites, including both admission tests. The [host integration log](acceptance/integration.txt) explicitly records the Linux end-to-end skip. Tests use fresh databases, real PostgreSQL, and real MinIO. The macOS worker suite exercises extraction failure handling; successful extraction and process recovery require the separate Linux run.

Formatting, Clippy with warnings denied, all targets/features tests, warning-free rustdoc, 48 doctests, and cargo-deny advisories/licenses/sources passed. cargo-deny reported an unused license allowance warning; it did not report a dependency advisory or license violation. The CI workflow now runs all integration suites, including end-to-end acceptance, and builds MinIO from the pinned upstream commit matching the local binary. Hosted CI itself has not been run from this workspace.

## CPU measurements

[Signing measurements](acceptance/signatures.txt) run ten iterations over precomputed shingles. [Batch measurements](acceptance/batches.txt) include normalization, shingling, signing, and bands for one or five files, also over ten iterations. Neither microbenchmark includes extraction, SQL, S3, or queue delivery. Inputs contain unique generated words rather than repeated tokens, so signing work grows with shingle count.

Use `cargo bench --locked --bench signatures` and `cargo bench --locked --bench batches` to reproduce them. The complete batch latency comes from the independent API/worker load run, not these CPU loops.

## Extraction at 20 MiB

The existing [bounded Linux extraction report](extraction-results.json) remains applicable: this phase does not change extraction algorithms or limits. All six exact 20,971,520-byte format probes succeeded under two CPUs and 4 GiB, with swap disabled.

| Format | Wall milliseconds | Maximum process RSS KiB |
| --- | ---: | ---: |
| DOCX | 48.49 | 58,124 |
| JPEG | 269.61 | 172,540 |
| PDF | 18.80 | 10,680 |
| PNG | 385.88 | 166,068 |
| PPTX | 41.04 | 59,400 |
| XLSX | 605.18 | 286,000 |

These are padded boundary fixtures, not complex documents representative of customers. Process RSS excludes simultaneous descendants; the cgroup supplies the aggregate ceiling. The same report includes malformed, expansion, image-limit, OCR, and empty-text cases. Reproduce it with the extraction guide's verification-image command.

## Linux service acceptance

The [Linux integration log](acceptance/linux-integration.txt) records twelve passing ignored integration tests across eleven suites. It includes successful extraction, worker crash/recovery, real S3 upload boundaries, duplicate ingest races, candidate version isolation, polling, retention, and the new cross-layer test. That test admits through HTTP, processes through the actual queue, replays terminal identities, checks canonical deduplication, polls matches, rejects foreign access, and verifies partial extraction failure and an unsupported version snapshot.

The [environment manifest](acceptance/environment.json), [load records](acceptance/load.json), and [worker stage events](acceptance/worker.jsonl) describe the independent API/worker run. Both application processes and extraction descendants shared a two-CPU, 4 GiB cgroup, with swap disabled, a read-only filesystem, a 128 MiB temporary filesystem, no capabilities, and uid 10001. PostgreSQL and MinIO were external. The corpus contained one canonical DOCX fixture; every check was an exact duplicate of that document. This exercises a warm duplicate lookup and does not measure a large LSH candidate set.

All 20 batches and 60 checked files completed successfully, at client concurrency two. The corpus seed also completed. The shared application cgroup's peak memory was 94,687,232 bytes (about 90.3 MiB), including cache and descendants. The eight operational samples observed at most one pending batch and an oldest pending age of 0.083 seconds. Sampling every 200 ms can miss brief queue peaks.

| Files per batch | Maximum admission ms | Median observed completion ms | Maximum observed completion ms | Maximum readiness ms | Maximum polling ms |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 130.54 | 165.66 | 266.93 | 8.38 | 16.82 |
| 5 | 168.63 | 250.14 | 401.45 | 4.65 | 31.47 |

Across 61 processed files, including the seed, median extraction was 6.62 ms, signing 1.23 ms, combined candidate lookup/exact verification 4.18 ms, and persistence 4.75 ms. These stages exclude admission. Maximum combined matching time was 9.93 ms. The extraction maximum of 38.14 ms includes the seed while the test image was still compiling; the load run began after compilation finished.

For this warm duplicate workload, multipart admission and the client's polling interval dominate observed batch latency; signing and candidate verification are smaller costs. This does not establish the bottleneck for OCR, complex archives, or a large corpus. No algorithm or resource limits were changed from these measurements. The demonstrated smoke settings are two workers/handlers, client concurrency two, ten batches per size, and admission ceilings of 100 retained jobs and 1 GiB per configured scope. Those ceilings were not exhausted by this run; separate admission tests verify rejection and lease races.

## Capacity interpretation

Keep two CPU workers maximum, queue handler concurrency equal to the CPU pool, zero CPU waiting slots, and the approved extraction limits. Admission quotas in the smoke setup are explicit test inputs, not production recommendations. Increasing queue quotas cannot increase extraction throughput. The fixture corpus cannot justify a production corpus size, submission rate, latency objective, or a new resource ceiling. Establish those from a representative corpus and arrival pattern before making a production capacity claim.
