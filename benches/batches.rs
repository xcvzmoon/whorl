use std::{hint::black_box, time::Instant};
use whorl::similarity::{
    normalize::NormalizationVersion,
    shingles::{ShingleSet, SourceProfile},
    signature::MinHashSignature,
};

#[expect(
    clippy::format_collect,
    reason = "Document generation is benchmark setup outside the measured operations"
)]
fn main() {
    for words in [100, 1_000, 10_000, 100_000] {
        let text = (0..words).map(|i| format!("word{i} ")).collect::<String>();
        for files in [1, 5] {
            let start = Instant::now();
            for _ in 0..10 {
                for _ in 0..files {
                    let shingles = ShingleSet::new(
                        black_box(&text),
                        NormalizationVersion::V1,
                        SourceProfile::NativeTextV1,
                        None,
                    )
                    .unwrap();
                    black_box(MinHashSignature::new(&shingles).unwrap().bands());
                }
            }
            println!(
                "words={words} files={files} normalization_shingling_signing_us={}",
                start.elapsed().as_micros() / 10
            );
        }
    }
}
