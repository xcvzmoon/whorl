# Extraction spike

Phase 3 benchmarks extraction candidates on the phase 2 corpus. The experiment is complete. The user approved its tool and resource recommendations on November 3, 2026, retaining single-character OCR lines and requesting a maintained EXIF alternative. See the [approval record](decisions.md#phase-13-extraction-approval) and [production adapter documentation](extraction.md). The measurements and proposal language below preserve the benchmark-stage findings.

## Benchmark recommendations

| Format | Recommendation | Reason and boundary |
| --- | --- | --- |
| Digital PDF | pdfium-render 0.9.5 with PDFium 158.0.8125.0 | Both PDF candidates recover all asserted W-4 anchors and properties. PDFium also supplies the rasterizer needed for scanned PDFs, avoiding a second PDF stack. This single form does not establish general PDF reading-order quality. |
| Scanned PDF | PDFium rendering at 300 DPI, then Tesseract 5.5.4, English tessdata_fast, PSM 3 | Better scan transcription and lower measured time/RSS than Ocrs on this page. Apply OCR to pages with no useful text in the production adapter; mixed text/image PDFs need page-level tests in phase 13. |
| DOCX | zip 9.2.2 and quick-xml 0.42.0 | The table text and asserted core properties are recovered. Image-only body content correctly stays empty. Traverse relationships and concatenate text runs correctly in the production adapter. |
| PPTX | zip and quick-xml, including notes and core properties | The merged table, notes, and asserted metadata are recovered. The slide-number decoration is reported below. Numeric part-name order in this spike is not a substitute for presentation relationships in production. |
| XLSX | calamine 0.38.0, with bounded ZIP preflight and core-property XML | Both the real workbook and generated 100,000-row workload match their full references. Range allocation happens before the cell-count check, so process memory limits remain necessary. |
| PNG/JPEG | image 0.25.10 and Ocrs 0.13.1; decide the single-character-line policy before adoption | Ocrs retrieves more map anchors and sees the STOP sign. The unfiltered output adds noise; the optional filtered configuration is evaluated separately. Removing one-character lines can discard real labels or an isolated "I". The models require CC BY-SA 4.0 attribution. |
| EXIF | Hold production adoption of kamadak-exif 0.6.1 pending a maintenance decision | Dimension extraction and absent-EXIF handling work, but this corpus asserts no populated EXIF tags. Its release/activity age fails the requested freshness criterion. Do not silently waive that criterion. |

Keep default word shingles at k=3 and the already approved short-text equality rule. Tesseract's scan output clears the existing >0.75 exact Jaccard threshold against the reference with that policy. Character shingles help the noisier Ocrs scan, but one page and one short photograph cannot calibrate false matches or justify changing the service's word-shingle requirement. If a later change is approved, use the same normalization/shingle configuration on both sides and a new signature version; never switch policy only at query time.

## Evidence

- [Corpus and provenance](../spikes/extraction/manifest.json), including ten licensed real source files and generated limit/failure cases. Generated files supply resource evidence only.
- [Measurements](../spikes/extraction/measurements.json), with every repetition, input/lock/source hashes, crate versions/revisions, OCR model/language hashes, commands, limits, machine/runtime details, and nullable metric reasons.
- [Complete case table](../spikes/extraction/results/summary.md) and [machine-readable summaries](../spikes/extraction/results/summary.json).
- [Raw results archive](../spikes/extraction/results/raw-results.tar.gz), containing stdout, stderr, extracted text, metadata, and GNU time output. Every member's checksum is recorded in its trial; the archive hash is recorded in measurements.json. Map OCR derivatives retain the source's CC BY-SA 4.0 terms and Euskaldunaa attribution; the corpus [license notices](../spikes/extraction/licenses/) apply to the other files and their derivatives.
- [Shingle experiment](../spikes/extraction/results/shingles.json), including unrelated controls and the exact symmetric normalization policy.
- [Candidate audit](../spikes/extraction/audit/README.md), with current crates.io/GitHub snapshots, license/MSRV/download data, maintenance concerns, pinned assets, and RustSec output.
- [Reproduction commands](../spikes/extraction/README.md#phase-3-benchmark).

## Measured environment and method

Trials ran November 3, 2026 in Asia/Manila (UTC timestamps are November 2). The machine is an Apple M1 running Linux ARM64 through Docker Desktop. A container CPU quota of `200000 100000` limits combined execution to two cores. `memory.max` is 4,294,967,296 bytes, `memory.swap.max` is zero, and the VM reports 4,010,356 KiB of RAM, slightly less than the nominal cap. These results describe that VM and CPU quota, not an unspecified future two-core server.

One candidate process runs at a time. RTen, Rayon, and OpenMP are each limited to two threads. There is one warmup and five measured fresh-process runs per candidate/configuration/case. OS caches are uncontrolled; models are loaded anew on every invocation. Startup, model loading, PDF rasterization, native OCR children, and process shutdown are included in wall time. Very short office/error trials are dominated by startup and the collector's roughly 2 ms polling interval. Do not infer small speed differences from those rows or claim concurrent-worker throughput.

The collector samples summed RSS for every process in the trial's process group, including Tesseract. It also records GNU time's maximum individual-process high-water RSS, converting KiB to bytes. The reported peak estimate is the larger of those values. Shared pages can be counted twice, and simultaneous short-lived peaks between samples can be missed. This is a process-tree estimate, not exact physical memory accounting or proof that two workers have the same latency. The cgroup supplies the hard memory ceiling independently of sampling.

The Rust binary is compiled in release mode with Rust 1.98.1. Both base images are digest-pinned. Crates have exact direct pins and a separate lockfile; dependencies remain outside the Hello World application. The package inventory and runtime image id are retained with the run. OS-package versions are recorded, though apt repository contents can change on rebuild. Fetched assets must match checked-in SHA-256 values; mutable model URLs fail closed if their bytes change.

Accuracy follows the corpus protocol: NFC, straightened quotes, joined line-break hyphenation, collapsed whitespace, and case folding for OCR-page references. WER/CER are edit distance divided by reference length and may exceed one. Coverage is ordered-token LCS or matched anchors. Partial PDF/map anchor references receive no full-document WER/CER. Metadata accuracy considers only asserted fields. Empty references and failed extractions have explicit null reasons. Full raw output remains available for reading-order and noise inspection.

## Real-file results

There are 414 raw trials: 69 candidate/configuration/case combinations, each with one warmup and five measured runs. Text output hashes are identical across measured repetitions for every combination. The table below uses only real files. Timing is the measured median; RSS is the maximum peak estimate across measured runs.

| Real case | Candidate/configuration | ms | RSS MiB | WER | CER | Coverage | Metadata |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| scan-pdf | lopdf | 3.51 | 4.88 | 1.0000 | 1.0000 | 0.0000 | 1.0000 |
| scan-pdf | pdfium | 5.63 | 7.07 | 1.0000 | 1.0000 | 0.0000 | 1.0000 |
| scan-pdf | pdfium-ocrs | 1816.67 | 202.57 | 0.1621 | 0.0735 | 0.8586 | 1.0000 |
| scan-pdf | pdfium-tesseract | 1309.30 | 93.96 | 0.0862 | 0.0644 | 0.9724 | 1.0000 |
| map-png | image-ocrs | 421.26 | 162.93 | n/a | n/a | 0.7647 | 1.0000 |
| map-png | image-ocrs-filtered | 411.81 | 162.90 | n/a | n/a | 0.7647 | 1.0000 |
| map-png | image-tesseract | 136.60 | 35.69 | n/a | n/a | 0.5294 | 1.0000 |
| map-png | image-tesseract-sparse | 155.75 | 35.58 | n/a | n/a | 0.6471 | 1.0000 |
| table-docx | ooxml | 3.36 | 5.01 | 0.0000 | 0.0000 | 1.0000 | 1.0000 |
| notes-pptx | ooxml | 3.38 | 4.30 | 1.0000 | 0.4000 | 1.0000 | 1.0000 |
| table-pptx | ooxml | 3.34 | 4.30 | 0.0000 | 0.0000 | 1.0000 | 1.0000 |
| workbook-xlsx | calamine | 3.43 | 4.55 | 0.0000 | 0.0000 | 1.0000 | 1.0000 |
| digital-pdf | lopdf | 19.83 | 19.71 | n/a | n/a | 1.0000 | 1.0000 |
| digital-pdf | pdfium | 17.86 | 11.62 | n/a | n/a | 1.0000 | 1.0000 |
| photo-jpeg | image-ocrs | 197.58 | 164.07 | 5.0000 | 2.5000 | 1.0000 | 1.0000 |
| photo-jpeg | image-ocrs-filtered | 194.28 | 164.07 | 0.0000 | 0.0000 | 1.0000 | 1.0000 |
| photo-jpeg | image-tesseract | 86.80 | 37.40 | 1.0000 | 1.0000 | 0.0000 | 1.0000 |
| photo-jpeg | image-tesseract-sparse | 155.37 | 39.43 | 1.0000 | 5.2500 | 1.0000 | 1.0000 |

The notes output is `Notes` followed by the automatic slide number `1`. Its one-token reference is `Notes`, so the unadjusted WER is 1 and CER is 0.4, while reference coverage and metadata accuracy are 1. The corpus explicitly permits this decoration; the metric is preserved rather than suppressed. Image-only DOCX/PPTX files return `no_extractable_text` and recover all asserted metadata. Embedded-image OCR in those formats remains outside the approved body/slide policy.

The raw Ocrs photo output includes `STOP`, `O`, `9`, `0`, `:`, and `?`. Its full-reference WER is 5 and CER is 2.5 despite perfect reference-token coverage. Coverage alone would conceal a poor similarity input. Filtered and sparse-layout trials expose the effect of configuration without changing the corpus reference or hiding unfiltered output. The small map's anchor score does not measure ordering or full transcription accuracy, and English-only trials do not establish multilingual support.

## OCR and shingle policy

Exact Jaccard against the complete reference, using the first measured repetition (all text hashes were stable):

| Case / OCR configuration | Word 1 | Word 2 | Word 3 | Character 3 | Character 5 |
| --- | ---: | ---: | ---: | ---: | ---: |
| scan-pdf / pdfium-ocrs | 0.8926 | 0.7270 | 0.6176 | 0.9235 | 0.8287 |
| scan-pdf / pdfium-tesseract | 0.9375 | 0.9216 | 0.8997 | 0.9584 | 0.9504 |
| photo-jpeg / image-ocrs | 0.2500 | 0.0000 | 0.0000 | 0.2500 | 0.0000 |
| photo-jpeg / image-ocrs-filtered | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| photo-jpeg / image-tesseract | 0.0000 | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| photo-jpeg / image-tesseract-sparse | 0.5000 | 0.0000 | 0.0000 | 0.2500 | 0.0000 |

Tesseract PSM 3 clears >0.75 with word k=3 on the scan (0.8997); Ocrs does not (0.6176). Ocrs character k=5 reaches 0.8287, but that changes the requested similarity semantics. On the photo, filtered Ocrs reaches exact equality; unfiltered Ocrs and both Tesseract modes remain below the threshold with word k=3. A one-word reference exercises the approved short-text rule, not paragraph-scale OCR robustness.

The shingle experiment strips punctuation and retains Unicode alphanumerics and whitespace after corpus normalization; character shingles include word-boundary spaces. It applies exactly the same function to reference/ingest and OCR/query. Word k=1, 2, 3 and character k=3, 5 are compared against complete scan/photo references. The unordered map anchors are excluded because their listed order cannot supply a valid sequence reference. Controls use the other complete references for the scan, photo, DOCX table, PPTX table, and workbook. Those few unrelated controls cannot estimate false-positive rates in a customer corpus.

Character shingles are an evaluated alternative, not a selected fallback. Keep word k=3 unless a separate policy change is approved. Likewise, filtered image output is an explicit extraction-policy proposal; do not apply a filter silently only to queries.

## Resource and failure results

The experimental guards are 20 MiB input, 64 MiB cumulative archive expansion, 32 MiB per member, 100x expansion ratio, 4,096 entries, XML depth 128, 25 million image pixels before decoding, 256 MiB decoder allocations, 100 PDF pages, 256 sheets, one million cells per sheet, and 8 MiB final text. Each process group has a 60 s wall deadline, inherited 60 s per-process CPU limit, 16 MiB per-file output limit, no core dumps, a 128 MiB temporary filesystem, and the container's 4 GiB memory/two-CPU ceiling. No network or writable corpus access is available; the image runs as a non-root user with capabilities dropped. Temporary files are isolated per trial and removed; deadline handling kills the entire process group. CPU limits are per process, while the wall deadline and cgroup bound the group as a whole.

Across warmups and measured trials, the largest RSS estimate was 202.57 MiB and the longest wall time was 1886.32 ms. No trial timed out or exited abnormally. The generated large workbook took 63.50 ms median, with 14.89 MiB maximum measured RSS estimate. All candidate/configuration repetitions returned the same outcome and text. These observations cover this corpus under the experimental guards, not all inputs up to the upload limit.

All three expansion packages are rejected during ZIP-entry preflight, before their added XML member is inflated. All huge-image candidate variants refuse the 100-million-pixel PNG before decoding/OCR. Every over-limit input is rejected before the candidate parser. Exactly 20 MiB inputs reach the candidate; an accepted upload does not guarantee successful extraction or OCR. The large XLSX's 100,000 ordered rows match the reference exactly. Generated padding and the synthetic large workbook do not establish real-file quality.

Malformed files and the empty PDF fail in bounded time. Their candidate result is `extraction_failed`; this spike does not implement the service's `malformed_file`/`empty_file` error mapping. The PNG disguised as DOCX fails ZIP parsing, but this does not satisfy `detect_png` content routing. Phase 13 must inspect magic/package structure and implement those contract outcomes. These gaps are recorded as adapter work, not counted as successful content detection.

## Proposed subprocess boundary

Use a supervised extraction subprocess for the selected Rust/native stack. A running Rust decoder cannot be safely killed as a thread. PDFium rendering and Tesseract must share the file's process group, deadline, output budget, temporary directory, and aggregate CPU/memory controls. Preserve the measured limits as an initial proposal, then exercise mixed PDFs, populated EXIF, rotated/multilingual photos, complete OOXML relationships, and larger realistic files in phase 13 before declaring production defaults. Archive sizes and cell metadata are preflight hints; retain the hard process/cgroup boundary for dishonest or malformed inputs. Limit actual pipe output while reading it in the production adapter; this spike also relies on the cgroup while native output is buffered.

Tesseract is a proposed scanned-PDF tool, and Ocrs is a proposed standalone-image tool. No runtime retry between engines is implemented or recommended as an automatic quality fix. A fallback choice or a change to extraction/shingle requirements requires approval first.

## Verification and approval gate

- All 34 corpus fixtures, references, and license evidence hashes verified.
- All 414 trial records and their 2,070 raw artifact hashes verified; the raw archive hash also verifies.
- Two Rust tests pass, including XML entity/truncation/DTD behavior and the real scan's indirect Producer metadata regression. The metadata regression was reproduced before fixing the adapter to use lopdf's reference resolver; the published trials use the corrected binary.
- Fourteen Python tests pass in the bounded Linux container, including deadline/process-group memory collection, output-file limits, accuracy metrics, shingle symmetry, short-text policy, and the five existing corpus-validator tests.
- Both Rust packages pass formatting, Clippy with warnings denied, tests, and warning-free rustdoc. Both are binary-only; there are no library doctests to run.
- cargo-audit 0.22.2 reports zero RustSec vulnerabilities and no warnings for the spike lockfile. This does not audit PDFium, Tesseract, or OS packages.
- Source/lockfile hashes match the published run. The final image and runtime versions are recorded in measurements.json. No application code, database, HTTP endpoint, or production defaults changed.

Approve the proposed per-format tools, native-process boundary, and experimental limits before phase 13 consumes them. The decisions that need explicit attention are Ocrs's CC BY-SA models, whether to discard single-character image lines, and whether to waive the freshness criterion for kamadak-exif or evaluate another EXIF implementation. Word k=3 remains unchanged. No fallback, maintenance exception, or requirement change has been selected on the user's behalf.
