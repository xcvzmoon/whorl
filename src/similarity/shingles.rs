//! Word shingles and exact equality for short canonical text.

use std::{collections::BTreeSet, error::Error, fmt, num::NonZeroUsize};

use super::normalize::NormalizationVersion;

/// Identifies extraction rules that may share a similarity index.
///
/// Both profiles preserve extracted lines and characters before normalization.
/// OCR uses the same word-shingle rules as native text, without correction,
/// single-character-line filtering, or character-shingle fallback. Distinct
/// profiles occupy separate comparison namespaces. Selecting an OCR engine and
/// its models remains the extraction adapter's responsibility.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceProfile {
    /// Native extracted text without OCR filtering or correction.
    NativeTextV1,
    /// Verbatim OCR output without filtering, spelling repair, or dehyphenation.
    OcrVerbatimV1,
}

/// The canonical input contains no words (`no_extractable_text`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NoExtractableText;

impl fmt::Display for NoExtractableText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("no_extractable_text")
    }
}

impl Error for NoExtractableText {}

/// Canonical text and its unique consecutive word shingles.
///
/// Owns canonical text and sorted unique shingle ranges. Verification reuses
/// those ranges without rebuilding a set. Inputs shorter than k use full-text equality.
/// Normalization, source profile, k, and the short-text rule are included in
/// [`super::signature::SignatureVersion`]. Run construction outside async executor threads.
#[derive(Debug)]
pub struct ShingleSet {
    text: String,
    ranges: Vec<(usize, usize)>,
    pub(crate) k: NonZeroUsize,
    normalization: NormalizationVersion,
    pub(crate) source: SourceProfile,
}

