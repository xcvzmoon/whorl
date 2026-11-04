# Extraction runtime

Phase 13 adds a content-detected extractor for PDF, DOCX, PPTX, XLSX, PNG and JPEG. `Extractor` supervises a separate `whorl-extract` process per file. Queue integration is phase 14/15; the application binary still has no HTTP endpoints.

The caller must acquire a bounded CPU slot before extraction. Dropping the extraction future kills its process group; a wall timeout also waits for the child to exit. The private temporary directory is removed on completion or cancellation. Input bytes are copied once to that directory, without trusting an upload filename or extension. Archive package relationships and content types select the Office adapter. External relationships are never fetched.

PDFium reads native PDF text. Pages without alphanumeric native text are rasterized at 300 DPI and passed to Tesseract PSM 3 with English tessdata_fast. PNG/JPEG use Ocrs, apply image orientation, and retain single-character lines. OCR language coverage and quality remain limited to the measured corpus. Metadata is returned separately and is never substituted for body text. A document with no body text fails with `no_extractable_text`.

## Limits and deployment

`ExtractionLimits::default()` preserves the approved initial ceilings: 20 MiB input, 64 MiB total archive expansion, 32 MiB/member, 100x expansion, 4096 entries, XML depth 128, 100 PDF pages, 100 slides, 256 sheets, one million cells per sheet, 25 million pixels per decoded image/rendered page, and 8 MiB combined text and metadata. Image decoder allocations have a 256 MiB cap. ZIP preflight checks declared and actual expansion; XML rejects DTDs and undeclared entities. Workbook range allocation occurs inside the child before the cell count check, so process memory isolation remains required.

The Linux supervisor requires `prlimit` from util-linux. It sets 60 CPU seconds, a 1 GiB per-process address-space cap, 16 MiB output-file cap, and disables core dumps. Native children inherit these limits. Configure the wall deadline per file (initially 60 seconds). Live protocol output is bounded even if the child misbehaves. Missing runtime libraries/models or an unsupported host return `extraction_failed`; there is no in-process fallback.

Deploy with a two-CPU, 4 GiB cgroup, no swap, a 128 MiB `/tmp` tmpfs, read-only root filesystem, a non-root user, dropped capabilities, no network, and no more than two extraction workers. These deployment limits complement per-process limits; they are not configured by a library constructor. Filesystem/model configuration is trusted operator input. The image currently packages the approved Linux ARM64 PDFium artifact.

Runtime environment variables point to trusted assets:

- `WHORL_PDFIUM_LIBRARY`: PDFium shared library.
- `WHORL_OCR_DETECTION_MODEL` and `WHORL_OCR_RECOGNITION_MODEL`: Ocrs ONNX model files.
- `TESSDATA_PREFIX`: directory containing `eng.traineddata`.

Build from the repository root after verifying the assets:

```sh
python3 spikes/extraction/fetch_assets.py
docker build --target runtime -f docker/extraction.Dockerfile -t whorl-extraction:phase13 .
```

The image runs the private extraction protocol, not an HTTP service. Service packaging and job wiring remain later phases. Ocrs model attribution and distribution obligations are recorded in the [asset audit](../spikes/extraction/audit/README.md) and [license evidence](../spikes/extraction/licenses/). Preserve those notices when distributing the models. `nom-exif` 3.8.0 replaces kamadak-exif after a maintenance evaluation; the [dependency snapshot](extraction-dependencies.json) records its MIT license, MSRV and release evidence. rten remains pinned to the spike-tested version compatible with Ocrs; zip remains pinned to the tested adapter version.

## Verification

Host checks cover all Office adapters, relationship ordering, text-run joining, XML entities/depth/truncation, ZIP traversal/expansion, image pixel limits, populated and absent EXIF, and worker error mapping. Linux tests additionally exercise the supervisor's deadline, descendant cleanup, temporary cleanup and live output cap.

Run the Linux supervisor tests with:

```sh
docker build --target tests -f docker/extraction.Dockerfile -t whorl-extraction-tests:phase13 .
```

To reproduce the bounded native corpus run:

```sh
docker build --target verification -f docker/extraction.Dockerfile -t whorl-extraction-verification:phase13 .
docker run --rm --init --network none --cpus 2 --memory 4g --memory-swap 4g \
  --read-only --cap-drop ALL --security-opt no-new-privileges \
  --tmpfs /tmp:rw,noexec,nosuid,size=128m \
  -v "$PWD/spikes/extraction:/corpus:ro" \
  -v "$PWD/scripts/verify-extraction.py:/verify-extraction.py:ro" \
  whorl-extraction-verification:phase13 > docs/extraction-results.json
```

The corpus runner verifies source hashes and expected outcomes, records wall time and GNU time maximum process RSS, checks ordered text (permitting the corpus's slide-number decoration), verifies asserted metadata, and records OCR WER/CER/coverage using the phase 3 metrics. A mixed native/scanned PDF additionally verifies page-level OCR routing. RSS excludes simultaneous OCR descendants; the cgroup supplies the aggregate hard ceiling. These measurements establish behavior on this corpus and machine, not production capacity or general extraction accuracy. Phase 3 retains the detailed quality comparison.

The recorded phase 13 run passed all 34 corpus cases and the mixed PDF check. Linux library/integration tests passed (eight and six tests respectively). Host formatting, Clippy, rustdoc and all 37 doctests passed; the existing database/MinIO integration tests were excluded. Asset checksums verify and RustSec reports no advisories or warnings. See [recorded measurements](extraction-results.json).
