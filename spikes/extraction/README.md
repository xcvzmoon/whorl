# Extraction corpus

Phase 2 supplies 34 checksum-pinned cases for the phase 3 extraction spike. Phase 3 records 414 trials and per-format recommendations; production selection is awaiting approval. `measurements.json` retains the preparation host alongside the bounded Linux benchmark machine.

Run from the repository root with Python 3.9 or later:

```sh
python3 spikes/extraction/corpus.py generate
python3 spikes/extraction/corpus.py verify
python3 -m unittest discover -s spikes/extraction -p 'test_*.py'
```

The commands need no Python packages or network access. Real source files, references, and license notices are checked in. Generated files occupy about 250 MiB and are ignored by Git. Delete `spikes/extraction/generated/` after use. Generation overwrites only named generated files. `verify` checks hashes against the checked-in manifest; it never updates expected hashes or runs an extractor.

## Real files and provenance

`manifest.json` contains each source URL, revision where available, attribution, license evidence, byte size, and SHA-256. Upstream GitHub URLs use full commit IDs. The IRS URL is mutable, so the vendored bytes and checksum identify the acquired 2026 edition. The photograph's file revision is 2008-03-11 19:09 UTC, linked from its pinned Commons description revision.

| Cases | Source | Coverage | License |
| --- | --- | --- | --- |
| `digital-pdf` | IRS Form W-4, 2026 | Five-page born-digital form, text, document info, XFA | US government work; see public-domain notice |
| `scan-pdf` | OCRmyPDF / Project Gutenberg | Real printed Huckleberry Finn page, illustration and scan noise | Public domain per upstream per-file declaration |
| `map-png` | OCRmyPDF / Euskaldunaa | Small multilingual map labels, image dimensions | CC BY-SA 4.0 |
| `photo-jpeg` | Wikimedia Commons / Dori | Real photograph of a STOP sign | Photographer's public domain dedication |
| `table-docx`, `images-docx` | python-docx | Merged tables; embedded images with no body text | MIT, copyright Steve Canny |
| `notes-pptx`, `table-pptx`, `picture-pptx` | python-pptx | Speaker notes, merged cells, embedded pictures | MIT, copyright Steve Canny |
| `workbook-xlsx` | python-pptx | Excel-authored workbook, numeric cells, workbook properties | MIT, copyright Steve Canny |

The office documents are existing upstream regression documents, rather than documents generated for this spike. They are a small public corpus, not evidence of a representative customer workload. The single-word photograph cannot establish accuracy for paragraphs photographed under varying lighting. Add licensed customer-like examples before making broader quality claims.

The original license texts and OCRmyPDF's per-file attribution declarations are in `licenses/`. Keep these with redistributed fixtures. The map and its derivatives retain CC BY-SA 4.0, with attribution to Euskaldunaa (2014). The other derivative fixtures retain their parents' terms. No application-wide license is implied.

## Generated cases

All recipes are in `corpus.py`. The manifest records Python and zlib versions used to pin their bytes. ZIP compression can differ between zlib versions; a checksum failure is a reproducibility failure to investigate, not a reason to silently rewrite the manifest.

- `boundary.*` is exactly 20,971,520 bytes for each of PDF, DOCX, PPTX, XLSX, PNG, and JPEG. PDF gets a comment before `startxref`; existing object offsets remain intact. Office packages use stored ZIP entries and an XML padding part. PNG gets a private ancillary chunk before IEND. JPEG gets APP15 segments after SOI. These cases exercise input-size handling; padding is not evidence of realistic extraction cost.
- `boundary.xlsx` also replaces Sheet1 with 100,000 inline-string rows (`row 000001` through `row 100000`). This is the large spreadsheet workload. Its complete reference is `expected/large-xlsx.txt`.
- `over-limit.*` adds one byte to each boundary file. Reject at upload counting before extraction, irrespective of parser tolerance for trailing bytes.
- `malformed.*` retains just the first 32 bytes of each boundary file. These are intentionally truncated inputs.
- `expansion.docx`, `.pptx`, and `.xlsx` preserve the original package and add a deflated XML member containing 64 MiB of spaces, with an expansion ratio above 100. Inspect ZIP entry sizes first. Do not inflate these as part of routine verification.
- `huge.png` is a valid 10,000 by 10,000 grayscale PNG. It expands to 100,000,000 pixel bytes plus scanline filters despite a small compressed file. Generate it a row at a time; inspect IHDR before decoding.
- `empty.pdf` is empty. `spoofed.docx` contains the map PNG bytes and exercises content detection independently of extension.

