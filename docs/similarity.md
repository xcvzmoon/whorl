# Similarity algorithm

Whorl performs lexical comparison over extracted text. It does not use semantic embeddings. A check searches the client's existing corpus; an ingest performs the same comparison and then stores canonical content. The current HTTP submission path always snapshots text-only scoring with a strict threshold of 0.75. The Rust scoring implementation also supports validated weighted policies; HTTP exposes no policy selector.

## Canonical text and document identity

`NormalizationVersion::V1` applies NFKC, locale-independent scalar lowercase, NFKC, character filtering, whitespace folding, and a final NFKC composition. Letters, numbers, and combining marks remain. Punctuation, symbols, and non-whitespace controls are deleted; punctuation deletion can join words (`co-operate` becomes `cooperate`). Whitespace becomes one ASCII space with no outer whitespace. Accents and repeated lines remain. Lowercase is not Unicode case folding: `ß` differs from `ss`.

Empty canonical text fails with `no_extractable_text`. SHA-256 of canonical text identifies corpus content within one client. First successful ingest wins canonical metadata; later identical uploads do not replace it. Normalization rules and their Unicode tables are versioned. See [normalizer source](../src/similarity/normalize.rs) and [normalization tests](../tests/normalization.rs).

## Word shingles and short text

The default shingle is three consecutive words. Shingles form a set, so repetition does not increase a shingle's weight. Native text and verbatim OCR use separate source profiles. OCR does not silently switch to a different character-shingle policy or remove single-character lines.

For fewer than k words, normalized full-text equality is the only text match: identical short texts score 1, unequal short texts and short/long pairs score 0. Short documents carry version membership without signature arrays. Metadata may still qualify such documents under a weighted policy. See [shingle implementation](../src/similarity/shingles.rs) and [tests](../tests/shingles.rs).

## MinHash and retrieval

Long-text signatures have 125 deterministic values arranged into 25 bands of five. Stable hashing, seeds, source profile, normalization, k, and short-text rules contribute to comparison identity. Persisted unsigned hash bits map reversibly into PostgreSQL signed integers. Band hashing includes band position; 32-bit collisions still exist and add candidates.

A typical full version is:

```text
nfkc-v1/native-text-v1/word-3/short-equality-v1/xxh3-low32-le-v1/125/25x5
```

The matcher first retrieves normalized-content duplicates without suppressing near matches. It then reads UUID-keyset pages of 32 candidates in a read-only repeatable-read transaction. SQL unions band overlap with metadata-token overlap when metadata has positive weight. Candidate membership always requires the same client and full version. Duplicate candidates are removed by the union; exact-content duplicates are excluded from later pages.

For text Jaccard p, idealized LSH retrieval probability is `1 - (1 - p^5)^25`, approximately 0.99885 at p=0.75. Qualifying corpus documents can be missed. MinHash never accepts or rejects a retrieved candidate by estimated score; exact verification controls qualification. The implementation does not promise exhaustive recall. See [approved decisions](decisions.md), [candidate SQL](../database/queries/candidates.sql), and [signature vectors](../tests/fixtures/vectors/).

## Exact verification and metadata

Text similarity is exact set Jaccard, intersection divided by union, under compatible profiles. `SimilarityPolicy` validates finite thresholds and weights in [0,1]. Qualification is strictly greater than the threshold. Default scoring uses text only while reporting metadata similarity separately.

Metadata tokens are deterministic, case-sensitive typed JSON leaf values with escaped paths. Array positions do not affect tokens; repeated elements have set semantics. Nulls and empty containers add no tokens. Keys `created`, `modified`, `created_at`, `modified_at`, `creation_date`, `modification_date`, `size`, `file_size`, and `size_bytes` exclude their whole subtree, matched ASCII case-insensitively. Extractors map format-specific dates and sizes to those names. The implementation defines object/array/null handling in [verify.rs](../src/similarity/verify.rs); [verification tests](../tests/verification.rs) preserve those rules. An undefined empty/empty metadata score is null in results and contributes zero to weighted scoring.

Weighted scoring is `text_weight * text_similarity + (1 - text_weight) * metadata_similarity`. There is no independent text threshold gate. Shared metadata tokens retrieve candidates even when text similarity is zero. A zero-weight contribution needs no corresponding candidate source. Common tokens can expand retrieval substantially.

Every retrieved candidate is verified through the shared CPU pool. Results sort by score descending, then document UUID ascending. Candidate texts are paged, but the full vector of qualifying scores remains in memory until returned.

## Version lifecycle and current limitations

Changing normalization, shingling, hashing, or scoring identity requires an explicit version decision. Reindexing adds missing signatures from stored canonical text and keeps old/new rows separate. It does not extract source files again or relabel old arrays.

`SignatureVersion` parses supported canonical keys and provides both the shingle length and expected source-specific registration configuration. Worker preparation uses that length rather than assuming k=3. Each source registration must match its own version; native/OCR derivation preserves the other algorithm rules without comparing their source fields for equality.

`SignatureIndex` supplies the same signed hash encoding and short-text null arrays to live preparation and reindexing. Unsupported or inconsistent registrations fail with `invalid_job`; no compatibility fallback is applied. Follow [maintenance](maintenance.md) for registering both profiles and performing a coverage-checked cutover. The [architecture review resolutions](architecture-review.md) explain the corrections and regression coverage.
