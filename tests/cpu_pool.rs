use std::{
    num::NonZeroUsize,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};
use whorl::cpu_pool::{CpuError, CpuPool, WorkDeadline};

fn pool(waiting: u64) -> CpuPool {
    CpuPool::new(
        NonZeroUsize::new(1).unwrap(),
        Duration::from_millis(waiting * 10),
        Duration::from_millis(10),
    )
    .unwrap()
}
fn deadline() -> WorkDeadline {
    WorkDeadline::new(Instant::now() + Duration::from_secs(10))
}
fn poll<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}

#[tokio::test]
async fn saturation_expiry_cancellation_and_slot_reuse() {
    let pool = pool(2);
    let (started, ready) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let mut running = Box::pin(pool.run(deadline(), move |_| {
        started.send(()).unwrap();
        wait.recv().unwrap();
        7
    }));
    assert!(poll(running.as_mut()).is_pending());
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    let expired_ran = Arc::new(AtomicBool::new(false));
    let flag = expired_ran.clone();
    let short_deadline = WorkDeadline::new(Instant::now() + Duration::from_millis(30));
    let mut expired = Box::pin(pool.run(short_deadline, move |_| {
        flag.store(true, Ordering::SeqCst);
    }));
    assert!(poll(expired.as_mut()).is_pending());
    let cancelled_ran = Arc::new(AtomicBool::new(false));
    let flag = cancelled_ran.clone();
    let mut cancelled = Box::pin(pool.run(deadline(), move |_| {
        flag.store(true, Ordering::SeqCst);
    }));
    assert!(poll(cancelled.as_mut()).is_pending());
    let start = Instant::now();
    assert!(matches!(
        pool.run(deadline(), |_| ()).await,
        Err(CpuError::Overloaded)
    ));
    assert!(start.elapsed() < Duration::from_millis(100));
    drop(cancelled);
    assert!(matches!(
        pool.run(deadline(), |_| ()).await,
        Err(CpuError::Overloaded)
    ));
    tokio::time::sleep(Duration::from_millis(40)).await;
    release.send(()).unwrap();
    assert_eq!(running.await.unwrap(), 7);
    assert!(matches!(expired.await, Err(CpuError::Expired)));
    pool.shutdown().await;
    assert!(!expired_ran.load(Ordering::SeqCst));
    assert!(!cancelled_ran.load(Ordering::SeqCst));
}

#[tokio::test(flavor = "current_thread")]
async fn running_cancellation_keeps_async_runtime_responsive() {
    let pool = pool(0);
    let (started, ready) = mpsc::channel();
    let stopped = Arc::new(AtomicBool::new(false));
    let flag = stopped.clone();
    let mut work = Box::pin(pool.run(deadline(), move |context| {
        started.send(()).unwrap();
        while !context.should_stop() {
            std::hint::spin_loop();
        }
        flag.store(true, Ordering::Release);
    }));
    assert!(poll(work.as_mut()).is_pending());
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    tokio::time::timeout(
        Duration::from_millis(100),
        tokio::time::sleep(Duration::from_millis(10)),
    )
    .await
    .unwrap();
    drop(work);
    pool.shutdown().await;
    assert!(stopped.load(Ordering::Acquire));
    assert!(matches!(
        pool.run(deadline(), |_| ()).await,
        Err(CpuError::Shutdown)
    ));
}

#[tokio::test]
async fn shutdown_skips_waiting_jobs_and_can_be_resumed() {
    let pool = pool(1);
    let (started, ready) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let mut running = Box::pin(pool.run(deadline(), move |_| {
        started.send(()).unwrap();
        wait.recv().unwrap();
    }));
    assert!(poll(running.as_mut()).is_pending());
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    let mut waiting =
        Box::pin(pool.run(deadline(), |_| panic!("queued job started during shutdown")));
    assert!(poll(waiting.as_mut()).is_pending());
    let mut shutdown = Box::pin(pool.shutdown());
    assert!(poll(shutdown.as_mut()).is_pending());
    drop(shutdown);
    assert!(matches!(
        pool.run(deadline(), |_| ()).await,
        Err(CpuError::Shutdown)
    ));
    release.send(()).unwrap();
    running.await.unwrap();
    assert!(matches!(waiting.await, Err(CpuError::Shutdown)));
    pool.shutdown().await;
}

#[tokio::test]
async fn expired_results_release_slots() {
    let pool = pool(0);
    let expired = WorkDeadline::new(Instant::now());
    assert!(matches!(
        pool.run(expired, |_| panic!("expired job ran")).await,
        Err(CpuError::Expired)
    ));
    let soon = WorkDeadline::new(Instant::now() + Duration::from_millis(10));
    assert!(matches!(
        pool.run(soon, |_| std::thread::sleep(Duration::from_millis(20)))
            .await,
        Err(CpuError::Expired)
    ));
    assert_eq!(pool.run(deadline(), |_| 42).await.unwrap(), 42);
}

#[tokio::test]
async fn shared_pool_caps_pipeline_concurrency() {
    let workers = std::thread::available_parallelism().unwrap().get().min(2);
    let pool = Arc::new(
        CpuPool::new(
            NonZeroUsize::new(workers).unwrap(),
            Duration::from_millis(100),
            Duration::from_millis(10),
        )
        .unwrap(),
    );
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let mut jobs = Vec::new();
    for _ in 0..10 {
        let pool = pool.clone();
        let active = active.clone();
        let peak = peak.clone();
        jobs.push(tokio::spawn(async move {
            pool.run(deadline(), move |_| {
                let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(count, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(10));
                active.fetch_sub(1, Ordering::SeqCst);
            })
            .await
            .unwrap();
        }));
    }
    for job in jobs {
        job.await.unwrap();
    }
    assert!(peak.load(Ordering::SeqCst) <= workers);
    assert_eq!(active.load(Ordering::SeqCst), 0);
}

#[test]
fn validates_worker_and_timing_limits() {
    let one = NonZeroUsize::new(1).unwrap();
    assert!(matches!(
        CpuPool::new(one, Duration::ZERO, Duration::ZERO),
        Err(CpuError::InvalidConfiguration)
    ));
    assert!(matches!(
        CpuPool::new(
            NonZeroUsize::new(3).unwrap(),
            Duration::ZERO,
            Duration::from_secs(1)
        ),
        Err(CpuError::InvalidConfiguration)
    ));
    assert!(matches!(
        CpuPool::new(one, Duration::MAX, Duration::from_nanos(1)),
        Err(CpuError::InvalidConfiguration)
    ));
}

#[tokio::test]
async fn panic_stops_later_admission() {
    let pool = pool(0);
    assert!(matches!(
        pool.run(deadline(), |_| panic!("test panic")).await,
        Err(CpuError::Panicked)
    ));
    assert!(matches!(
        pool.run(deadline(), |_| 42).await,
        Err(CpuError::Panicked | CpuError::Shutdown)
    ));
    tokio::time::timeout(Duration::from_secs(1), pool.failed())
        .await
        .unwrap();
    assert!(pool.has_failed());
    pool.shutdown().await;
}
