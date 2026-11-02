# Bounded resources

- Target two CPU cores and 4 GiB RAM.
- Bound queued work, stored bytes, extraction expansion, and processing time.
- Keep extraction, OCR, normalization, hashing, and verification off async executor threads.
- Use PostgreSQL for application persistence and a library-backed durable queue.
- Use shared S3-compatible storage directly; do not depend on the sibling upload service.
- Source: approved task constraints and deployment clarification.
