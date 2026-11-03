# Configuration and operations

Each executable reads only the current directory's optional `.env` before
starting its async runtime. Existing environment variables win. Missing files
are allowed; malformed or unreadable files fail startup. Keep this file local,
mode 0600, and out of source control. No process changes its environment after
starting threads. Production should inject environment variables directly.

The dotenv parser is pinned to dotenvy 0.15.7 (MIT), already present transitively
through SQLx. Its [upstream source](https://github.com/allan2/dotenvy) and
[API](https://docs.rs/dotenvy/0.15.7/dotenvy/fn.from_path.html) were checked on
November 4, 2026. This adds no new resolved package. No telemetry dependency is
needed: existing serde_json serializes fixed, bounded log fields.

| Variable | Default and validation | Used by |
| --- | --- | --- |
| DATABASE_URL | Required nonempty PostgreSQL connection URL/options accepted by SQLx | All service binaries |
| WHORL_S3_ENDPOINT | Required nonempty URL accepted by object_store; HTTPS in production, HTTP for local MinIO | API, worker, sweep |
| WHORL_S3_BUCKET | Required nonempty existing service-owned bucket | API, worker, sweep |
| WHORL_S3_REGION | Required nonempty signing region | API, worker, sweep |
| WHORL_LISTEN | 127.0.0.1:8080; IP socket address, port 0..65535 | API |
| WHORL_TRUSTED_PROXY_ONLY | Required literal true; deployment must enforce the authenticated proxy boundary | API |
| WHORL_SIGNATURE_VERSION | Required nonempty registered full native version; see maintenance guide | API |
| WHORL_CPU_WORKERS | 2; integer 1..2, also no greater than available hardware parallelism | Worker |
| AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_SESSION_TOKEN | No service default; standard object_store AWS credential chain, optional session token | S3 users |
| WHORL_PDFIUM_LIBRARY | No default; trusted shared-library path | Extraction child |
| WHORL_OCR_DETECTION_MODEL, WHORL_OCR_RECOGNITION_MODEL | No default; trusted ONNX model paths | Extraction child |
| TESSDATA_PREFIX | Tesseract's installed data location unless supplied | Extraction child |

Database and object-store libraries validate connection settings; credentials
are checked when used. Missing extraction assets fail the affected file. A
version must be registered with the matching configuration before submissions.
Configuration errors name the setting without printing its value.
Provider-specific AWS settings remain governed by object_store's credential
and client configuration.

## Fixed limits

These are implementation defaults, not environment tuning knobs. Changing
extraction or queue limits requires benchmark evidence and approval.

- API/worker database pools: 12 connections each; maintenance: 4.
- File: 20 MiB; request: 100 MiB; at most five files. Submission leases,
  deadlines and quotas are described in [admission](admission.md) and
  [uploads](uploads.md). Capacity is configured in `capacity_limits`, with
  nonnegative job/byte ceilings; absent limits fail admission.
- Two CPU workers by default, zero waiting CPU slots. Queue concurrency equals
  CPU workers; three delivery attempts. Heartbeat 30 seconds, sweep 60 seconds,
  stale threshold 300 seconds, recovery delay 30 seconds, shutdown grace 5 seconds.
- Extraction wall deadline: 60 seconds. All archive, page, image, text, memory
  and subprocess defaults are listed in [extraction](extraction.md).
- Results: 86400 seconds; tombstones: another 86400 seconds. Worker upload
  reconciliation: every 60 seconds, at most 32 entries. Snapshot query timeout:
  5 seconds. Schedule maintenance separately every minute.
- Health readiness database deadline: 2 seconds. Storage retries/timeouts are
  set in the [storage implementation](../src/storage/s3.rs) and provider defaults.

## Logs and alerts

Collect stderr as JSON lines for the fixed telemetry events below. Queue-library
diagnostics may also appear; configure the collector to accept non-JSON lines.
Never enrich events with filenames, text, metadata, keys, payloads or tenant IDs.

`stage_duration` emits elapsed seconds and a completion flag for extraction
(including source retrieval), signing, combined candidate lookup/exact verification,
persistence and cleanup. A false flag means the stage exited without successful
completion, including cancellation. CPU closures can briefly outlive cancellation;
this duration measures the awaiting stage, not abandoned thread lifetime.
`cpu_admitted` records occupied slots and total capacity at admission;
`cpu_overloaded` counts rejected CPU admissions. Admission samples are events,
not a continuously sampled utilization gauge.

Once per worker cleanup tick, `operational_snapshot` reports queued/running file
counts, oldest pending batch age in seconds, retained retry attempts beyond the
first, retained failed files, reserved pending bytes (including cleanup-held
reservations), and expired upload journal count. These are database gauges over
retained rows, not monotonic totals. Sum neither snapshots across replicas nor
retry gauges over time. Queue depth represents application files, including
delayed retries, rather than library-internal jobs. Missing snapshots are a
monitoring failure; the worker emits `operational_snapshot_failed`.
Snapshot scans are read-only and bounded by timeout; measure their cost as the
retained dataset grows.

Alert on missing snapshots for two ticks, increasing pending age, repeated
cleanup/snapshot failures, CPU overload, growing reserved bytes near configured
quotas, and failed-file/retry growth. Set workload-specific latency and failure
thresholds from acceptance measurements. Do not infer production capacity from
the extraction fixture corpus.

## Deployment and recovery

Apply [migrations](maintenance.md) with the deployment role before starting
runtime replicas. Provision the bucket and multipart-abort lifecycle rule,
install trusted extraction assets, register signature versions, then configure
capacity limits. Start workers and scheduled maintenance before admitting API
traffic through the authenticated proxy. Restrict API ports to that proxy.

Stop admission and let active jobs drain before stopping workers. Queue delivery
is at least once; attempt fences and transactional writes prevent duplicate
canonical ingests. After an outage, restore PostgreSQL/S3 access and restart
workers. Inspect exhausted queue rows and unfinished application files before
operator replay; do not manually mark files successful. Follow
[worker recovery](worker.md) and [queue policy](queue-adapter.md).

Run `maintenance sweep` every minute and retry failures. Upload deletion keeps
its reservation until S3 cleanup succeeds. Keep the multipart lifecycle rule
enabled for crashes before journal updates. Retention removes results and source
objects irreversibly; back up both PostgreSQL and S3 according to the deployment's
recovery requirements. Follow [maintenance](maintenance.md) for bounded cleanup,
restartable reindexing and version rollout ordering.