impl ShingleSet {
    /// Normalizes extracted text and prepares unique word-shingle ranges.
    ///
    /// # Arguments
    ///
    /// * `text` - Extracted text. Ingest and check must use the same policy.
    /// * `normalization` - Canonicalization rules for this index.
    /// * `source` - Versioned native or verbatim OCR text policy for this input.
    /// * `k` - Number of consecutive words per shingle; `None` selects three.
    ///
    /// # Returns
    ///
    /// An owned input whose shingles use set semantics.
    ///
    /// # Errors
    ///
    /// Returns [`NoExtractableText`] (`no_extractable_text`) if normalization
    /// produces no words.
    ///
    /// # Examples
    ///
    /// ```
    /// use whorl::similarity::{normalize::NormalizationVersion, shingles::{ShingleSet, SourceProfile}};
    /// let input = ShingleSet::new("One two three four", NormalizationVersion::V1,
    ///     SourceProfile::NativeTextV1, None)?;
    /// assert_eq!(input.shingles().len(), 2);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new(
        text: &str,
        normalization: NormalizationVersion,
        source: SourceProfile,
        k: Option<NonZeroUsize>,
    ) -> Result<Self, NoExtractableText> {
        let text = normalization.normalize(text);
        if text.is_empty() {
            return Err(NoExtractableText);
        }
        let mut offset = 0;
        let words: Vec<_> = text
            .split(' ')
            .map(|word| {
                let start = offset;
                offset += word.len() + 1;
                (start, start + word.len())
            })
            .collect();
        let k = k.unwrap_or(const { NonZeroUsize::new(3).unwrap() });
        let mut ranges = words
            .windows(k.get())
            .map(|window| {
                let range = (window[0].0, window[window.len() - 1].1);
                (&text[range.0..range.1], range)
            })
            .collect::<Vec<_>>();
        ranges.sort_unstable_by_key(|&(slice, _)| slice);
        ranges.dedup_by_key(|(slice, _)| *slice);
        let ranges = ranges.into_iter().map(|(_, range)| range).collect();
        Ok(Self {
            text,
            ranges,
            k,
            normalization,
            source,
        })
    }

    /// Collects unique consecutive k-word slices of the canonical text.
    ///
    /// # Returns
    ///
    /// A lexically ordered set borrowing this input. Short inputs return an empty
    /// set. Repeated occurrences contribute only one member.
    ///
    /// # Examples
    ///
    /// ```
    /// # use whorl::similarity::{normalize::NormalizationVersion, shingles::{ShingleSet, SourceProfile}};
    /// let input = ShingleSet::new("a a a a", NormalizationVersion::V1,
    ///     SourceProfile::NativeTextV1, None)?;
    /// assert_eq!(input.shingles().into_iter().collect::<Vec<_>>(), ["a a a"]);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn shingles(&self) -> BTreeSet<&str> {
        self.slices().collect()
    }

    fn slices(&self) -> impl ExactSizeIterator<Item = &str> {
        self.ranges
            .iter()
            .map(|&(start, end)| &self.text[start..end])
    }

    /// Scores prepared ranges after the caller checks source policy and short text.
    #[expect(
        clippy::cast_precision_loss,
        reason = "Set cardinalities become floating-point ratios for similarity scores"
    )]
    pub(crate) fn jaccard(&self, other: &Self) -> f64 {
        let mut left = self.slices().peekable();
        let mut right = other.slices().peekable();
        let mut intersection = 0;
        while let (Some(a), Some(b)) = (left.peek(), right.peek()) {
            match a.cmp(b) {
                std::cmp::Ordering::Less => {
                    left.next();
                }
                std::cmp::Ordering::Greater => {
                    right.next();
                }
                std::cmp::Ordering::Equal => {
                    intersection += 1;
                    left.next();
                    right.next();
                }
            }
        }
        let union = self.ranges.len() + other.ranges.len() - intersection;
        if union == 0 {
            0.0
        } else {
            intersection as f64 / union as f64
        }
    }

    /// Identifies all text-processing rules used by this input.
    ///
    /// Persist this key as part of the signature version, alongside hash
    /// seeds and band layout. A processing-rule change requires a new key and
    /// reindexing. OCR engine/model revisions must also be recorded by the
    /// extraction adapter; this key describes the similarity policy only.
    ///
    /// # Returns
    ///
    /// A stable ASCII key containing normalization, source policy, word k, and
    /// the short-text equality rule. The key does not depend on input content.
    ///
    /// # Examples
    ///
    /// ```
    /// # use whorl::similarity::{normalize::NormalizationVersion, shingles::{ShingleSet, SourceProfile}};
    /// let input = ShingleSet::new("hello", NormalizationVersion::V1,
    ///     SourceProfile::OcrVerbatimV1, None)?;
    /// assert_eq!(input.version_key(), "nfkc-v1/ocr-verbatim-v1/word-3/short-equality-v1");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn version_key(&self) -> String {
        let normalization = match self.normalization {
            NormalizationVersion::V1 => "nfkc-v1",
        };
        let source = match self.source {
            SourceProfile::NativeTextV1 => "native-text-v1",
            SourceProfile::OcrVerbatimV1 => "ocr-verbatim-v1",
        };
        format!("{normalization}/{source}/word-{}/short-equality-v1", self.k)
    }

    /// Validates policy compatibility before comparing any input lengths.
    ///
    /// Call this before exact shingle comparison as well as short-text scoring.
    /// Raw shingle sets do not carry their policy and cannot enforce this check.
    ///
    /// # Arguments
    ///
    /// * `other` - Input whose processing policy must match this input.
    ///
    /// # Returns
    ///
    /// Unit when normalization, source profile, and word k match.
    ///
    /// # Errors
    ///
    /// Returns [`IncompatibleProfile`] when any processing policy differs.
    ///
    /// # Examples
    ///
    /// ```
    /// # use whorl::similarity::{normalize::NormalizationVersion, shingles::{ShingleSet, SourceProfile}};
    /// let input = ShingleSet::new("one two three", NormalizationVersion::V1,
    ///     SourceProfile::NativeTextV1, None)?;
    /// input.ensure_compatible(&input)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn ensure_compatible(&self, other: &Self) -> Result<(), IncompatibleProfile> {
        if self.normalization != other.normalization
            || self.source != other.source
            || self.k != other.k
        {
            return Err(IncompatibleProfile);
        }
        Ok(())
    }

    /// Applies the equality rule when at least one input is short.
    ///
    /// # Arguments
    ///
    /// * `other` - Input built with the same normalization, source profile, and k.
    ///
    /// # Returns
    ///
    /// `Err(IncompatibleProfile)` for different policies. Otherwise returns
    /// `Some(1.0)` for equal short inputs, `Some(0.0)` for unequal short or
    /// short/long inputs, and `None` when both inputs require shingle comparison.
    ///
    /// # Errors
    ///
    /// Returns [`IncompatibleProfile`] when normalization, source profile, or k differ.
    ///
    /// # Examples
    ///
    /// ```
    /// # use whorl::similarity::{normalize::NormalizationVersion, shingles::{ShingleSet, SourceProfile}};
    /// let input = ShingleSet::new("Hello!", NormalizationVersion::V1,
    ///     SourceProfile::NativeTextV1, None)?;
    /// assert_eq!(input.short_text_similarity(&input)?, Some(1.0));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn short_text_similarity(&self, other: &Self) -> Result<Option<f64>, IncompatibleProfile> {
        self.ensure_compatible(other)?;
        if self.ranges.is_empty() || other.ranges.is_empty() {
            return Ok(Some(if self.text == other.text { 1.0 } else { 0.0 }));
        }
        Ok(None)
    }
}

/// The inputs use different normalization, extraction, or shingle policies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IncompatibleProfile;

impl fmt::Display for IncompatibleProfile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("incompatible similarity profiles")
    }
}

impl Error for IncompatibleProfile {}
