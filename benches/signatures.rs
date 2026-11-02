use std::{hint::black_box, time::Instant};
use whorl::similarity::{
    normalize::NormalizationVersion,
    shingles::{ShingleSet, SourceProfile},
    signature::MinHashSignature,
};

fn main() {
    for words in [100, 1_000, 10_000, 100_000] {
        let text = (0..words).map(|i| format!("word{i} ")).collect::<String>();
        let input = ShingleSet::new(
            &text,
            NormalizationVersion::V1,
            SourceProfile::NativeTextV1,
            None,
        )
        .unwrap();
        let start = Instant::now();
        for _ in 0..10 {
            black_box(MinHashSignature::new(black_box(&input)).unwrap().bands());
        }
        println!(
            "{words} words: {:?} per signature + bands (10 iterations)",
            start.elapsed() / 10
        );
    }
}
