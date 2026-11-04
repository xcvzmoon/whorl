use std::{hint::black_box, time::Instant};
use whorl::similarity::{
    normalize::NormalizationVersion,
    shingles::{ShingleSet, SourceProfile},
    verify::{SimilarityEngine, SimilarityPolicy},
};

fn prepared(text: &str) -> ShingleSet {
    ShingleSet::new(
        text,
        NormalizationVersion::V1,
        SourceProfile::NativeTextV1,
        None,
    )
    .unwrap()
}

#[expect(
    clippy::format_collect,
    reason = "Document generation is benchmark setup outside the measured operations"
)]
fn main() {
    let query_text = (0..1_000)
        .map(|word| format!("word{word} "))
        .collect::<String>();
    let query = prepared(&query_text);
    let engine = SimilarityEngine::new(SimilarityPolicy::default());
    for count in [32, 1_000] {
        let text = (0..count)
            .map(|candidate| {
                (0..1_000)
                    .map(|word| {
                        if word < 850 {
                            format!("word{word} ")
                        } else {
                            format!("candidate{candidate}word{word} ")
                        }
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        let candidates = text.iter().map(|text| prepared(text)).collect::<Vec<_>>();
        let mut verify = Vec::new();
        let mut prepare_verify = Vec::new();
        for _ in 0..5 {
            let start = Instant::now();
            for candidate in &candidates {
                black_box(
                    engine
                        .verify(black_box(&query), black_box(candidate), None)
                        .unwrap(),
                );
            }
            verify.push(start.elapsed().as_micros());
            let start = Instant::now();
            for text in &text {
                let candidate = prepared(black_box(text));
                black_box(engine.verify(black_box(&query), &candidate, None).unwrap());
            }
            prepare_verify.push(start.elapsed().as_micros());
        }
        verify.sort_unstable();
        prepare_verify.sort_unstable();
        println!(
            "words=1000 candidates={count} median_verification_us={} median_candidate_prepare_verify_us={} repetitions=5",
            verify[2], prepare_verify[2]
        );
    }
}
