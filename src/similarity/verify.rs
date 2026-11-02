//! Exact candidate verification and version-one metadata tokens.

use std::{collections::BTreeSet, error::Error, fmt};

use serde_json::Value;

use super::shingles::{IncompatibleProfile, ShingleSet};

/// Validated settings copied into a batch before admission.
///
/// The default is text-only qualification above 0.75. Metadata remains supporting
/// information. A weighted policy uses metadata even when text similarity is zero.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SimilarityPolicy {
    threshold: f64,
    text_weight: Option<f64>,
}

impl Default for SimilarityPolicy {
    fn default() -> Self {
        Self {
            threshold: 0.75,
            text_weight: None,
        }
    }
}

impl SimilarityPolicy {
    /// Validates a policy for snapshotting before batch admission.
    ///
    /// # Arguments
    ///
    /// * `threshold` - Exclusive qualification threshold, finite and in [0, 1].
    /// * `text_weight` - `None` selects text-only scoring; `Some` selects weighted
    ///   scoring with a finite text weight in [0, 1]. Undefined metadata adds zero.
    ///
    /// # Returns
    ///
    /// Immutable, copyable effective settings, independent of later configuration.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidPolicy`] if either supplied number is nonfinite or outside [0, 1].
    ///
    /// # Examples
    ///
    /// ```
    /// use whorl::similarity::verify::SimilarityPolicy;
    /// let policy = SimilarityPolicy::new(0.75, Some(0.8))?;
    /// assert_eq!(policy.text_weight(), Some(0.8));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new(threshold: f64, text_weight: Option<f64>) -> Result<Self, InvalidPolicy> {
        let valid = |value: f64| value.is_finite() && (0.0..=1.0).contains(&value);
        if !valid(threshold) || text_weight.is_some_and(|weight| !valid(weight)) {
            return Err(InvalidPolicy);
        }
        Ok(Self {
            threshold,
            text_weight,
        })
    }

    /// Returns the exclusive qualification threshold.
    ///
    /// # Returns
    ///
    /// The validated threshold, defaulting to 0.75.
    ///
    /// # Examples
    ///
    /// ```
    /// # use whorl::similarity::verify::SimilarityPolicy;
    /// assert_eq!(SimilarityPolicy::default().threshold(), 0.75);
    /// ```
    pub fn threshold(self) -> f64 {
        self.threshold
    }

    /// Returns the snapshotted text weight.
    ///
    /// # Returns
    ///
    /// `None` for text-only qualification, or the weighted policy's text weight.
    ///
    /// # Examples
    ///
    /// ```
    /// # use whorl::similarity::verify::SimilarityPolicy;
    /// assert_eq!(SimilarityPolicy::default().text_weight(), None);
    /// ```
    pub fn text_weight(self) -> Option<f64> {
        self.text_weight
    }
}

/// A threshold or weight is nonfinite or outside [0, 1].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidPolicy;

impl fmt::Display for InvalidPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("similarity threshold and weight must be finite and in [0, 1]")
    }
}
impl Error for InvalidPolicy {}

/// Deterministic, case-sensitive JSON leaf tokens under metadata policy v1.
///
/// Object paths escape `~` as `~0`, `/` as `~1`, and `=` as `~2`.
/// Array positions do not affect tokens;
/// repeated elements have set semantics. Nulls and empty containers add no tokens.
/// Scalar values retain JSON types and escaping. Keys named `created`, `modified`,
/// `created_at`, `modified_at`, `creation_date`, `modification_date`, `size`,
/// `file_size`, or `size_bytes` (ASCII case-insensitive) exclude their whole subtree.
/// Extractors must map format-specific dates and sizes to these canonical keys.
/// This policy must be shared by candidate indexing and verification; changing it
/// requires rebuilding the metadata index.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MetadataTokens(BTreeSet<String>);

impl MetadataTokens {
    /// Flattens metadata into ordered, unique path=value tokens.
    ///
    /// # Arguments
    ///
    /// * `metadata` - JSON metadata, normally an object; root scalars use an empty path.
    ///
    /// # Returns
    ///
    /// Owned tokens with excluded fields and nulls removed. No normalization is
    /// applied to string values. Call on a CPU worker for large metadata objects.
    ///
    /// # Examples
    ///
    /// ```
    /// use serde_json::json;
    /// use whorl::similarity::verify::MetadataTokens;
    /// let tokens = MetadataTokens::new(&json!({"author": "Ada", "size": 42}));
    /// assert_eq!(tokens.iter().collect::<Vec<_>>(), ["/author=\"Ada\""]);
    /// ```
    pub fn new(metadata: &Value) -> Self {
        let mut tokens = BTreeSet::new();
        let mut pending = vec![(String::new(), metadata)];
        while let Some((path, value)) = pending.pop() {
            match value {
                Value::Object(fields) => {
                    for (key, value) in fields {
                        if matches!(
                            key.to_ascii_lowercase().as_str(),
                            "created"
                                | "modified"
                                | "created_at"
                                | "modified_at"
                                | "creation_date"
                                | "modification_date"
                                | "size"
                                | "file_size"
                                | "size_bytes"
                        ) {
                            continue;
                        }
                        let key = key.replace('~', "~0").replace('/', "~1").replace('=', "~2");
                        pending.push((format!("{path}/{key}"), value));
                    }
                }
                Value::Array(values) => {
                    pending.extend(values.iter().map(|value| (path.clone(), value)));
                }
                Value::Null => {}
                scalar => {
                    tokens.insert(format!("{path}={scalar}"));
                }
            }
        }
        Self(tokens)
    }

