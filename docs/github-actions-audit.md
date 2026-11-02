# GitHub Actions audit

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
