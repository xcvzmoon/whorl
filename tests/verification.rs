use std::num::NonZeroUsize;

use serde_json::json;
use whorl::similarity::{
    normalize::NormalizationVersion,
    shingles::{ShingleSet, SourceProfile},
    verify::{MetadataTokens, SimilarityEngine, SimilarityPolicy},
};

fn text(input: &str) -> ShingleSet {
    ShingleSet::new(
        input,
        NormalizationVersion::V1,
        SourceProfile::NativeTextV1,
        None,
    )
    .unwrap()
}

#[test]
fn exact_threshold_is_exclusive() {
    let left = text("a b c d e");
    let right = text("a b c d e f");
    let score = SimilarityEngine::new(SimilarityPolicy::default())
        .verify(&left, &right, None)
        .unwrap();
    assert_eq!(score.text_similarity, 0.75);
    assert!(!score.qualifies);
    assert!(
        SimilarityEngine::new(SimilarityPolicy::new(0.749, None).unwrap())
            .verify(&left, &right, None)
            .unwrap()
            .qualifies
    );
}

#[test]
fn validates_and_snapshots_policy() {
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.01, 1.01] {
        assert!(SimilarityPolicy::new(invalid, None).is_err());
        assert!(SimilarityPolicy::new(0.75, Some(invalid)).is_err());
    }
    for valid in [0.0, 0.75, 1.0] {
        assert!(SimilarityPolicy::new(valid, Some(valid)).is_ok());
    }
    let mut policy = SimilarityPolicy::default();
    let engine = SimilarityEngine::new(policy);
    policy = SimilarityPolicy::new(1.0, None).unwrap();
    let input = text("a b c");
    assert!(engine.verify(&input, &input, None).unwrap().qualifies);
    assert!(
        !SimilarityEngine::new(policy)
            .verify(&input, &input, None)
            .unwrap()
            .qualifies
    );
}

#[test]
fn metadata_tokens_are_deterministic_typed_and_unambiguous() {
    let left = MetadataTokens::new(&json!({
        "author": {"name": "Ada"}, "tags": ["rust", "math", "rust", null],
        "Created_At": "yesterday", "nested": {"SIZE": {"ignored": true}},
        "empty": [], "null": null, "number": 1, "bool": true,
        "a/b": "slash", "a~b": "tilde", "a=b": "equals"
    }));
    let right = MetadataTokens::new(&json!({
        "tags": ["math", "rust"], "author": {"name": "Ada"},
        "number": 1, "bool": true, "a/b": "slash", "a~b": "tilde", "a=b": "equals"
    }));
    assert_eq!(left, right);
    assert_eq!(
        left.iter().collect::<Vec<_>>(),
        [
            "/author/name=\"Ada\"",
            "/a~0b=\"tilde\"",
            "/a~1b=\"slash\"",
            "/a~2b=\"equals\"",
            "/bool=true",
            "/number=1",
            "/tags=\"math\"",
            "/tags=\"rust\""
        ]
    );
    assert_ne!(
        MetadataTokens::new(&json!({"n": 1})),
        MetadataTokens::new(&json!({"n": "1"}))
    );
    assert_ne!(
        MetadataTokens::new(&json!({"a/b": 1})),
        MetadataTokens::new(&json!({"a": {"b": 1}}))
    );
}

#[test]
fn metadata_empty_and_disabled_semantics() {
    let engine = SimilarityEngine::new(SimilarityPolicy::default());
    let input = text("a b c");
    let empty = MetadataTokens::new(&json!({"created": 1, "modified": 2, "creation_date": 3,
        "modification_date": 4, "modified_at": 5, "file_size": 6, "size_bytes": 7}));
    assert_eq!(empty, MetadataTokens::default());
    let populated = MetadataTokens::new(&json!({"author": "Ada"}));
    assert_eq!(
        engine
            .verify(&input, &input, None)
            .unwrap()
            .metadata_similarity,
        None
    );
    assert_eq!(
        engine
            .verify(&input, &input, Some((&empty, &empty)))
            .unwrap()
            .metadata_similarity,
        None
    );
    assert_eq!(
        engine
            .verify(&input, &input, Some((&empty, &populated)))
            .unwrap()
            .metadata_similarity,
        Some(0.0)
    );
    assert_eq!(
        engine
            .verify(&input, &input, Some((&populated, &populated)))
            .unwrap()
            .metadata_similarity,
        Some(1.0)
    );
}

