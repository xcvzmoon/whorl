# Engineering principles

Whorl targets two CPU cores and 4 GiB RAM. Resource ownership is part of correctness: bound queued work, retained bytes, extraction expansion, and processing time. Keep extraction, OCR, normalization, hashing, and verification off async executor threads. Use the shared CPU pool for the worker pipeline and subprocess isolation for native extraction. Maintenance reindexing processes one blocking computation at a time.

PostgreSQL owns application state and durable queue delivery. Shared S3-compatible storage owns source objects; Whorl does not depend on the sibling upload service. A failed cleanup retains its durable charge until the object can be reclaimed. See [architecture](architecture.md) and [storage](storage.md).

Make the smallest correct change and leave unrelated code alone. Keep each step independently verifiable and each commit focused on one purpose. Work in the main worktree unless explicitly asked otherwise. Commit and push only when asked. The repository's [AGENTS.md](../AGENTS.md) is the source for editing and contribution instructions.

The initial API and deviations from the specification were approved before implementation. Preserve those decisions when changing behavior; record an explicit replacement decision when requirements change. The [decision record](decisions.md) preserves the approvals and their rationale.

These principles consolidate the former project notes about bounded resources and small reviewable changes. The [project history](history.md) explains how I applied these constraints and links the recorded evidence.