Expansion and image cases have outcome `limit_probe`. Phase 3 must record explicit candidate limits and whether each probe succeeds within them or returns `extraction_limit` (or `no_extractable_text` for the blank image); a safe bounded refusal is acceptable. These probes do not prescribe production thresholds before measurement. Never open hostile cases in a desktop viewer. Run candidate decoders in disposable subprocesses with memory, CPU, wall-time, and output caps.

## CorpusCase

Each `cases` entry in `manifest.json` has:

| Field | Meaning |
| --- | --- |
| `id`, `path`, `format` | Stable case key, relative local path, format family |
| `category`, `origin` | Coverage purpose; `upstream` or `generated` |
| `bytes`, `sha256` | Exact input length and lowercase SHA-256 |
| `provenance` | Source URL/revision/path or generator recipe/parent; license, attribution and evidence |
| `expected.outcome` | Success, stable API error, content-detection result, or limit probe |
| `expected.text_path`, `text_sha256` | Reference text and its checksum, when applicable |
| `expected.comparison` | Reference scope and comparison method described below |
| `expected.metadata` | Assert only these known fields; omitted fields are unspecified, not required absent |

Metadata keys use semantic names, independent of a candidate library's API. ISO timestamps remain UTC strings. `notes_pages` and `sheet_names` describe source structure. Missing document properties are valid. Image references specify width and height; do not invent absent EXIF properties. The image-only DOCX and PPTX cases expect `no_extractable_text` under the specified body/slide extraction policy; embedded-image OCR is outside that policy. Notes content is `Notes`; automatic slide number `1` is optional decoration.

## Reference text and accuracy

Office references come from inspection of the existing OOXML text/cell values and core properties, independently of any candidate extraction implementation. Table tokens follow document order, preserving repeated cells. The scanned page was manually transcribed against a rendered image; the caption precedes the body in the reference. The photo reference was checked visually. The map reference lists labels without imposing a reading order and omits scale tick numerals. The IRS reference contains selected first-page anchors, not a complete transcription.

Use Unicode NFC, normalize curly apostrophes/quotes to straight equivalents, collapse whitespace, and join line-break hyphenations before comparison. Report the normalization version with every trial. For `ocr-page`, compare case-insensitively, including punctuation; use the stated caption order for the scan. Keep raw output so reading-order errors can be inspected. For `ordered-tokens`, compare all whitespace-delimited reference tokens in order. For `anchors` and `unordered-anchors`, report the fraction of reference lines found after normalization, allowing any order. Do not report full-document WER or CER from partial anchor references.

WER is word-level Levenshtein distance divided by reference word count. CER is character-level Levenshtein distance divided by reference character count after normalization. Both can exceed 1 due to insertions. Coverage is the fraction of reference tokens matched by longest common subsequence for ordered references, or matched anchors for anchor cases. Metadata accuracy is correct expected fields divided by expected field count, using exact normalized values. Empty references and unmeasured metrics are `null`, never zero. Report errors separately from successful trials; an extraction failure cannot receive perfect accuracy.

## ExtractionMeasurement

Phase 3 appends one trial per case, candidate, configuration, and repetition to `measurements.json.trials`. Each trial must record:

- `case_id`, input `sha256`, candidate name/version and source revision, dependency lockfile hash, OCR model/language hashes, command arguments, configuration and enforced limits.
- `machine_id` referencing a machine record with OS, architecture, CPU model, available/allocated cores, RAM, container/runtime versions, and imposed memory/CPU limits. The recorded preparation host is explicitly not the two-core/4 GiB deployment target.
- UTC start time, warmup/measured repetition number and cache state, wall time in milliseconds, peak process-tree RSS in bytes, exit status, outcome/error code, raw stdout/stderr artifact paths and hashes.
- Extracted text/metadata artifact paths and hashes, normalization version, WER, CER, coverage, metadata accuracy, and the comparison scope. Nullable metrics must carry a reason such as `not_run`, `partial_reference`, or `failed`.

Use a monotonic clock for wall time and process-tree peak memory, including external OCR children. Record the collector and units; macOS `time -l` reports bytes, while Linux `time -v` reports KiB. A main-process-only RSS value is insufficient for a subprocess extractor. Use at least one warmup and five measured repetitions per successful case, preserving raw trials before summarizing. Run with at most two workers and a 4 GiB memory cap for target claims. Corpus preparation/validation timings are not extraction timings.

