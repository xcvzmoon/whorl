use std::num::NonZeroUsize;
use whorl::similarity::{
    normalize::NormalizationVersion,
    shingles::{ShingleSet, SourceProfile},
    signature::MinHashSignature,
};

fn input(text: &str, k: usize, source: SourceProfile) -> ShingleSet {
    ShingleSet::new(text, NormalizationVersion::V1, source, NonZeroUsize::new(k)).unwrap()
}

fn sign(text: &str, k: usize) -> MinHashSignature {
    MinHashSignature::new(&input(text, k, SourceProfile::NativeTextV1)).unwrap()
}

#[test]
fn persisted_golden_vectors() {
    for (text, k, expected) in [
        (
            "  CAFÉ! Ｆｉｌｅ one two three four café file ",
            3,
            include_str!("vectors/native-k3.txt"),
        ),
        (
            "café file one two three four café file",
            1,
            include_str!("vectors/native-k1.txt"),
        ),
        (
            "one two three",
            3,
            include_str!("vectors/single-shingle.txt"),
        ),
    ] {
        let signature = sign(text, k);
        assert_eq!(
            format!(
                "{}\n{:?}\n{:?}\n",
                signature.version().as_str(),
                signature.values(),
                signature.bands().values()
            ),
            expected
        );
    }
}

#[test]
fn normalization_sets_and_policy_identity() {
    assert_eq!(
        sign("  CAFÉ! Ｆｉｌｅ one two three four café file ", 3),
        sign("café file one two three four café file", 3)
    );
    assert_eq!(sign("one two one two", 1), sign("two one", 1));
    assert_ne!(
        sign("one two three", 1).version(),
        sign("one two three", 3).version()
    );
    let native = sign("one two three", 3);
    let ocr =
        MinHashSignature::new(&input("one two three", 3, SourceProfile::OcrVerbatimV1)).unwrap();
    assert_ne!(native.version(), ocr.version());
    assert_eq!(native.values(), ocr.values());
    for text in ["one", "one two"] {
        assert!(MinHashSignature::new(&input(text, 3, SourceProfile::NativeTextV1)).is_none());
    }
    let single = sign("one two three", 3);
    assert!(
        single
            .bands()
            .values()
            .windows(2)
            .all(|pair| pair[0] != pair[1])
    );
    for value in single.values().iter().chain(single.bands().values()) {
        let stored = i32::from_le_bytes(value.to_le_bytes());
        assert_eq!(u32::from_le_bytes(stored.to_le_bytes()), *value);
    }
}

#[test]
fn seeded_known_jaccard_retrieval_tracks_theory() {
    let trials = 2_000;
    for (shared, unique) in [(42, 7), (18, 21)] {
        let similarity = shared as f64 / (shared + unique * 2) as f64;
        let expected = 1.0 - (1.0 - similarity.powi(5)).powi(25);
        let mut hits = 0;
        let mut equal_rows = 0;
        for trial in 0..trials {
            let words = |side: &str| {
                (0..shared)
                    .map(|i| format!("seed{trial}common{i}"))
                    .chain((0..unique).map(|i| format!("seed{trial}{side}{i}")))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let left = input(&words("left"), 1, SourceProfile::NativeTextV1);
            let right = input(&words("right"), 1, SourceProfile::NativeTextV1);
            let a = left.shingles();
            let b = right.shingles();
            assert_eq!(a.intersection(&b).count(), shared);
            assert_eq!(a.union(&b).count(), shared + unique * 2);
            let left = MinHashSignature::new(&left).unwrap();
            let right = MinHashSignature::new(&right).unwrap();
            hits += usize::from(
                left.bands()
                    .values()
                    .iter()
                    .zip(right.bands().values())
                    .any(|(a, b)| a == b),
            );
            equal_rows += left
                .values()
                .iter()
                .zip(right.values())
                .filter(|(a, b)| a == b)
                .count();
        }
        let observed = hits as f64 / trials as f64;
        let tolerance = 6.0 * (expected * (1.0 - expected) / trials as f64).sqrt() + 0.005;
        assert!(
            (observed - expected).abs() < tolerance,
            "s={similarity}, observed={observed}, theory={expected}, tolerance={tolerance}"
        );
        let row_rate = equal_rows as f64 / (trials * 125) as f64;
        assert!((row_rate - similarity).abs() < 0.01);
        println!(
            "Jaccard {similarity}: {hits}/{trials} retrieved, theory {expected:.8}, row agreement {row_rate:.6}"
        );
    }
}
