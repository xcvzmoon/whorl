//! Deterministic `MinHash` rows and locality-sensitive bands for persisted indexes.
//!
//! V1 hashes each UTF-8 shingle with XXH3-64, using row seeds
//! `0x9e3779b97f4a7c15 * (row + 1)` modulo 2^64, and retains the low 32 bits.
//! Each row stores the minimum hash across the unique shingles. Bands hash a
//! little-endian u32 band index followed by five little-endian u32 rows with
//! XXH3-64 seed `0x6a09e667f3bcc909`, retaining the low 32 bits.
//! These hashes are non-cryptographic; exact verification remains mandatory.

use xxhash_rust::xxh3::xxh3_64_with_seed;

use super::shingles::{ShingleSet, SourceProfile};
use serde_json::{Value, json};
use std::{num::NonZeroUsize, str::FromStr};

/// Identifies every processing and hashing rule for a persisted index.
///
/// Parse persisted keys with [`str::parse`]. Unsupported algorithms, sources,
/// zero lengths, and noncanonical spellings return [`InvalidSignatureVersion`].
/// Equality is required before comparing signatures or retrieving bands. V1
/// includes the input's normalization, source, k, and short-equality policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignatureVersion {
    key: String,
    source: SourceProfile,
    k: NonZeroUsize,
}

/// An unsupported or noncanonical signature version (`invalid_signature_version`).
#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("invalid_signature_version")]
pub struct InvalidSignatureVersion;

impl FromStr for SignatureVersion {
    type Err = InvalidSignatureVersion;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let parts: Vec<_> = value.split('/').collect();
        let [
            "nfkc-v1",
            source,
            words,
            "short-equality-v1",
            "xxh3-low32-le-v1",
            "125",
            "25x5",
        ] = parts.as_slice()
        else {
            return Err(InvalidSignatureVersion);
        };
        let source = match *source {
            "native-text-v1" => SourceProfile::NativeTextV1,
            "ocr-verbatim-v1" => SourceProfile::OcrVerbatimV1,
            _ => return Err(InvalidSignatureVersion),
        };
        let k = words
            .strip_prefix("word-")
            .and_then(|s| s.parse::<NonZeroUsize>().ok())
            .ok_or(InvalidSignatureVersion)?;
        let version = Self::new(source, k);
        if version.as_str() != value {
            return Err(InvalidSignatureVersion);
        }
        Ok(version)
    }
}

impl SignatureVersion {
    /// Builds the supported comparison version for a source and shingle length.
    ///
    /// # Arguments
    /// * `source` - Native or verbatim OCR comparison namespace.
    /// * `k` - Positive word-shingle length; no default is assumed.
    /// # Returns
    /// A canonical version under the current normalization, hash and band rules.
    /// # Examples
    /// ```
    /// use whorl::similarity::{shingles::SourceProfile, signature::SignatureVersion};
    /// let version = SignatureVersion::new(SourceProfile::NativeTextV1, std::num::NonZeroUsize::MIN);
    /// assert!(version.as_str().contains("word-1"));
    /// ```
    pub fn new(source: SourceProfile, k: NonZeroUsize) -> Self {
        let source_key = match source {
            SourceProfile::NativeTextV1 => "native-text-v1",
            SourceProfile::OcrVerbatimV1 => "ocr-verbatim-v1",
        };
        Self {
            key: format!(
                "nfkc-v1/{source_key}/word-{k}/short-equality-v1/xxh3-low32-le-v1/125/25x5"
            ),
            source,
            k,
        }
    }

    /// Returns the word-shingle length encoded in this version.
    /// # Returns
    /// The positive length used for preparation and reindexing.
    /// # Examples
    /// ```
    /// use whorl::similarity::signature::SignatureVersion;
    /// let version: SignatureVersion = "nfkc-v1/native-text-v1/word-4/short-equality-v1/xxh3-low32-le-v1/125/25x5".parse()?;
    /// assert_eq!(version.k().get(), 4);
    /// # Ok::<(), whorl::similarity::signature::InvalidSignatureVersion>(())
    /// ```
    pub fn k(&self) -> NonZeroUsize {
        self.k
    }

    /// Returns the configuration that must be registered with this version.
    /// # Returns
    /// Source-specific normalization and shingle settings. Callers must validate
    /// each registration against its own version, rather than equating sources.
    /// # Examples
    /// ```
    /// use whorl::similarity::{shingles::SourceProfile, signature::SignatureVersion};
    /// let version = SignatureVersion::new(SourceProfile::OcrVerbatimV1, std::num::NonZeroUsize::MIN);
    /// assert_eq!(version.configuration()["source"], "ocr-verbatim-v1");
    /// ```
    pub fn configuration(&self) -> Value {
        let source = match self.source {
            SourceProfile::NativeTextV1 => "native-text-v1",
            SourceProfile::OcrVerbatimV1 => "ocr-verbatim-v1",
        };
        json!({"normalization":"nfkc-v1", "source":source, "k":self.k.get()})
    }