## Phase 2 verification

Verified all 34 fixture hashes, reference hashes, license evidence hashes, and required coverage. Regeneration produced identical bytes on the recorded Python/zlib host. Poppler opened the 20 MiB PDF with the same five pages, metadata, and extracted text as the source. macOS ImageIO read both boundary image dimensions unchanged. All boundary office ZIP CRCs and XML parsed; the large workbook contains 100,000 rows. Negative checks rejected altered reference text and missing format coverage. These are corpus checks, not candidate performance or quality results.

## Phase 3 benchmark

The standalone Rust package compares candidate libraries without adding dependencies to the application. See [the report](../../docs/extraction-spike.md), [candidate audit](audit/README.md), [raw measurement records](measurements.json), and [all case summaries](results/summary.md). The report distinguishes completed experiments from recommendations awaiting approval.

Reproduce on Linux ARM64 or Docker Desktop with an ARM64 Linux VM. From the repository root:

```sh
python3 spikes/extraction/corpus.py generate
python3 spikes/extraction/corpus.py verify
python3 spikes/extraction/fetch_assets.py
cargo fmt --manifest-path spikes/extraction/Cargo.toml --check
cargo clippy --manifest-path spikes/extraction/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path spikes/extraction/Cargo.toml
RUSTDOCFLAGS='-D warnings' cargo doc --manifest-path spikes/extraction/Cargo.toml --no-deps
docker build -t whorl-extraction-spike spikes/extraction
mkdir -p /tmp/whorl-extraction-results
docker run --rm --cpus=2 --memory=4g --memory-swap=4g --pids-limit=128 \
  --network=none --cap-drop=ALL --security-opt=no-new-privileges \
  --read-only --tmpfs /tmp:rw,nosuid,nodev,size=128m \
  -v "$PWD/spikes/extraction:/corpus:ro" \
  -v /tmp/whorl-extraction-results:/out whorl-extraction-spike /out
```

Use a fresh output directory for every run. Existing trial directories cause an error instead of overwriting evidence. Container CPU quota is two cores and the cgroup memory cap is 4 GiB with swap disabled. The parent runs one candidate at a time. Engines are allowed up to two threads each, with the quota bounding their combined CPU time. The recorded Docker VM has slightly less than 4 GiB available; the report states that limit. This is a quota-limited Apple M1 experiment, not a hardware-independent capacity claim.

`fetch_assets.py` verifies each download against `audit/assets.json`. Model URLs without immutable revisions may stop reproducing if the upstream bytes change; a hash mismatch stops the script. It never updates pins. PDFium and Tesseract archives, models, and build outputs are ignored. Keep their upstream license notices when redistributing a built image. The Ocrs models are CC BY-SA 4.0 with attribution to Ocrs and Google Research's HierText dataset; see `audit/ocrs-models-LICENSE.md`.

Run the collector's Linux isolation tests in the same sandbox (without another benchmark running):

```sh
docker run --rm --cpus=2 --memory=4g --memory-swap=4g --pids-limit=128 \
  --network=none --cap-drop=ALL --security-opt=no-new-privileges \
  --read-only --tmpfs /tmp:rw,nosuid,nodev,size=128m \
  -v "$PWD/spikes/extraction:/corpus:ro" --entrypoint python3 \
  whorl-extraction-spike -m unittest discover -s /corpus -p 'test_*.py'
```

Raw stdout, stderr, text, metadata, and GNU time reports are stored in `results/raw-results.tar.gz`. Their archive-relative paths and SHA-256 hashes are in `measurements.json`. Recompute summaries and verify every artifact without extracting the archive:

```sh
python3 spikes/extraction/summarize.py spikes/extraction/results --measurements spikes/extraction/measurements.json
```

Reproduction runs produce a measurement file in their own output directory. To summarize a new run, first archive its raw directory with `tar -czf /tmp/whorl-extraction-results/raw-results.tar.gz -C /tmp/whorl-extraction-results raw`, then pass that directory to `summarize.py`.

Both Rust packages are binary-only and expose no public library API or doctests. `cargo doc` is checked with warnings denied; `cargo test --doc` has no library target to run. The spike's format adapters intentionally cover this experiment. Production adapters, API error mapping, OOXML relationship traversal and complete text-run handling belong to phase 13 after approval.