#[test]
fn weighted_policy_has_no_text_gate_and_undefined_metadata_adds_zero() {
    let left = text("one");
    let right = text("two");
    let metadata = MetadataTokens::new(&json!({"author": "Ada"}));
    let engine = SimilarityEngine::new(SimilarityPolicy::new(0.75, Some(0.2)).unwrap());
    let score = engine
        .verify(&left, &right, Some((&metadata, &metadata)))
        .unwrap();
    assert_eq!(score.text_similarity, 0.0);
    assert_eq!(score.score, 0.8);
    assert!(score.qualifies);
    assert!(
        !SimilarityEngine::new(SimilarityPolicy::default())
            .verify(&left, &right, Some((&metadata, &metadata)))
            .unwrap()
            .qualifies
    );
    let empty = MetadataTokens::default();
    for pair in [None, Some((&empty, &empty))] {
        let score = engine.verify(&left, &left, pair).unwrap();
        assert_eq!(score.score, 0.2);
        assert_eq!(score.metadata_similarity, None);
        assert!(!score.qualifies);
    }
    for weight in [0.0, 1.0] {
        let engine = SimilarityEngine::new(SimilarityPolicy::new(0.5, Some(weight)).unwrap());
        assert_eq!(
            engine
                .verify(&left, &right, Some((&metadata, &metadata)))
                .unwrap()
                .score,
            1.0 - weight
        );
    }
}

#[test]
fn incompatible_profiles_fail_for_short_and_long_inputs() {
    for input in ["one", "one two three four"] {
        let native = text(input);
        let ocr = ShingleSet::new(
            input,
            NormalizationVersion::V1,
            SourceProfile::OcrVerbatimV1,
            None,
        )
        .unwrap();
        let different_k = ShingleSet::new(
            input,
            NormalizationVersion::V1,
            SourceProfile::NativeTextV1,
            NonZeroUsize::new(2),
        )
        .unwrap();
        for candidate in [&ocr, &different_k] {
            assert!(
                SimilarityEngine::new(SimilarityPolicy::default())
                    .verify(&native, candidate, None)
                    .is_err()
            );
        }
    }
}

#[test]
fn exact_scores_obey_set_and_edit_properties() {
    let engine = SimilarityEngine::new(SimilarityPolicy::default());
    for length in 3..40 {
        let words = (0..length)
            .map(|index| format!("word{index}"))
            .collect::<Vec<_>>();
        let original = text(&words.join(" "));
        assert_eq!(
            engine.verify(&original, &original, None).unwrap().score,
            1.0
        );
        let disjoint = text("completely separate vocabulary");
        assert_eq!(
            engine.verify(&original, &disjoint, None).unwrap().score,
            0.0
        );
        let mut previous = 1.0;
        for removed in 1..length {
            let edited = text(&words[..length - removed].join(" "));
            let forward = engine.verify(&original, &edited, None).unwrap().score;
            let reverse = engine.verify(&edited, &original, None).unwrap().score;
            let expected = if length - removed < 3 {
                0.0
            } else {
                f64::from(u32::try_from(length - removed - 2).unwrap())
                    / f64::from(u32::try_from(length - 2).unwrap())
            };
            assert_eq!(forward, expected);
            assert_eq!(forward, reverse);
            assert!(forward <= previous);
            previous = forward;
        }
    }
    let repeated = text("a a a a a");
    assert_eq!(
        engine
            .verify(&repeated, &text("a a a"), None)
            .unwrap()
            .score,
        1.0
    );
    assert_eq!(
        engine
            .verify(&text("hello"), &text("HELLO!"), None)
            .unwrap()
            .score,
        1.0
    );
    assert_eq!(
        engine
            .verify(&text("a b"), &text("a b c"), None)
            .unwrap()
            .score,
        0.0
    );
}

#[test]
fn metadata_uses_exact_jaccard() {
    let left = MetadataTokens::new(&json!({"tags": ["a", "b", "c"]}));
    let right = MetadataTokens::new(&json!({"tags": ["a", "b", "d"]}));
    let text = text("a b c");
    let score = SimilarityEngine::new(SimilarityPolicy::new(0.75, Some(0.5)).unwrap())
        .verify(&text, &text, Some((&left, &right)))
        .unwrap();
    assert_eq!(score.metadata_similarity, Some(0.5));
    assert_eq!(score.score, 0.75);
    assert!(!score.qualifies);
}

#[test]
fn repeated_verification_preserves_unicode_and_duplicate_shingle_scores() {
    let query = text("Café bleu café bleu café vert café bleu");
    let candidates = [
        ("café bleu café bleu café vert café bleu", 1.0),
        ("café bleu café", 0.2),
        ("different words entirely", 0.0),
        ("café", 0.0),
    ];
    let engine = SimilarityEngine::new(SimilarityPolicy::default());
    for _ in 0..5 {
        for (candidate, expected) in candidates {
            let score = engine.verify(&query, &text(candidate), None).unwrap();
            assert_eq!(score.text_similarity, expected);
        }
    }
}
