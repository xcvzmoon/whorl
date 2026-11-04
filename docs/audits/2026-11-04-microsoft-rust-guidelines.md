# Microsoft Rust Guidelines audit

Reviewed November 4, 2026 against Microsoft's [Pragmatic Rust Guidelines, version 2026.6](https://microsoft.github.io/rust-guidelines/), using its [checklist](https://microsoft.github.io/rust-guidelines/guidelines/checklist/index.html). Scope is the current working tree above `31e7a62`, including the uncommitted architecture and file-organization changes. Findings refer to that snapshot, not just the last commit.

This is an application audit of the main package: production Rust, public interfaces used by its executables, tests, configuration, and automation. The packages under `spikes/` were considered as research context, not audited implementation. No production code, tests, dependencies, or workflow configuration changed during this audit.

Implementation follow-up: the eight numbered findings have been addressed. See the [resolution record](2026-11-04-microsoft-rust-guidelines-resolutions.md) for current behavior, measurements, and verification. The findings and line references below describe the pre-fix snapshot.

## Assessment

The strongest finding is the CPU pool's explicit continuation after a panic. Telemetry backpressure is the next runtime concern. The remaining findings cover verification policy, public debugging/documentation, shared lock constants, and two performance opportunities. No undefined behavior or invariant bypass was established by inspection. That is not a proof of soundness or a production acceptance result.

| Finding | Priority | Evidence type |
| --- | --- | --- |
| 1. Continuing after CPU work panics | High | Source and existing regression behavior |
| 2. Synchronous hot-path stderr telemetry | Medium | Source; operational impact unmeasured |
| 3. Guideline lint coverage is not configured | Medium | Manifest and a broader lint probe |
| 4. Public debugging/formatting traits are incomplete | Low | Seven compiler diagnostics and interface inspection |
| 5. Module documentation omits important interaction rules | Low | Rustdoc source inspection |
| 6. Advisory-lock protocol values are duplicated inline | Low | Source; maintenance risk rather than a demonstrated race |
| 7. Query shingles are rebuilt for every candidate | Measure next | Repeated allocation path identified; no throughput measurement |
| 8. Match persistence issues one insert per result | Measure next | Query count follows result count; no throughput measurement |

## 1. CPU panics are converted into ordinary retryable failures

