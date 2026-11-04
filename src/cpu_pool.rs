//! Shared, bounded execution for extraction, signing and matching.

use std::{
    num::NonZeroUsize,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{Semaphore, oneshot, watch};

const RUNNING: u8 = 0;
const STOPPING: u8 = 1;
const POISONED: u8 = 2;

/// Admission and execution failures for CPU work.
#[derive(Debug, thiserror::Error)]
pub enum CpuError {
    /// Worker count exceeds two or available parallelism, or timing arithmetic is invalid.
    #[error("invalid CPU pool configuration")]
    InvalidConfiguration,
    /// Dedicated worker threads could not start.
    #[error(transparent)]
    Start(#[from] rayon::ThreadPoolBuildError),
    /// All running and waiting slots are occupied (`overloaded`).
    #[error("overloaded")]
    Overloaded,
    /// Shutdown has started (`cpu_shutdown`).
    #[error("cpu_shutdown")]
    Shutdown,
    /// The deadline elapsed before result delivery (`cpu_expired`).
    #[error("cpu_expired")]
    Expired,
    /// The work panicked (`cpu_panicked`).
    #[error("cpu_panicked")]
    Panicked,
    /// The worker exited without delivering a result (`cpu_delivery`).
    #[error("cpu_delivery")]
    Delivery,
}

/// Absolute deadline shared by every stage of one file.
#[derive(Clone, Copy, Debug)]
pub struct WorkDeadline(Instant);

impl WorkDeadline {
    /// Sets an absolute monotonic deadline.
    ///
    /// # Arguments
    /// * `deadline` - Latest permitted completion time; past times reject work.
    /// # Returns
    /// The deadline, without starting any work.
    /// # Examples
    /// ```
    /// let deadline = whorl::cpu_pool::WorkDeadline::new(std::time::Instant::now());
    /// assert!(deadline.is_expired());
    /// ```
    pub fn new(deadline: Instant) -> Self {
        Self(deadline)
    }

    /// Tests whether the deadline has elapsed.
    ///
    /// # Returns
    /// `true` once the monotonic deadline is reached.
    /// # Examples
    /// ```
    /// let deadline = whorl::cpu_pool::WorkDeadline::new(std::time::Instant::now());
    /// assert!(deadline.is_expired());
    /// ```
    pub fn is_expired(self) -> bool {
        Instant::now() >= self.0
    }
}

/// Cooperative stop signal for running Rust work.
///
/// Poll between bounded chunks. Running threads cannot be killed safely.
#[derive(Clone)]
pub struct WorkContext {
    deadline: WorkDeadline,
    cancelled: Arc<AtomicBool>,
    state: Arc<AtomicU8>,
}

impl WorkContext {
    /// Checks deadline, caller cancellation and pool shutdown.
    ///
    /// # Returns
    /// `true` when work should release its resources and return promptly.
    /// # Examples
    /// ```no_run
    /// # async fn example(pool: &whorl::cpu_pool::CpuPool, deadline: whorl::cpu_pool::WorkDeadline) -> Result<(), whorl::cpu_pool::CpuError> {
    /// pool.run(deadline, |context| { if context.should_stop() { return; } }).await?;
    /// # Ok(()) }
    /// ```
    pub fn should_stop(&self) -> bool {
        self.deadline.is_expired()
            || self.cancelled.load(Ordering::Acquire)
            || self.state.load(Ordering::Acquire) != RUNNING
    }
}

struct CancelOnDrop(Arc<AtomicBool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

/// Fixed dedicated workers with a measured, bounded waiting budget.
///
/// Construct once per process and share through [`Arc`]. All pipeline stages must
/// use this instance. Dropping the last handle stops admission; submitted work
/// keeps its memory and slot until it exits. Use [`Self::shutdown`] to drain it.
pub struct CpuPool {
    pool: rayon::ThreadPool,
    slots: Arc<Semaphore>,
    capacity: u32,
    state: Arc<AtomicU8>,
    panicked: watch::Sender<bool>,
}

impl std::fmt::Debug for CpuPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CpuPool")
            .field("capacity", &self.capacity)
            .field("shutdown", &(self.state.load(Ordering::Acquire) != RUNNING))
            .field("panicked", &self.has_failed())
            .finish_non_exhaustive()
    }
}

impl CpuPool {
    /// Starts dedicated workers and calculates the waiting budget.
    ///
    /// # Arguments
    /// * `workers` - One or two, no greater than available parallelism.
    /// * `acceptable_wait` - Waiting budget; zero disables waiting slots.
    /// * `typical_job_time` - Measured nonzero duration for the admitted work unit.
    /// # Returns
    /// A pool with `floor(workers * acceptable_wait / typical_job_time)` waiting
    /// slots in addition to its running slots. No CPU work uses Tokio's blocking pool.
    /// # Errors
    /// [`CpuError::InvalidConfiguration`] for unsupported workers, zero job time,
    /// or capacity overflow; [`CpuError::Start`] for thread creation failure.
    /// # Examples
    /// ```
    /// use std::{num::NonZeroUsize, time::Duration};
    /// let pool = whorl::cpu_pool::CpuPool::new(NonZeroUsize::new(1).unwrap(), Duration::ZERO, Duration::from_millis(10))?;
    /// # Ok::<(), whorl::cpu_pool::CpuError>(())
    /// ```
    pub fn new(
        workers: NonZeroUsize,
        acceptable_wait: Duration,
        typical_job_time: Duration,
    ) -> Result<Self, CpuError> {
        let workers = workers.get();
        if workers > 2
            || workers > std::thread::available_parallelism().map_or(1, NonZeroUsize::get)
            || typical_job_time.is_zero()
        {
            return Err(CpuError::InvalidConfiguration);
        }
        let waiting = acceptable_wait
            .as_nanos()
            .checked_mul(workers as u128)
            .ok_or(CpuError::InvalidConfiguration)?
            / typical_job_time.as_nanos();
        let capacity = waiting
            .checked_add(workers as u128)
            .and_then(|n| u32::try_from(n).ok())
            .filter(|&n| n as usize <= Semaphore::MAX_PERMITS)
            .ok_or(CpuError::InvalidConfiguration)?;
        Ok(Self {
            pool: rayon::ThreadPoolBuilder::new()
                .num_threads(workers)
                .thread_name(|i| format!("cpu-{i}"))
                .build()?,
            slots: Arc::new(Semaphore::new(capacity as usize)),
            capacity,
            state: Arc::new(AtomicU8::new(RUNNING)),
            panicked: watch::channel(false).0,
        })
    }

    /// Submits owned CPU work without waiting for admission.
    ///
    /// # Arguments
    /// * `deadline` - Absolute deadline, including time spent waiting.
    /// * `work` - Owned operation; poll its context to stop running work cooperatively.
    /// # Returns
    /// The operation's result through a oneshot channel. Dropping this future
    /// cancels queued work and signals running work; its slot remains held until exit.
    /// # Errors
    /// [`CpuError::Overloaded`] when full, [`CpuError::Shutdown`] after shutdown,
    /// [`CpuError::Expired`] before starting or delivering expired work,
    /// [`CpuError::Panicked`] for a caught work panic, or [`CpuError::Delivery`]
    /// if delivery fails. A caught panic permanently stops admission and signals
    /// [`Self::failed`]; the host must stop the process after draining unrelated
    /// work. Panics configured to abort cannot be caught.
    /// # Examples
    /// ```no_run
    /// # async fn example(pool: &whorl::cpu_pool::CpuPool, deadline: whorl::cpu_pool::WorkDeadline) -> Result<(), whorl::cpu_pool::CpuError> {
    /// assert_eq!(pool.run(deadline, |_| 2 + 2).await?, 4);
    /// # Ok(()) }
    /// ```
    pub async fn run<T: Send + 'static>(
        &self,
        deadline: WorkDeadline,
        work: impl FnOnce(WorkContext) -> T + Send + 'static,
    ) -> Result<T, CpuError> {
        if self.has_failed() {
            return Err(CpuError::Panicked);
        }
        if self.state.load(Ordering::Acquire) != RUNNING {
            return Err(CpuError::Shutdown);
        }
        if deadline.is_expired() {
            return Err(CpuError::Expired);
        }
        let permit = Arc::clone(&self.slots).try_acquire_owned().map_err(|_| {
            tracing::warn!(
                event = "cpu_overloaded",
                capacity = self.capacity,
                "CPU admission is full"
            );
            CpuError::Overloaded
        })?;
        tracing::debug!(
            event = "cpu_admitted",
            occupied = self.capacity as usize - self.slots.available_permits(),
            capacity = self.capacity,
            "CPU work admitted"
        );
        if self.state.load(Ordering::Acquire) != RUNNING {
            return Err(CpuError::Shutdown);
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let _cancel = CancelOnDrop(cancelled.clone());
        let context = WorkContext {
            deadline,
            cancelled,
            state: Arc::clone(&self.state),
        };
        let (send, receive) = oneshot::channel();
        let panicked = self.panicked.clone();
        let state = Arc::clone(&self.state);
        self.pool.spawn(move || {
            if send.is_closed() {
                drop(work);
                return;
            }
            let result = if context.state.load(Ordering::Acquire) != RUNNING {
                drop(work);
                Err(CpuError::Shutdown)
            } else if deadline.is_expired() {
                drop(work);
                Err(CpuError::Expired)
            } else {
                match catch_unwind(AssertUnwindSafe(|| work(context))) {
                    Ok(value) if !deadline.is_expired() => Ok(value),
                    Ok(_) => Err(CpuError::Expired),
                    Err(_) => {
                        state.store(POISONED, Ordering::Release);
                        panicked.send_replace(true);
                        Err(CpuError::Panicked)
                    }
                }
            };
            drop(permit);
            let _ = send.send(result);
        });
        receive.await.map_err(|_| CpuError::Delivery)?
    }

    /// Waits until a submitted operation panics and permanently stops admission.
    ///
    /// # Returns
    /// Nothing after the first caught panic, including one before this call.
    /// The host must stop queue admission, drain unrelated work, and exit with
    /// failure so process supervision can restart it. Cancellation only stops
    /// waiting; it does not reset the failure or reopen admission.
    /// # Examples
    /// ```no_run
    /// # async fn example(pool: &whorl::cpu_pool::CpuPool) {
    /// pool.failed().await;
    /// pool.shutdown().await;
    /// # }
    /// ```
    pub async fn failed(&self) {
        let mut receiver = self.panicked.subscribe();
        let _ = receiver.wait_for(|failed| *failed).await;
    }

    /// Reports whether CPU work has panicked.
    /// # Returns
    /// `true` after the first caught panic; failure is permanent for this pool.
    /// # Examples
    /// ```
    /// # use std::{num::NonZeroUsize, time::Duration};
    /// let pool = whorl::cpu_pool::CpuPool::new(NonZeroUsize::MIN, Duration::ZERO, Duration::from_secs(1))?;
    /// assert!(!pool.has_failed());
    /// # Ok::<(), whorl::cpu_pool::CpuError>(())
    /// ```
    pub fn has_failed(&self) -> bool {
        self.state.load(Ordering::Acquire) == POISONED
    }

    /// Stops admission, skips waiting work, and drains submitted operations.
    ///
    /// # Returns
    /// Once every submitted closure has exited and released its resources.
    /// Cancellation of this future leaves admission stopped; calling again drains.
    /// Running work must cooperate. Uninterruptible extraction must run in a
    /// resource-limited subprocess with deadline-driven kill, wait and cleanup.
    /// # Examples
    /// ```no_run
    /// # async fn example(pool: &whorl::cpu_pool::CpuPool) {
    /// pool.shutdown().await;
    /// # }
    /// ```
    pub async fn shutdown(&self) {
        let _ = self
            .state
            .compare_exchange(RUNNING, STOPPING, Ordering::AcqRel, Ordering::Acquire);
        let _drained = self.slots.acquire_many(self.capacity).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn poison_is_visible_before_notification_and_survives_shutdown() {
        let pool = CpuPool::new(NonZeroUsize::MIN, Duration::ZERO, Duration::from_secs(1)).unwrap();
        pool.state.store(POISONED, Ordering::Release);
        assert!(pool.has_failed());
        assert!(matches!(
            pool.run(
                WorkDeadline::new(Instant::now() + Duration::from_secs(1)),
                |_| 42
            )
            .await,
            Err(CpuError::Panicked)
        ));
        pool.shutdown().await;
        assert!(pool.has_failed());
        pool.panicked.send_replace(true);
        pool.failed().await;
    }
}