    pub(crate) fn same_algorithm(&self, other: &Self) -> bool {
        self.k == other.k
    }

    /// Returns the stable ASCII persistence key.
    ///
    /// # Returns
    ///
    /// The full policy key, including XXH3 seed/encoding revision and band layout.
    ///
    /// # Examples
    ///
    /// ```
    /// # use whorl::similarity::{normalize::NormalizationVersion, shingles::{ShingleSet, SourceProfile}, signature::MinHashSignature};
    /// let input = ShingleSet::new("one two three", NormalizationVersion::V1, SourceProfile::NativeTextV1, None)?;
    /// let signature = MinHashSignature::new(&input).unwrap();
    /// assert!(signature.version().as_str().ends_with("xxh3-low32-le-v1/125/25x5"));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn as_str(&self) -> &str {
        &self.key
    }
}

/// A policy-bound array of 125 deterministic `MinHash` rows.
///
/// Run construction outside async executor threads. It performs 125 hashes per
/// unique shingle and owns only the resulting rows and policy key afterward.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MinHashSignature {
    version: SignatureVersion,
    values: [u32; 125],
}

/// The 25 indexed hashes of five consecutive `MinHash` rows each.
///
/// Values are meaningful only alongside the originating [`SignatureVersion`].
/// PostgreSQL integer arrays preserve each value with
/// `i32::from_le_bytes(value.to_le_bytes())`; reverse with
/// `u32::from_le_bytes(value.to_le_bytes())`. Band collisions only select
/// candidates and never establish a match.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BandHashes([u32; 25]);

impl MinHashSignature {
    /// Signs the unique shingles under the fixed V1 hash rules.
    ///
    /// # Arguments
    ///
    /// * `input` - Canonical input with its normalization, source, and word policy.
    ///
    /// # Returns
    ///
    /// A signature for at least k words. Returns `None` for short text, which
    /// must use content-hash retrieval and exact normalized equality instead.
    ///
    /// # Examples
    ///
    /// ```
    /// # use whorl::similarity::{normalize::NormalizationVersion, shingles::{ShingleSet, SourceProfile}, signature::MinHashSignature};
    /// let input = ShingleSet::new("one", NormalizationVersion::V1, SourceProfile::NativeTextV1, None)?;
    /// assert!(MinHashSignature::new(&input).is_none());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new(input: &ShingleSet) -> Option<Self> {
        let shingles = input.shingles();
        if shingles.is_empty() {
            return None;
        }
        let mut values = [u32::MAX; 125];
        for shingle in shingles {
            for (row, value) in values.iter_mut().enumerate() {
                let seed = 0x9e37_79b9_7f4a_7c15_u64.wrapping_mul(row as u64 + 1);
                *value = (*value).min(xxh3_64_with_seed(shingle.as_bytes(), seed) as u32);
            }
        }
        Some(Self {
            version: SignatureVersion::new(input.source, input.k),
            values,
        })
    }

    /// Returns the policy required for comparisons and index selection.
    ///
    /// # Returns
    ///
    /// The version shared by all rows and derived bands.
    ///
    /// # Examples
    ///
    /// ```
    /// # use whorl::similarity::{normalize::NormalizationVersion, shingles::{ShingleSet, SourceProfile}, signature::MinHashSignature};
    /// let input = ShingleSet::new("one two three", NormalizationVersion::V1, SourceProfile::NativeTextV1, None)?;
    /// let signature = MinHashSignature::new(&input).unwrap();
    /// assert_eq!(signature.version(), signature.clone().version());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn version(&self) -> &SignatureVersion {
        &self.version
    }

    /// Borrows the ordered `MinHash` rows for persistence or candidate ordering.
    ///
    /// # Returns
    ///
    /// Exactly 125 unsigned values. Convert to PostgreSQL integers using the
    /// bit-preserving mapping documented on [`BandHashes`].
    ///
    /// # Examples
    ///
    /// ```
    /// # use whorl::similarity::{normalize::NormalizationVersion, shingles::{ShingleSet, SourceProfile}, signature::MinHashSignature};
    /// let input = ShingleSet::new("one two three", NormalizationVersion::V1, SourceProfile::NativeTextV1, None)?;
    /// assert_eq!(MinHashSignature::new(&input).unwrap().values().len(), 125);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn values(&self) -> &[u32; 125] {
        &self.values
    }

    /// Hashes the 25 consecutive bands, mixing each band's position into its hash.
    ///
    /// # Returns
    ///
    /// Band hashes for candidate retrieval within this signature's version.
    /// The ideal retrieval probability at Jaccard similarity s is
    /// `1 - (1 - s^5)^25`. It is probabilistic, including near the 0.75 threshold.
    ///
    /// # Examples
    ///
    /// ```
    /// # use whorl::similarity::{normalize::NormalizationVersion, shingles::{ShingleSet, SourceProfile}, signature::MinHashSignature};
    /// let input = ShingleSet::new("one two three", NormalizationVersion::V1, SourceProfile::NativeTextV1, None)?;
    /// assert_eq!(MinHashSignature::new(&input).unwrap().bands().values().len(), 25);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn bands(&self) -> BandHashes {
        BandHashes(std::array::from_fn(|band| {
            let mut bytes = [0; 24];
            bytes[..4].copy_from_slice(&(band as u32).to_le_bytes());
            for (row, value) in self.values[band * 5..band * 5 + 5].iter().enumerate() {
                bytes[4 + row * 4..8 + row * 4].copy_from_slice(&value.to_le_bytes());
            }
            xxh3_64_with_seed(&bytes, 0x6a09_e667_f3bc_c909) as u32
        }))
    }
}

