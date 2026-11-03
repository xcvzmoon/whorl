# GitHub Actions audit

## Follow-up after phase 14

Measured November 3, 2026 over 14 days: 42 workflow runs, 81 executed jobs, 165 estimated Linux runner minutes (354/month if this short burst continued). These are rounded duration estimates, not an invoice. Rounding contributed 60 minutes (36.3%); failed jobs consumed 25 minutes; cancelled jobs consumed none.

| Workflow / event | Estimated minutes | Share |
| --- | ---: | ---: |
| CI / pull request | 103 | 62.4% |
| CI / push | 57 | 34.5% |
| PR description | 5 | 3.0% |

| Job / event | Estimated minutes | Share |
| --- | ---: | ---: |
| Checks / pull request | 81 | 49.1% |
| Checks / push | 41 | 24.8% |
| CI summary / pull request | 22 | 13.3% |
| CI summary / push | 16 | 9.7% |
| PR description | 5 | 3.0% |

Successful PR CI runs had a 1.1-minute median and 11.3-minute p90; successful push runs had a 1.2-minute median and 4.2-minute p90. The median includes early phases with few dependencies and understates current latency. Branches started a median of one run and a maximum of three per workflow. No release workflow ran in this sample.

### Why the latest run took eleven minutes

[PR 16 CI run](https://github.com/xcvzmoon/whorl/actions/runs/41642987099) took 11m16s overall. Its Checks job took 11m05s:

| Step | Seconds |
| --- | ---: |
| Project setup | 8 |
| Formatting and Clippy | 109 |
| Tests including doctests | 170 |
| Documentation | 15 |
| Release build | 313 |
| Cache cleanup and upload | 20 |

The release build alone used 47% of Checks time. Test compilation took 152 seconds; the all-target tests took 170 seconds overall. This is compilation cost, not evidence that individual tests need removal. The earlier prebuilt auditor correction still works.

The log reports `No cache found`. The cache API returned only two entries, both under PR merge refs (`refs/pull/15/merge` and `refs/pull/16/merge`), totalling about 1.78 GB. There was no default-branch cache. GitHub scopes PR caches to their merge ref, so later PRs cannot restore them. The preceding main run failed in tests, before the cache action's default success-only save. A main run was still in progress during measurement; its eventual result is outside this snapshot.

### Selected correction and limits

The user chose to preserve every gate. The shared setup action now enables `cache-on-failure`, allowing built dependencies to survive a failed main run and become available to subsequent PRs. Existing shared keys, toolchain and dependency hashing remain intact. Partial failed-run caches may omit later release artifacts; immutable existing keys are not refreshed, so this does not guarantee a fully warm release build. No arbitrary per-run keys were added: each full cache is about 1 GB, and duplicating them would increase eviction and upload costs.

| Option | Estimated runner saving | Estimated wall-clock saving | Decision / risk |
| --- | --- | --- | --- |
| Save dependencies after failed runs | Unmeasured; conservative budget estimate is zero until hosted verification | Unmeasured; reduces repeated compilation when a usable main cache exists | Selected; preserves all gates, may save partial artifacts |
| Remove PR release build | Up to 313 seconds on this cold run; rounded savings depend on total duration | Up to 5m13s on this run | Declined; PR release-profile validation would be lost |
| Remove main validation | Historical ceiling 57 rounded minutes in this sample | Does not shorten PR critical path | Deferred; merge enforcement unavailable |
| Remove summary job | 38 rounded minutes in this sample | Only a few execution seconds plus scheduling | Deferred; preserves status names |

Rulesets and main protection APIs again returned HTTP 403 with a plan-upgrade message. Required checks and strictness remain unverified. The account is a personal owner, so no organisation plan was queried. No checks, triggers, timeouts, runner types, or status names were removed. Phase 15 also adds a PostgreSQL-backed Linux worker recovery gate; its incremental hosted duration is unmeasured and is not included in the historical totals. No workflow-layout tests were found. No workflow was pushed or dispatched; hosted savings remain unverified.

Sources: [GitHub cache scope and immutability](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching), [rust-cache v2.9.3 options](https://github.com/Swatinem/rust-cache/tree/v2.9.3). Reproduce with the measurement command below, using `/tmp/whorl-ci-jobs.json` for raw rows.

## Initial audit (historical)

Measured on 2026-11-03, covering the preceding 14 days. Three workflow runs contained five executed jobs. Estimates below use hosted Linux job duration rounded up per job, not an invoice or remaining account allowance.

| Area | Estimated minutes | Share |
| --- | ---: | ---: |
| CI / pull_request | 10 | 90.9% |
| PR description / pull_request | 1 | 9.1% |
| Total | 11 | 100% |

- Job rounding added about 3 minutes (29.2%). Failed jobs consumed 4 minutes; cancelled jobs consumed none.
- A linear monthly projection is about 24 minutes, based on only three runs; this is not a workload forecast.
- One successful full CI run took 5.1 minutes wall-clock; its median and p90 are the same because the sample contains one run.
- The successful description-only run took about 0.1 minutes.
- Checks jobs consumed 8 rounded minutes, summary CI jobs 2, and the separate description job 1.

## Cause and correction

Run 41515307198 spent 171 seconds in setup. Its logs show cargo-deny 0.20.2 compilation took 2m49s; static checks, tests, rustdoc, and dependency auditing each took about one second, and the starter release build rounded to zero seconds. The job lasted 291 seconds; step execution does not explain all runner lifecycle time.

The shared setup action now installs cargo-deny 0.20.2 using taiki-e/install-action v2.89.1. Checksums remain enabled by default; fallback is explicitly disabled so an unavailable binary fails instead of silently compiling from source. Both CI and release checks use this action.

| Change | Estimated saving per full CI run | Risk |
| --- | --- | --- |
| Download prebuilt auditor | About 2–3 minutes of setup; conservatively 2 rounded job minutes if other overhead stays similar | Requires upstream release availability; tool upgrades are explicit |

This is an estimate, not a measured improved-run result. Actual timing will be recorded after CI completes.

## Preserved checks and deferred changes

- Keep Checks, CI, and PR description status names, including the summary job's one-minute rounding cost. Removing it is an optional separate decision.
- Ruleset and main protection APIs returned 403 with a plan-upgrade message for this private repository. Required statuses and strictness could not be verified.
- Keep main validation: direct pushes and untested merge combinations remain possible without verified enforcement.
- Keep all formatting, lint, test, documentation, dependency/license/source, and release build gates.
- Do not add Cargo caches yet: measured project compilation is negligible, and caching adds restore/save work. Revisit when dependencies are introduced.
- Do not add draft or path skipping without measured savings and an agreed merge-gate policy. No workflow-layout tests currently exist.
- No self-hosted runners, merge queue, runner changes, or billing-plan changes.

## Reproduction

```sh
node .agents/skills/optimise-github-actions/scripts/measure.mjs xcvzmoon/whorl --days 14 --out /tmp/whorl-actions-jobs.json
gh api repos/xcvzmoon/whorl/actions/runs/41515307198/jobs --paginate
```

The measurement script belongs to the local skill installation and is not tracked in this PR. GitHub's API supplies the source job/step timestamps. Local validation includes actionlint 1.7.12 and cargo-deny 0.20.2; hosted execution remains the final test of the composite action.
