//! Canonical Unicode text for content identity and similarity.

use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

/// Identifies the canonicalization rules included in a signature version.
///
/// Ingest and check must select the same variant. Any change to these rules or
/// their Unicode tables requires a new normalization and signature version.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NormalizationVersion {
    /// NFKC, scalar lowercase, retained letters/numbers/marks, and folded whitespace.
    ///
    /// Uses Unicode 18.0.0 normalization/mark tables and Unicode 17.0.0
    /// lowercase/alphanumeric/whitespace tables. Punctuation, symbols, and non-whitespace controls
    /// are deleted. Accents and repeated lines remain; casing is locale independent.
    V1,
}

impl NormalizationVersion {
    /// Produces canonical text using this version's rules.
    ///
    /// V1 applies NFKC, lowercases each scalar without locale or word context,
    /// and applies NFKC again. It retains Unicode alphanumerics and combining
    /// marks, deletes other non-whitespace characters, and folds Unicode
    /// whitespace to single ASCII spaces without leading or trailing spaces.
    /// A final NFKC pass composes sequences joined by character deletion.
    /// Punctuation deletion can join words: `co-operate` becomes `cooperate`.
    /// No page-header removal or line-break dehyphenation is performed.
    /// Run this CPU work outside async executor threads.
    ///
    /// # Arguments
    ///
    /// * `text` - Extracted text, with the same rules for ingest and check.
    ///
    /// # Returns
    ///
    /// Canonical text. Empty or entirely discarded input returns an empty string;
    /// the extraction pipeline must reject that state before creating shingles.
    ///
    /// # Examples
    ///
    /// ```
    /// use whorl::similarity::normalize::NormalizationVersion;
    ///
    /// let text = NormalizationVersion::V1.normalize("  CAFÉ\nＦｉｌｅ! ");
    /// assert_eq!(text, "café file");
    /// ```
    pub fn normalize(self, text: &str) -> String {
        match self {
            Self::V1 => {
                let mut normalized = String::with_capacity(text.len());
                let mut space_pending = false;
                for character in text.nfkc().flat_map(char::to_lowercase).nfkc() {
                    if character.is_whitespace() {
                        space_pending = !normalized.is_empty();
                    } else if character.is_alphanumeric() || is_combining_mark(character) {
                        if space_pending {
                            normalized.push(' ');
                            space_pending = false;
                        }
                        normalized.push(character);
                    }
                }
                normalized.nfkc().collect()
            }
        }
    }
}