impl BandHashes {
    /// Borrows the band hashes in band-index order.
    ///
    /// # Returns
    ///
    /// Exactly 25 unsigned hashes for persistence under the signature's version.
    ///
    /// # Examples
    ///
    /// ```
    /// # use whorl::similarity::{normalize::NormalizationVersion, shingles::{ShingleSet, SourceProfile}, signature::MinHashSignature};
    /// let input = ShingleSet::new("one two three", NormalizationVersion::V1, SourceProfile::NativeTextV1, None)?;
    /// let bands = MinHashSignature::new(&input).unwrap().bands();
    /// assert_eq!(bands.values().len(), 25);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn values(&self) -> &[u32; 25] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::{MinHashSignature, SignatureVersion};

    #[test]
    fn persisted_versions_reject_unsupported_or_noncanonical_rules() {
        use super::SourceProfile;
        use std::num::NonZeroUsize;
        let version =
            SignatureVersion::new(SourceProfile::NativeTextV1, NonZeroUsize::new(4).unwrap());
        assert_eq!(
            version.as_str().parse::<SignatureVersion>().unwrap(),
            version
        );
        for invalid in [
            version.as_str().replace("word-4", "word-0"),
            version.as_str().replace("word-4", "word-04"),
            version.as_str().replace("word-4", "word-+4"),
            version.as_str().replace("nfkc-v1", "nfkc-v2"),
            version.as_str().replace("native-text-v1", "unknown"),
            version.as_str().replace("125/25x5", "100/20x5"),
            format!("{}/extra", version.as_str()),
        ] {
            assert!(invalid.parse::<SignatureVersion>().is_err(), "{invalid}");
        }
    }

    #[test]
    fn identical_band_rows_still_include_band_position() {
        let signature = MinHashSignature {
            version: SignatureVersion::new(
                super::SourceProfile::NativeTextV1,
                std::num::NonZeroUsize::MIN,
            ),
            values: [0; 125],
        };
        let bands = signature.bands();
        let unique: std::collections::BTreeSet<_> = bands.values().iter().collect();
        assert_eq!(unique.len(), 25);
    }
}

/// Persisted signature arrays, including version membership for short text.
///
/// Construction performs CPU work without I/O. Use the same representation for
/// live preparation and reindexing so signed hash encoding stays consistent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureIndex {
    /// Canonical comparison version, even when the text is short.
    pub version: SignatureVersion,
    /// Signed 32-bit hash patterns, or `None` for short text.
    pub values: Option<Vec<i32>>,
    /// Signed band hash patterns, or `None` for short text.
    pub bands: Option<Vec<i32>>,
}

impl SignatureIndex {
    /// Computes the persisted arrays for canonical shingles.
    /// # Arguments
    /// * `input` - Input carrying the normalization, source and shingle policy.
    /// # Returns
    /// A version and paired arrays. Both arrays are absent for short text.
    /// # Examples
    /// ```
    /// use whorl::similarity::{normalize::NormalizationVersion, shingles::{ShingleSet, SourceProfile}, signature::SignatureIndex};
    /// let input = ShingleSet::new("one", NormalizationVersion::V1, SourceProfile::NativeTextV1, None)?;
    /// assert!(SignatureIndex::new(&input).values.is_none());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new(input: &ShingleSet) -> Self {
        let signature = MinHashSignature::new(input);
        let signed = |values: &[u32]| {
            values
                .iter()
                .map(|v| i32::from_ne_bytes(v.to_ne_bytes()))
                .collect()
        };
        Self {
            version: SignatureVersion::new(input.source, input.k),
            values: signature.as_ref().map(|s| signed(s.values())),
            bands: signature.as_ref().map(|s| signed(s.bands().values())),
        }
    }
}
