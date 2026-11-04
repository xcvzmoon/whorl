# Whorl documentation

Whorl accepts document batches, extracts their text in isolated workers, and compares them with a client-scoped canonical corpus. This documentation describes the implementation in this repository. Historical evidence is dated separately from current operating instructions.

## Understand the implementation

| Guide | What it covers |
| --- | --- |
| [Architecture and code map](architecture.md) | Processes, module ownership, durable flow, transactions, failure recovery, and source tree |
| [Similarity algorithm](similarity.md) | Normalization, shingles, signatures, candidate retrieval, exact scoring, and version isolation |
| [API contract](api-contract.md) | Requests, responses, status/error semantics, conditional polling, and expiration |
| [Persistence](persistence.md) | Schema, indexes, capacity accounting, candidate lookup, and canonical identity |
| [Extraction](extraction.md) | Formats, native tools, sandbox limits, assets, and corpus verification |
| [CPU pool](cpu-pool.md) | Admission, deadlines, cancellation, and shutdown |
| [Architecture review](architecture-review.md) | Implemented signature, lifecycle, and attachment corrections with regression coverage |

## Run and operate

| Guide | What it covers |
| --- | --- |
| [README quick start](../README.md#run-locally) | Prerequisites, build, initialization, and first ingest |
| [Configuration and operations](configuration.md) | Environment variables, fixed limits, deployment, telemetry, alerts, and recovery |
| [Multipart submission](uploads.md) | Streaming limits, durable admission, and request cleanup |
| [Admission](admission.md) | Trusted identity, global/client budgets, lease transfer, and overload |
| [Shared storage](storage.md) | S3 setup, multipart journal, reservations, and reconciliation |
| [Queue adapter](queue-adapter.md) | Transactional enqueue, eligibility, retry policy, and migrations |
| [Worker](worker.md) | Processing, visibility, retry fencing, shutdown, and recovery |
| [Maintenance](maintenance.md) | Migrations, expiration, tombstones, reindexing, and version cutover |

## Develop and verify

- [Development and testing](development.md): commands, integration prerequisites, suite map, CI, and contribution expectations.
- [Engineering principles](engineering.md): resource constraints and change discipline.
- [Benchmarks and acceptance](benchmarks.md): measured workloads, reproduction, raw evidence, and capacity limits.
- [Extraction research](extraction-spike.md) and [queue research](queue-spike.md): evidence behind implementation choices.
- [Actions audit](github-actions-audit.md): historical CI cost and setup measurements.

## Decisions and history

- [Approved decisions](decisions.md) and [original specification](original-specification.md).
- [Project history](history.md), a chronological account of implementation, corrections, and verification.

The history describes what I implemented and verified as the application grew. Current source, operating guides, and the architecture review describe today's behavior and remaining limitations.
