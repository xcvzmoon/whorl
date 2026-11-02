use std::num::NonZeroUsize;
use whorl::similarity::{
    normalize::NormalizationVersion,
    shingles::{IncompatibleProfile, ShingleSet, SourceProfile},
};

fn input(text: &str, k: Option<NonZeroUsize>) -> ShingleSet {
    ShingleSet::new(
        text,
        NormalizationVersion::V1,
        SourceProfile::NativeTextV1,
        k,
    )
    .unwrap()
}

#[test]
fn shingles_are_unique_word_triples() {
    let text = input("ONE two three one two three", None);
    assert_eq!(
        text.shingles().into_iter().collect::<Vec<_>>(),
        ["one two three", "three one two", "two three one"]
    );
    assert_eq!(input("a a a a", None).shingles().len(), 1);
    assert_eq!(
        input("café हिन्दी العربية", None)
            .shingles()
            .into_iter()
            .collect::<Vec<_>>(),
        ["café हिन्दी العربية"]
    );
}

#[test]
fn short_text_rule_is_symmetric() {
    for (left, right, expected) in [
        ("ONE!", "one", Some(1.0)),
        ("one two", "ONE TWO", Some(1.0)),
        ("one", "two", Some(0.0)),
        ("one two", "two one", Some(0.0)),
        ("one two", "one two three", Some(0.0)),
        ("one two three", "one two three", None),
    ] {
        let left = input(left, None);
        let right = input(right, None);
        assert_eq!(left.short_text_similarity(&right), Ok(expected));
        assert_eq!(right.short_text_similarity(&left), Ok(expected));
    }
    assert!(input("one two", None).shingles().is_empty());
}

#[test]
fn empty_canonical_text_fails() {
    for text in ["", " \n\t", "!💡"] {
        let error = ShingleSet::new(
            text,
            NormalizationVersion::V1,
            SourceProfile::NativeTextV1,
            None,
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "no_extractable_text");
    }
}

#[test]
fn configured_k_changes_boundaries_and_prevents_incompatible_comparison() {
    let two = NonZeroUsize::new(2);
    let text = input("one two", two);
    assert_eq!(text.shingles().into_iter().collect::<Vec<_>>(), ["one two"]);
    assert_eq!(text.short_text_similarity(&text), Ok(None));
    assert_eq!(
        text.short_text_similarity(&input("one two", None)),
        Err(IncompatibleProfile)
    );
    assert_eq!(input("one", NonZeroUsize::new(1)).shingles().len(), 1);
    assert!(
        input("one two", NonZeroUsize::new(usize::MAX))
            .shingles()
            .is_empty()
    );
}

#[test]
fn source_policy_versions_isolate_native_and_ocr_for_all_lengths() {
    for text in ["one", "one two", "one two three four"] {
        let native = input(text, None);
        let ocr = ShingleSet::new(
            text,
            NormalizationVersion::V1,
            SourceProfile::OcrVerbatimV1,
            None,
        )
        .unwrap();
        assert_eq!(native.ensure_compatible(&ocr), Err(IncompatibleProfile));
        assert_eq!(ocr.ensure_compatible(&native), Err(IncompatibleProfile));
        assert_eq!(native.short_text_similarity(&ocr), Err(IncompatibleProfile));
        assert_eq!(ocr.short_text_similarity(&native), Err(IncompatibleProfile));
        assert_eq!(ocr.ensure_compatible(&ocr), Ok(()));
        assert_eq!(
            native.version_key(),
            "nfkc-v1/native-text-v1/word-3/short-equality-v1"
        );
        assert_eq!(
            ocr.version_key(),
            "nfkc-v1/ocr-verbatim-v1/word-3/short-equality-v1"
        );
    }
    assert_eq!(
        input("one two", NonZeroUsize::new(2)).version_key(),
        "nfkc-v1/native-text-v1/word-2/short-equality-v1"
    );
}

#[test]
fn ocr_preserves_single_character_lines_and_uses_word_shingles() {
    let ocr = ShingleSet::new(
        "I\nread WORDS\nI",
        NormalizationVersion::V1,
        SourceProfile::OcrVerbatimV1,
        None,
    )
    .unwrap();
    assert_eq!(
        ocr.shingles().into_iter().collect::<Vec<_>>(),
        ["i read words", "read words i"]
    );
    let same = ShingleSet::new(
        "i read words i",
        NormalizationVersion::V1,
        SourceProfile::OcrVerbatimV1,
        None,
    )
    .unwrap();
    assert_eq!(ocr.version_key(), same.version_key());
    assert_eq!(ocr.shingles(), same.shingles());
}
