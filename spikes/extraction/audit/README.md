# Candidate audit

Queried crates.io and GitHub on November 3, 2026. JSON snapshots in this directory retain URLs, release metadata, recent commits, and a small issue sample. Downloads below are crates.io recent downloads (rolling 90 days), not unique users. Pins apply only to the spike; the service has no new dependencies.

| Crate | Exact spike pin | Latest stable | Release date of pin | Recent downloads | Declared MSRV | License |
| --- | --- | --- | --- | ---: | --- | --- |
| [lopdf](https://crates.io/api/v1/crates/lopdf) | 0.46.0 | 0.46.0 | 2026-10-21 | 13,611,466 | 1.88 | MIT |
| [pdfium-render](https://crates.io/api/v1/crates/pdfium-render) | 0.9.5 | 0.9.5 | 2026-11-01 | 1,794,787 | 1.61 | MIT OR Apache-2.0 |
| [zip](https://crates.io/api/v1/crates/zip) | 9.2.2 | 9.2.2 | 2026-11-02 | 96,914,313 | 1.88 | MIT |
| [quick-xml](https://crates.io/api/v1/crates/quick-xml) | 0.42.0 | 0.42.0 | 2026-08-22 | 159,461,676 | 1.86 | MIT |
| [calamine](https://crates.io/api/v1/crates/calamine) | 0.38.0 | 0.38.0 | 2026-11-02 | 6,449,294 | 1.88 | MIT |
| [image](https://crates.io/api/v1/crates/image) | 0.25.10 | 0.25.10 | 2026-03-10 | 73,390,566 | 1.88.0 | MIT OR Apache-2.0 |
| [kamadak-exif](https://crates.io/api/v1/crates/kamadak-exif) | 0.6.1 | 0.6.1 | 2024-11-06 | 4,373,382 | 1.60 | BSD-2-Clause |
| [ocrs](https://crates.io/api/v1/crates/ocrs) | 0.13.1 | 0.13.1 | 2026-09-13 | 317,149 | Not declared | MIT OR Apache-2.0 |
| [rten](https://crates.io/api/v1/crates/rten) | 0.26.0 | 0.27.0 | 2026-08-29 | 1,011,824 | 1.94.0 | MIT OR Apache-2.0 |
| [serde](https://crates.io/api/v1/crates/serde) | 1.0.229 | 1.0.229 | 2026-07-18 | 400,731,754 | 1.56 | MIT OR Apache-2.0 |
| [serde_json](https://crates.io/api/v1/crates/serde_json) | 1.0.154 | 1.0.154 | 2026-10-11 | 401,667,438 | 1.71 | MIT OR Apache-2.0 |

## Activity and issue samples

| Repository | Last push | Open issues including PRs | Non-PR issues in sample | Closed in sample | Comments in sample |
| --- | --- | ---: | ---: | ---: | ---: |
| [J-F-Liu/lopdf](https://github.com/J-F-Liu/lopdf) | 2026-11-02 | 88 | 1 | 0 | 1 |
| [ajrcarey/pdfium-render](https://github.com/ajrcarey/pdfium-render) | 2026-11-01 | 12 | 5 | 4 | 7 |
| [bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries) | 2026-10-26 | 33 | 5 | 2 | 8 |
| [image-rs/image](https://github.com/image-rs/image) | 2026-09-15 | 244 | 3 | 1 | 1 |
| [kamadak/exif-rs](https://github.com/kamadak/exif-rs) | 2025-10-21 | 32 | 3 | 1 | 2 |
| [robertknight/ocrs](https://github.com/robertknight/ocrs) | 2026-11-01 | 25 | 2 | 1 | 1 |
| [robertknight/rten](https://github.com/robertknight/rten) | 2026-11-02 | 49 | 3 | 1 | 3 |
| [serde-rs/json](https://github.com/serde-rs/json) | 2026-10-16 | 252 | 1 | 0 | 1 |
| [serde-rs/serde](https://github.com/serde-rs/serde) | 2026-10-16 | 412 | 2 | 0 | 5 |
| [tafia/calamine](https://github.com/tafia/calamine) | 2026-11-02 | 42 | 2 | 1 | 5 |
| [tafia/quick-xml](https://github.com/tafia/quick-xml) | 2026-11-02 | 91 | 0 | 0 | 0 |
| [tesseract-ocr/tesseract](https://github.com/tesseract-ocr/tesseract) | 2026-11-02 | 488 | 3 | 2 | 2 |
| [zip-rs/zip2](https://github.com/zip-rs/zip2) | 2026-11-02 | 44 | 1 | 0 | 0 |

The issue sample is the ten most recently created issues/PRs returned by GitHub, filtered here to issues. Closures and comments show some activity; they do not establish response-time guarantees. No non-PR quick-xml issue appeared in that sample, so responsiveness is unverified. Later GitHub API requests hit the unauthenticated rate limit. Review issue links before production adoption.

## Decisions for the experiment

- Every pre-1.0 direct dependency is pinned with `=` and the standalone lockfile is checked in. `rten` 0.26.0 matches `ocrs` 0.13.1; 0.27.0 is newer but has an incompatible model type. The resolved dependency inventory includes crate checksums in Cargo.lock and registry source revisions in dependencies.json.
- All candidate crates except kamadak-exif have a release within twelve months. kamadak-exif 0.6.1 was released November 6, 2024, and its last push was October 2025. It is included to measure the requested candidate, with no production selection implied. Its maintenance exception needs a decision before adoption.
- ocrs has no declared MSRV; the combined graph requires at least Rust 1.94 through rten. This spike builds with Rust 1.98.1, rather than claiming the wrapper crate MSRV is the graph MSRV.
- PDFium is the checksum-pinned Linux ARM64 binary from [chromium/8125](https://github.com/bblanchon/pdfium-binaries/releases/tag/chromium%2F8125), version 158.0.8125.0. Its bundled LICENSE includes third-party notices. pdfium-render uses dynamic loading; native binary distribution and updates remain an operational responsibility.
- [Tesseract 5.5.4](https://github.com/tesseract-ocr/tesseract/releases/tag/5.5.4) is built from the tagged source (Apache-2.0). English tessdata_fast is pinned by commit and checksum; its Apache-2.0 license is retained here.
- [Ocrs model licensing](https://github.com/robertknight/ocrs-models/blob/main/LICENSE.md) differs from its Rust code. The pretrained models inherit CC BY-SA 4.0 from HierText. The license snapshot and attribution must accompany redistribution. This is a selection consideration, not a claim about the license of extracted user text. Models are downloaded into ignored assets/, not vendored in this repository.
- Image fixtures contain no asserted EXIF values. This corpus tests dimensions and absent EXIF handling, not correctness of populated EXIF tags.
- RustSec audit output is retained in rustsec.json. License identifiers for the resolved graph are in dependencies.json; this is an inventory, not a legal opinion. Native binary and OS-package vulnerabilities are outside cargo-audit.