Rules: [M-PANIC-CONTINUATION](https://microsoft.github.io/rust-guidelines/guidelines/correctness/index.html#panic-continuation-is-last-resort-m-panic-continuation), M-PANIC-IS-STOP.

[CpuPool::run](../../src/cpu_pool.rs#L237) wraps arbitrary submitted work in `catch_unwind(AssertUnwindSafe(...))`, returns `CpuError::Panicked`, and releases the admission slot. The [existing test](../../tests/cpu_pool.rs#L133) then successfully runs more work on the same pool. The [worker error policy](../../src/service/process_file.rs#L242) treats this CPU error as an infrastructure retry rather than requesting termination.

A closure can mutate shared or thread-local logical state before panicking. `AssertUnwindSafe` does not restore that state. Catching the panic lets the process keep using it indefinitely, without a restart signal. This is a confirmed policy deviation; the audit did not demonstrate corrupted application state.

| Option | Effort and risk | Effect |
| --- | --- | --- |
| A, recommended: poison admission and request controlled worker exit after unrelated work drains | Moderate; shutdown/recovery behavior needs integration coverage | Retains cleanup opportunities while preventing continued use of potentially inconsistent state |
| B: propagate the panic into an explicit fail-fast process policy | Smaller implementation; interrupts other work | Relies on process supervision and existing durable recovery |

Verify that a work panic prevents later admission and causes executable shutdown. Preserve normal extraction rejection as a recoverable file outcome. Exercise process restart and durable retry separately from panic handling.

## 2. Hot-path telemetry can block the caller on stderr

Rules: [M-LOG-OVERHEAD](https://microsoft.github.io/rust-guidelines/guidelines/performance/index.html#library-telemetry-does-not-tank-performance-m-log-overhead), [M-LOG-STRUCTURED](https://microsoft.github.io/rust-guidelines/guidelines/universal/index.html#use-structured-logging-with-message-templates-m-log-structured).

[CPU admission](../../src/cpu_pool.rs#L203) allocates JSON and writes synchronously through `eprintln!` before scheduling work. [StageMetrics::drop](../../src/telemetry.rs#L31) does the same on stage exit. A slow stderr consumer can therefore stall a runtime thread or delay cleanup. There is no configurable sink, level filter, or bounded writer policy here. The workload impact has not been measured.

The JSON fields already provide structured events, and Microsoft's guidance permits custom telemetry. The absence of `tracing` is not itself a defect. Likewise, maintenance output and the extraction child's JSON stdout are intentional interfaces, not logging violations.

| Option | Effort and risk | Effect |
| --- | --- | --- |
| A, recommended: use a configurable structured sink with a bounded nonblocking writer and explicit saturation/shutdown policy | Moderate; requires delivery/drop semantics | Keeps diagnostic backpressure off processing paths and enables filtering |
| B: retain the current transport and benchmark it with fast, slow, and blocked consumers | Small code impact; runtime risk remains until results justify it | Establishes whether an explicit exception is acceptable |

Add blocked-sink and sink-failure tests, including unwinding/drop paths. Measure enabled telemetry overhead under representative admission and candidate workloads before claiming a throughput improvement.

## 3. Passing current Clippy does not establish the recommended lint coverage

Rule: [M-STATIC-VERIFICATION](https://microsoft.github.io/rust-guidelines/guidelines/universal/index.html#use-static-verification-m-static-verification).

[Cargo.toml](../../Cargo.toml) has no project lint configuration. The current strict Clippy command denies warnings from its enabled defaults, but does not enable Microsoft's additional compiler, pedantic, cargo, and selected restriction checks. A library-only probe produced 90 warnings, including seven missing `Debug` implementations that ordinary Clippy did not report.

This is a prevention gap, not 90 demonstrated bugs. Some diagnostics concern intentional fixed-width hashing, bounded casts, or dependency duplication. The `missing_panics_doc` warning points at an unwrap of a constant known to be nonzero, not a proven caller-triggered panic.

| Option | Effort and risk | Effect |
| --- | --- | --- |
| A, recommended: explicitly configure the applicable lint set and triage findings with narrow, reasoned exceptions | Moderate initial cleanup; little runtime risk | Makes the policy repeatable locally and in the existing static CI gate |
| B: maintain a separate local guideline-probe command | Low effort; findings can drift without enforcement | Gives visibility without immediately adopting every recommendation |

Do not reintroduce tests into CI: the user explicitly moved execution local. Keep formatting, static analysis, documentation, audits, and builds automated. Prefer `#[expect]` for applicable handwritten overrides under M-LINT-OVERRIDE-EXPECT, but evaluate the existing role-dependent `dead_code` suppressions before changing them mechanically.

## 4. Seven public types lack Debug; SignatureVersion lacks Display

Rules: [M-PUBLIC-DEBUG](https://microsoft.github.io/rust-guidelines/guidelines/universal/index.html#public-types-are-debug-m-public-debug), M-PUBLIC-DISPLAY.

The compiler probe identified:

| Type | Location |
| --- | --- |
| `WorkContext`, `CpuPool` | [cpu_pool.rs](../../src/cpu_pool.rs#L78) |
| `PreparedDocument`, `Matcher` | [matching.rs](../../src/service/matching.rs#L48) |
| `Extractor` | [supervisor.rs](../../src/extraction/supervisor.rs#L11) |
| `UploadStore` | [storage/mod.rs](../../src/storage/mod.rs#L55) |
| `JobQueue` | [graphile.rs](../../src/queue/graphile.rs#L11) |

[SignatureVersion](../../src/similarity/signature.rs#L23) is a readable string-like value with `as_str()` but no `Display` implementation. These interfaces cross the library/executable seam even though the package is unpublished.

| Option | Effort and risk | Effect |
| --- | --- | --- |
| A, recommended: add safe Debug implementations and canonical Display for the version | Low effort; redaction needs care | Improves diagnostics and direct formatting |
| B: reduce visibility for types not intended to cross crate interfaces | Potentially larger caller changes | Shrinks the public obligation, but does not solve needed exported handles |

Do not blindly derive `Debug` for document or storage state. Use redacted implementations and tests proving marker text, metadata, and credentials are absent. Add compile-time trait checks and assert that formatted versions equal their persistence key.

## 5. Module rustdoc is incomplete despite a warning-free build

Rule: [M-MODULE-DOCS](https://microsoft.github.io/rust-guidelines/guidelines/docs/index.html#has-comprehensive-module-documentation-m-module-docs).

[storage::s3](../../src/storage/s3.rs#L1) has no module overview. [service](../../src/service/mod.rs#L1) describes only matching while exporting admission, durable processing, and lifecycle operations. The [crate root](../../src/lib.rs#L1) and [similarity module](../../src/similarity/mod.rs#L1) give summaries without a minimal interaction example or ownership flow.

Repository guides contain much of this information, but generated Rust documentation does not lead callers through preparation, admission, processing, transaction ownership, and cleanup. Rustdoc does not warn about all omissions.

| Option | Effort and risk | Effect |
| --- | --- | --- |
| A, recommended: add concise module overviews and compiling interaction examples, linking detailed guides | Low-to-moderate effort; no runtime changes | Makes public interfaces navigable without duplicating operational manuals |
| B: retain external guides as the only architectural explanation | No immediate effort | Leaves a documented exception to the guideline and weaker generated navigation |

Verify with doctests and `RUSTDOCFLAGS='-D warnings' cargo doc`. Follow repository-required `# Arguments` sections: Microsoft's different parameter-prose preference does not override `AGENTS.md`. Missing `#[doc(inline)]` is not a material finding here because private-source re-exports already inline automatically.

## 6. Shared lock protocol numbers are scattered in SQL strings

Rule: [M-DOCUMENTED-MAGIC](https://microsoft.github.io/rust-guidelines/guidelines/universal/index.html#magic-values-are-documented-m-documented-magic).

The capacity lock `746831904` occurs in [admission](../../src/service/admission.rs#L138) and [maintenance](../../src/service/maintenance.rs#L20). The file-attempt hash seed `15` occurs in [processing](../../src/service/process_file.rs#L200) and [deadline recovery](../../src/service/lifecycle.rs#L152). Upload hash seed zero is shared by admission, discard, upload, and reconciliation.

The current values agree. The risk is changing one participant without its peers, silently defeating mutual exclusion. The existing storage guide explains parts of the protocol, but the values are not one named, locally documented implementation contract.

| Option | Effort and risk | Effect |
| --- | --- | --- |
| A, recommended: centralize named lock constants/helpers with rustdoc explaining participants and change constraints | Small focused change; preserve exact SQL values | Prevents accidental divergence without changing the lock protocol |
| B: document each repeated value and its peer call sites | Smaller change; duplicate definitions remain | Makes coordination explicit but still requires manual updates |

Preserve admission races, attachment/reconciliation exclusion, and worker/recovery exclusion tests. Do not change deployed lock identities as incidental cleanup; mixed-process coordination depends on them.

## 7. Exact verification rebuilds the query shingle set for every candidate

Rules: [M-MEM-REUSE](https://microsoft.github.io/rust-guidelines/guidelines/performance/index.html#reuse-allocations-where-possible-m-mem-reuse), M-HOTPATH.

[SimilarityEngine::verify](../../src/similarity/verify.rs#L278) calls `query.shingles()` on every long-text comparison. [ShingleSet::shingles](../../src/similarity/shingles.rs#L121) allocates a new `BTreeSet`, although one prepared query is reused across candidate pages. Construction is repeated work; its contribution to runtime has not been profiled.

| Option | Effort and risk | Effect |
| --- | --- | --- |
| A, recommended: benchmark then retain a reusable prepared query set or equivalent representation | Moderate lifetime/memory design work | Removes repeated preparation when many candidates share one query |
| B: keep recomputation if profiling shows its cost is negligible | Low implementation effort | Avoids additional retained memory and complexity |

Measure allocations and verification at 32 and 1,000 candidates using realistic text lengths. Preserve exact scores, short-text equality, source/version isolation, and deterministic vectors. Avoid speculative boxed storage or custom hashers without evidence.

## 8. Match persistence scales database statements with every result

Rule: [M-THROUGHPUT](https://microsoft.github.io/rust-guidelines/guidelines/performance/index.html#optimize-for-throughput-avoid-empty-cycles-m-throughput).

The [final persistence transaction](../../src/service/process_file.rs#L393) awaits one insert per qualifying match. There is intentionally no silent top-N truncation. Larger qualifying sets therefore increase statement count and time holding the transaction/file lock. This is a structural scaling concern; the audit did not measure customer-scale latency.

| Option | Effort and risk | Effect |
| --- | --- | --- |
| A, recommended: measure and use bounded multirow inserts or UNNEST in the existing transaction | Moderate SQL change; rollback/fencing require verification | Reduces round trips while preserving atomicity |
| B: retain row-at-a-time inserts for a measured small-result workload | No initial implementation work | Keeps simple SQL but needs a stated operating envelope |

Verify all rows, score precision, client/file scoping, rollback after a failed insert, stale-attempt rejection, and immutable repeated delivery. Compare query counts and latency before choosing a batch size.

## Applicable recommendations and explicit limitations

- **Safety:** the only production `unsafe` located is the process-group `libc::kill` call with a local `SAFETY` explanation. Its platform purpose fits [M-UNSAFE](https://microsoft.github.io/rust-guidelines/guidelines/correctness/index.html#unsafe-needs-reason-should-be-avoided-m-unsafe). No unsafe trait implementations, pointer casts, or custom allocators were found. Miri was not run; dependency/native-library soundness was not proven.
- **Async/resource ownership:** CPU work uses a dedicated bounded pool; native extraction has output limits, timeout, process-group cleanup, child waiting, and temporary-directory ownership. Admission remains held until cancelled CPU work exits. Polling uses bounded cursor pages. Individual Rust computations are not forcibly terminable; existing cooperative-cancellation limits remain relevant.
- **Strong types:** signature parsing, scoring settings, and retention use checked construction/private invariants. Public submitted-file records are revalidated before durable side effects; mutability alone is not evidence of an invariant bypass.
- **Errors:** public error enums and foreign error types differ from the strict library pattern in [M-ERRORS-CANONICAL-STRUCTS](https://microsoft.github.io/rust-guidelines/guidelines/libs/ux/index.html#errors-are-canonical-structs-m-errors-canonical-structs). This application is unpublished and relies on variants for retry policy. [M-APP-ERROR](https://microsoft.github.io/rust-guidelines/guidelines/apps/index.html#applications-may-use-anyhow-or-derivatives-m-app-error) permits simpler application error handling. A contextual error redesign should have a concrete diagnostic objective, not merely replace every enum.
- **Compiler and crate layout:** edition 2024 is in use. `rust-version` is undeclared; decide and test compiler support before declaring it under [M-MSRV](https://microsoft.github.io/rust-guidelines/guidelines/project/index.html#msrv-is-conservatively-updated-m-msrv). Separate research packages and the application do not share a workspace; this departs from the literal workspace preference, but no current dependency duplication defect or need for a crate split was established.
- **Allocator/CPU settings:** the application uses the default allocator and generic target settings, rather than the [mimalloc and target-CPU recommendations](https://microsoft.github.io/rust-guidelines/guidelines/apps/index.html). Record this gap and benchmark allocator/RSS effects. Choose CPU flags only after defining the deployment CPU floor. No performance gains were assumed.
- **Other library rules:** exported procedural macros, custom FFI bindings, feature combinations, and test bypass features were not found in the main package. No feature-matrix run was needed. cargo-hack, cargo-udeps, native corpus acceptance, and external PostgreSQL/S3 suites were not run for this audit.

## Verification performed locally

Toolchain: rustc 1.98.1 and Cargo 1.98.1, macOS ARM64. No tests were added back to CI.

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | Passed with current lint policy |
| `cargo test --locked --lib --tests --all-features` | 55 passed; 14 external checks ignored |
| `cargo test --locked --doc --all-features` | 56 passed |
| `RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps --all-features` | Passed |
| `cargo deny check advisories licenses sources` | Passed; existing nom-exif license-field and unused BSD allowance warnings |
| Broader library lint probe below | 90 warnings, triaged; no policy files changed |

Probe command:

```sh
cargo clippy --locked --lib --message-format=json -- \
  -W missing_debug_implementations \
  -W clippy::pedantic \
  -W clippy::cargo \
  -W clippy::undocumented_unsafe_blocks \
  -W clippy::allow_attributes_without_reason
```

The largest probe categories were must-use suggestions (25), multiple dependency versions (18), documentation formatting (9), possibly truncating casts (8), and missing Debug implementations (7). Hash truncation is part of the persisted signature format; it must not be mechanically replaced. All other diagnostic categories require the same context-sensitive triage. Passing checks establish the stated checks only; external runtime behavior and a complete guideline-by-guideline conformance proof remain outside this audit.

Recommended sequence: resolve panic policy, bound telemetry backpressure, configure the lint baseline, then address public diagnostics/documentation and lock constants. Profile the two candidate-heavy paths before changing their data structures or SQL.
