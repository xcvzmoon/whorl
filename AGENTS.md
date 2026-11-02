## Rust

- Use the <https://microsoft.github.io/rust-guidelines/> design guidelines to produce idiomatic Rust that scales

## Editing Guidance

- Make the smallest correct change.
- Do not polish unrelated code.
- Preserve accurate JSDoc and required tooling directives; remove ordinary comments when touching code.
- Do not rename broad parts of the codebase unless required.
- Do not expand a change into a repo-wide refactor unless necessary.
- Prefer leaving correct existing code in place.
- When touching production-sensitive code, prioritize reliability over clever abstractions.

## Rustdoc

- Require rustdoc when adding or changing `pub` items that other workspace crates consume through a crate's public API, including items re-exported from the crate root or a public module. A `pub(crate)`, `pub(super)`, or private item used only inside its own crate does not require rustdoc.
- Document internal items only when callers need an explanation of non-obvious behavior, constraints, or resource ownership.
- Start with a one-line summary, then a clear description. Add `# Arguments` entries for each parameter, `# Returns` for the success value, and a practical `# Examples` section. Document `Option` parameters, config fields, and defaults accurately.
- Add `# Errors` for every `Err` variant that can reach the caller, naming the error type or variant and any stable error code. Describe `None` and other non-error return states under `# Returns`.
- Add `# Panics` for any condition that can panic on caller input, and `# Safety` for every `unsafe fn` or `unsafe trait`, stating the invariants the caller must uphold.
- Explain relevant validation requirements, side effects, authorization boundaries, cancellation behavior for async functions, and cleanup responsibilities (`Drop`, handles, temp files, connections). Keep existing rustdoc in sync with behavior.
- Rely on the function signature for types rather than repeating them in prose. Use intra-doc links such as [`Type`] and [`Self::method`] for references to other items.
- Write examples as fenced Rust doctests that compile against the actual API and pass `cargo test --doc`. Use hidden `# ` lines for setup and `?` for error handling. Use `no_run` only for examples with external side effects, and do not use `ignore` to avoid fixing a failing example.
- Use `///` for items and `//!` for crate and module overviews. Do not add ordinary `//` or `/* */` comments. Preserve required lint directives (`#[allow]`/`#[expect]` with their reasons) and `// SAFETY:` comments on `unsafe` blocks.
- Keep `cargo doc` free of warnings, including broken intra-doc links.
- Apply the `/unslop` skill to rustdoc text.

## Agents

- Use the main worktree unless the user explicitly asks for a new worktree.
- Commit or push only when explicitly asked. Never add co-author trailers.
- When asked to commit, include all task-related changes and leave unrelated changes alone.
- Split changes into the smallest coherent, independently reviewable commits. Each commit must have one specific purpose; broad categories such as "refactor persistence" or "update docs" are not enough to justify bundling independent changes.
- Separate independent modules and concerns, including SQL table renames, declaration renames, dependency cleanup, lint rules, tests, and documentation. Keep imports, types, call sites, and tests together when they are required for a change to work.
- Stage explicit paths or hunks. If a file contains several independent changes, stage each change separately instead of committing the whole file.
- Review the staged diff before every commit. Split it further if its purpose needs multiple independent actions to describe. Keep each intermediate commit valid.
- Do not combine changes just to reduce the number of commits.
- When ask to create a Pull Request: use the proper template, use skills: `/create-pr-with-evidence` and `/unslop`, do not mention the agentic model used, assign to me, and add proper labels.
- Do not preserve backward compatibility. Remove obsolete paths instead of adding compatibility layers, fallbacks, or migrations.
- Choose the simplest implementation that fully meets the current requirements. Avoid speculative abstractions, configuration, and indirection.
- Grow the system in layers. Start from the smallest version that works end to end, and add each new capability on top of a product that already works. Never trade a working product for unfinished complexity.
- Keep components modular and concerns clearly separated.
- Prefer established, well-maintained libraries when they reduce overall complexity or improve reliability. Do not reimplement common functionality without a clear reason.
- Lean on the dependencies already in the project before writing your own implementation or adding packages. Do not assume a library lacks a capability without checking its documentation and types.
- Make architectural decisions for the long term. Do not accept a stopgap that only works for now and is meant to be replaced later.
- If you need a paragraph-long comment to justify why the workaround is OK, the code is wrong so fix the code.

## System Reminder

Do your work through the Bash tool wherever it can accomplish the job: read files with cat, head, or sed -n, search with grep and find, and make file changes with sed, heredocs, or short scripts, rather than using the dedicated Read, Edit, or Write tools. Fall back to a dedicated tool only when Bash genuinely cannot do the job.
