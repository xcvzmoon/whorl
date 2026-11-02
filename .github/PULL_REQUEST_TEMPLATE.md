<!--
Use a Conventional Commit title, such as `fix(api): handle invalid upload requests`.
Keep Summary and Verification. Remove optional sections that do not help review this change.
Do not invent evidence to fill a section.
-->

<!-- Start with one paragraph of two or three complete sentences describing the problem or capability, resulting behavior, and why the change is needed. Put it before Summary. -->

## Summary

<!-- State the problem or capability, resulting behavior, and reason for the approach. Link related issues with closing keywords where appropriate. For stacked work, name the parent PR or branch and distinguish inherited changes from this PR's own changes. -->

## Change map

<!-- Optional: use when several modules, responsibilities, or ownership boundaries change. Use module or domain names rather than a file list. Prose is enough for a single local module. -->

| Module | Before | After | Description |
| ------ | ------ | ----- | ----------- |
|        |        |       |             |

## Architecture and behavior

<!-- Optional: add code-backed module, sequence, or state diagrams when they explain the change better than prose. Identify changed behavior scenarios with stable IDs and short titles. Cover each scenario with evidence or mark it unverified. For fixes that change flow or state, include comparable Before and After diagrams and explain the changed step, owner, or transition. Reference relevant paths or symbols; mark assumptions explicitly. -->

## Boundaries and risks

<!-- Optional: include meaningful failure modes, invariants, migrations, or external effects. Map high-risk invariants to tests, runtime evidence, or an explicit Not verified gap. Reuse behavior scenario IDs when applicable. Prose is enough for a single boundary. -->

| Invariant or boundary | Failure mode | Protection | Evidence or gap |
| --------------------- | ------------ | ---------- | --------------- |
|                       |              |            |                 |

## Verification

<!-- List exact commands and results. Explain which behavior or invariant each focused test or runtime check demonstrates. Distinguish static checks, tests, CI, and manual acceptance. List unverified conditions explicitly; a passing check does not prove unrelated behavior. -->

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --locked --all-targets --all-features -- -D warnings`
- [ ] `cargo test --locked --all-targets --all-features`
- [ ] `cargo build --locked --release`
- [ ] Added or updated tests where behavior changed.
- [ ] Updated documentation where usage changed.

## Visual changes

<!-- Required for user-visible UI changes; otherwise remove. Capture every affected state at the merge base and proposed HEAD with matching viewport, theme, locale, fixtures, and readiness conditions. Upload captures as GitHub user assets; keep PR-only images out of tracked files. Put image pairs before their page or state labels. Use Absent for new states and Removed for deleted states. Capture or upload failures block completion of UI evidence. -->

| Before                                           | After                   |
| ------------------------------------------------ | ----------------------- |
| <!-- Before image -->                            | <!-- After image -->    |
| <!-- Page, component, state, viewport, theme --> | <!-- Matching label --> |

## Rollout and follow-up

<!-- Optional: describe breaking changes, migrations, deployment order, rollback, feature gates, monitoring, or follow-up work. State whether any gap blocks merge or belongs to a separate task. -->
