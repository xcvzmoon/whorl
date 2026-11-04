use std::{
    num::NonZeroUsize,
    time::{Duration, Instant},
};
use whorl::cpu_pool::{CpuPool, WorkDeadline};

fn main() {
    let logging = whorl::logging::init().unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let pool = CpuPool::new(NonZeroUsize::MIN, Duration::ZERO, Duration::from_secs(1)).unwrap();
    let mut samples = Vec::new();
    runtime.block_on(async {
        for _ in 0..5 {
            let start = Instant::now();
            for _ in 0..2000 {
                let deadline = WorkDeadline::new(Instant::now() + Duration::from_secs(10));
                std::hint::black_box(pool.run(deadline, |_| 42).await.unwrap());
            }
            samples.push(start.elapsed().as_micros());
        }
        pool.shutdown().await;
    });
    samples.sort_unstable();
    println!(
        "cpu_admissions=2000 median_us={} dropped_lines={}",
        samples[2],
        logging.dropped_lines()
    );
}
