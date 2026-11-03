# Queue spike

Phase 10 evaluates the Rust [graphile_worker 0.14.1](https://crates.io/crates/graphile_worker/0.14.1) release against PostgreSQL 18.6. The executable and repeatable assertions are in `spikes/queue/`. This adds no queue dependency to the application.

## Dependency audit

Checked November 3, 2026 using the crates.io API, [upstream repository](https://github.com/leo91000/graphile_worker_rs), published crate sources, and [official API documentation](https://docs.rs/graphile_worker/0.14.1/graphile_worker/). The compact API snapshot is `spikes/queue/evidence/dependency-audit.json`.

- Release 0.14.1 was published October 11, after 0.14.0 on October 8. Several 0.13 releases also landed in October. The latest repository commit was October 29 (`4d536e95e509c13d79df96aee67f8cfd880ee31c`).
- 89,804 total downloads and 48,376 recent downloads were reported by crates.io. These counts indicate usage, not correctness.
- MIT license; no declared MSRV in the published package. The locked spike builds on Rust 1.98.1. An older compiler minimum has not been established.
- SQLx 0.9.0 matches the application. Default features enable Tokio, SQLx and Rustls. The separate lockfile pins the library's split crates, including migrations 0.4.24, queries 0.2.1 and recovery 0.1.6.
- Six open repository items included dependency automation, a release PR and a job-dependency feature request. This snapshot showed recent maintenance, but it does not establish a support response SLA.

## Lifecycle contract

The probe uses two handler slots, a two-second poll interval, and database time. Test recovery settings are one-second heartbeats/sweeps, a six-second stale threshold and a one-second recovery delay. These shortened timings exercise recovery; they are not production defaults.

The runner asserts pending-key deduplication, successful-row deletion, three bounded failure attempts, permanent error retention, notification delivery, scheduled work with notifications disabled, a 185-second live job under competing sweeps, SIGKILL recovery and SIGTERM shutdown. Events are recorded in an independent application table so deletion of queue rows cannot erase execution evidence.

The queue uses exponential retry delay `exp(min(attempts, 10))` seconds. With three attempts, the first two waits are approximately 2.718 and 7.389 seconds, plus polling/scheduling overhead. Exhausted rows retain `attempts`, `max_attempts` and `last_error`; successful rows are deleted. Failed-row cleanup must be scheduled explicitly in operations. Neither row lifetime defines the application's result retention.

Default recovery is disabled. When enabled, stale-worker heartbeats trigger release and delayed retry. A healthy multi-minute handler must keep its ownership while heartbeats continue. Recovery requires another running sweeper; downtime without a survivor adds to the recovery window. Defaults are 30-second heartbeat, 60-second sweep, five-minute threshold and 30-second recovery delay. Resilient job flags extend the threshold by a default factor of three.

SIGTERM allows five seconds of graceful draining by default. An unfinished async handler is then cancelled and its job released with a 30-second retry delay. This does not stop a Rayon thread or an external process. The shared CPU pool retains admission until actual completion, and supervised extraction stops its subprocesses on cancellation. See the [worker lifecycle](worker.md) for the integrated shutdown behavior.

Pending job keys return the same job id. Replacing a running key creates a successor with another id; both handlers can execute. Keys are scheduling deduplication, not an execution fence. A process pause, database outage, or heartbeat starvation can make a live process appear dead. Use at-least-once semantics: authoritative file-state checks and transactional result/ingest updates must prevent repeated side effects. Do not release capacity or delete staged objects solely because a heartbeat expired. No exactly-once claim follows from this experiment.

## Migration and privilege ownership

The library owns its versioned queue schema and SQL functions. `WorkerOptions::init` checks migrations and applies missing ones; the API must use `WorkerUtils::new` rather than initializing a worker. Run `WorkerUtils::migrate` under a deployment migration role before starting replicas. A new installation needs database CREATE plus ownership of its schema; upgrades need ownership of the existing tables/functions. Runtime processes should have no schema CREATE privilege and should fail deployment if migrations are missing, rather than becoming migration owners.

`spikes/queue/privileges.sql` creates a temporary NOLOGIN enqueue role inside a transaction and rolls everything back. Enqueue needs schema USAGE, SELECT/INSERT on tasks and named queues, SELECT/INSERT/UPDATE on jobs, INSERT on job retirements, and sequence USAGE. Queue tables have RLS enabled: corresponding role policies are also required. The probe checks new named queues and repeated keys without granting DELETE or schema CREATE. PostgreSQL's default PUBLIC function EXECUTE applies in this test; a hardened deployment must explicitly grant the enqueue functions after revoking PUBLIC access. These grants permit direct table writes, so this is a trusted backend role, not a tenant authorization boundary. Tenant scoping remains in application code.

The enqueue-only Rust utility runs with an existing SQLx pool. `with_executor(&mut *transaction)` accepts an application transaction; the executable adds a typed job and rolls it back, then verifies absence through another connection. SQL `add_job` likewise rolls back with its surrounding transaction. Admission records, reservations and queue insertion can therefore commit atomically in one PostgreSQL database. MinIO writes remain a separate lifecycle.

## Reproduce

Use a fresh, disposable PostgreSQL 18.6 database and a role allowed to migrate and create a temporary test role. The runner retains queue/application evidence and does not reset existing databases. The PostgreSQL image digest is recorded in [persistence](persistence.md).

```sh
cargo build --locked --manifest-path spikes/queue/Cargo.toml
WHORL_QUEUE_TEST_URL=postgresql://postgres@localhost:55434/whorl_queue_test \
  python3 spikes/queue/run.py
cargo fmt --check --manifest-path spikes/queue/Cargo.toml
cargo clippy --locked --manifest-path spikes/queue/Cargo.toml --all-targets -- -D warnings
```

Allow about five minutes. Database URLs stay in environment variables and are not written to evidence. The probe stores only synthetic labels, durations and failure flags in job payloads. Production payloads should contain record ids, never storage credentials or document bodies.

## Recorded run

All lifecycle assertions passed on November 3, 2026. `spikes/queue/evidence/results.json` records the measured notification observation, retry intervals, recovery and shutdown times. The 185-second handler started once under competing sweeps. SIGKILL produced a second start and one completed execution after recovery; SIGTERM cancelled its long handler, returned successfully and left an unlocked delayed retry. Exhausted failures remained after the multi-minute run. Utility rollback, enqueue-role SQL and running-key replacement were checked separately against the same database and included in the final runner.

Application tests, 24 doctests, formatting, Clippy with warnings denied and warning-free rustdoc passed. The ordinary application suite skips its existing PostgreSQL integration tests; this experiment ran the queue-specific database assertions directly. The spike has no unit tests; its behavior is exercised by the PostgreSQL runner.

## Adoption decision

Retain graphile_worker for the planned adapter. No tested requirement requires evaluating apalis-postgres, and no library switch was made. Adoption is limited to the measured at-least-once contract. The [adapter](queue-adapter.md) pins this release and separates migration ownership from enqueue-only API startup. The [worker](worker.md) implements authoritative state transitions and transactional, retry-safe results. Production heartbeat thresholds, retry limits and retention cleanup still require workload and failure-budget calibration.