    /// Borrows the canonical tokens for persistence and candidate lookup.
    ///
    /// # Returns
    ///
    /// Tokens in lexical order, with no duplicates.
    ///
    /// # Examples
    ///
    /// ```
    /// # use whorl::similarity::verify::MetadataTokens;
    /// assert_eq!(MetadataTokens::default().iter().count(), 0);
    /// ```
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }
}

/// Exact component scores and the policy's threshold decision.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MatchScore {
    /// Unrounded policy score in [0, 1].
    pub score: f64,
    /// Exact word-shingle Jaccard or short-text equality score.
    pub text_similarity: f64,
    /// Exact metadata Jaccard; `None` when disabled or both sets are empty.
    pub metadata_similarity: Option<f64>,
    /// Whether the policy score is strictly greater than its threshold.
    pub qualifies: bool,
}

/// CPU-only verifier holding an immutable batch policy snapshot.
///
/// Every retrieved candidate must pass through this verifier. MinHash estimates
/// are deliberately absent from the interface: they may only order candidates.
#[derive(Clone, Copy, Debug)]
pub struct SimilarityEngine {
    policy: SimilarityPolicy,
}

impl SimilarityEngine {
    /// Creates a verifier from validated batch settings.
    ///
    /// # Arguments
    ///
    /// * `policy` - Effective settings copied before admission.
    ///
    /// # Returns
    ///
    /// A verifier with no database, HTTP, or async runtime dependency.
    ///
    /// # Examples
    ///
    /// ```
    /// use whorl::similarity::verify::{SimilarityEngine, SimilarityPolicy};
    /// let engine = SimilarityEngine::new(SimilarityPolicy::default());
    /// ```
    pub fn new(policy: SimilarityPolicy) -> Self {
        Self { policy }
    }

    /// Exactly scores one retrieved candidate and applies the batch policy.
    ///
    /// # Arguments
    ///
    /// * `query` - Query text under the index's versioned shingle policy.
    /// * `candidate` - Stored text under the same policy.
    /// * `metadata` - Query and canonical first-ingest metadata tokens; `None`
    ///   disables metadata scoring. Undefined metadata contributes zero in weighted mode.
    ///
    /// # Returns
    ///
    /// Exact scores and strict qualification, including nonqualifying candidates.
    /// This synchronous work belongs on the bounded CPU pool. Inputs remain owned
    /// by the caller; the verifier performs no persistence or external side effects.
    ///
    /// # Errors
    ///
    /// Returns [`IncompatibleProfile`] if normalization, source profile, or k differ.
    ///
    /// # Examples
    ///
    /// ```
    /// use whorl::similarity::{normalize::NormalizationVersion,
    ///     shingles::{ShingleSet, SourceProfile}, verify::{SimilarityEngine, SimilarityPolicy}};
    /// let text = ShingleSet::new("one two three", NormalizationVersion::V1,
    ///     SourceProfile::NativeTextV1, None)?;
    /// let result = SimilarityEngine::new(SimilarityPolicy::default()).verify(&text, &text, None)?;
    /// assert!(result.qualifies);
    /// assert_eq!(result.text_similarity, 1.0);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn verify(
        &self,
        query: &ShingleSet,
        candidate: &ShingleSet,
        metadata: Option<(&MetadataTokens, &MetadataTokens)>,
    ) -> Result<MatchScore, IncompatibleProfile> {
        let text_similarity = match query.short_text_similarity(candidate)? {
            Some(score) => score,
            None => jaccard(&query.shingles(), &candidate.shingles()).unwrap_or(0.0),
        };
        let metadata_similarity = metadata.and_then(|(left, right)| jaccard(&left.0, &right.0));
        let score = match self.policy.text_weight {
            None => text_similarity,
            Some(weight) => {
                weight * text_similarity + (1.0 - weight) * metadata_similarity.unwrap_or(0.0)
            }
        };
        Ok(MatchScore {
            score,
            text_similarity,
            metadata_similarity,
            qualifies: score > self.policy.threshold,
        })
    }
}

fn jaccard<T: Ord>(left: &BTreeSet<T>, right: &BTreeSet<T>) -> Option<f64> {
    let intersection = left.intersection(right).count();
    let union = left.len() + right.len() - intersection;
    (union != 0).then(|| intersection as f64 / union as f64)
}
