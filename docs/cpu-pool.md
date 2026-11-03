# Shared CPU executor

Create one `Arc<CpuPool>` per process and pass it to extraction, signing and the matcher. Configure one or two workers, no greater than available parallelism. The two-core deployment uses two. No stage should construct its own pool or use Tokio's general blocking pool for CPU work.

Admission counts running and waiting operations together. Waiting capacity is `floor(workers * acceptable_wait / measured_typical_job_time)`. Timing inputs are required, zero typical duration is rejected, and zero acceptable wait provides no waiting slots. Full admission returns `CpuError::Overloaded` without waiting. The HTTP adapter must translate that into the approved overload response. This phase adds no HTTP endpoint.

Measure the operation being submitted, including its realistic input sizes. Do not use signature-only timings to size extraction or a complete file job. On the development host, `cargo bench --bench signatures` measured about 3.53 ms for a 10,000-word signature and its bands (10 iterations). If that were the submitted unit, two workers and 10 ms acceptable wait would allow five waiting slots. This is an arithmetic example, not a production setting. The extraction workload and target-host timings are still needed before choosing a nonzero production waiting budget.

Pass the same absolute `WorkDeadline` through every CPU stage. Queued jobs check it before invoking user work. Dropping a submission future cancels queued work and signals running work through `WorkContext::should_stop`. Matching checks that context between candidates; a single signature, normalization or exact comparison must finish before it can stop. Results completed after the deadline return `CpuError::Expired`. Cancellation retains the slot and owned inputs until the worker exits, so repeated cancellation cannot admit unbounded background work.

`shutdown().await` stops admission, skips waiting closures and waits for all admitted work to release its slots. Running closures must poll their context. Cancelling the shutdown future leaves admission stopped; another call resumes draining. Dropping the pool does not synchronously join running threads. Caught closure panics return `CpuError::Panicked` and release admission; an aborting process cannot recover this way.

Rust threads cannot safely enforce a hard execution timeout. Uninterruptible native extraction must use a subprocess adapter with CPU/RSS/output limits, an absolute deadline, and kill followed by wait/reaping and temporary-file cleanup on timeout or cancellation. Phase 13 must implement those controls for the selected tools. An async timeout around a thread future alone cannot terminate extraction.

## Verification

- `tests/cpu_pool.rs` covers immediate saturation, expired jobs never starting, cancelled queued jobs never starting, running cancellation, runtime timer responsiveness, shutdown and resumed draining, panic recovery, capacity validation and shared worker concurrency.
- The PostgreSQL candidate suite exercises matching through the shared pool, including exact verification and duplicate races.
- No new runtime dependency was added. Tokio's existing `time` feature is enabled only for test timers.
