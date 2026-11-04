//! Bounded, structured diagnostics for Whorl executables.
//!
//! Call [`init`] after loading the local environment and retain its guard until
//! the runtime has stopped. Libraries emit `tracing` events without installing
//! a subscriber. `RUST_LOG` controls filtering; the default is `info`.

use std::io;
use tracing_appender::non_blocking::{ErrorCounter, NonBlockingBuilder, WorkerGuard};
use tracing_subscriber::EnvFilter;

/// Maximum queued log lines before new diagnostics are dropped rather than blocking.
///
/// Events have fixed fields without document content. This bound limits retained
/// diagnostics when a collector stalls; it is independent of processing admission.
const BUFFERED_LINES: usize = 1024;

/// Startup could not configure or install structured diagnostics.
#[derive(Debug, thiserror::Error)]
pub enum LoggingError {
    /// `RUST_LOG` contains an invalid filter directive.
    #[error(transparent)]
    Filter(#[from] tracing_subscriber::filter::ParseError),
    /// Another global subscriber is already installed.
    #[error(transparent)]
    Installed(#[from] tracing::subscriber::SetGlobalDefaultError),
}

/// Keeps the log writer alive and exposes its dropped-line count.
///
/// Drop only after application work stops. The appender makes a bounded attempt
/// to flush at shutdown; a stalled sink must not prevent process termination.
/// Saturation drops new lines instead of blocking application threads.
#[must_use]
pub struct Logging {
    _guard: WorkerGuard,
    dropped: ErrorCounter,
}

impl std::fmt::Debug for Logging {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Logging")
            .field("dropped_lines", &self.dropped_lines())
            .finish_non_exhaustive()
    }
}

impl Logging {
    /// Counts log lines dropped because the bounded writer queue was full.
    /// # Returns
    /// A process-local cumulative count since initialization. Sink write failures
    /// are not counted by this queue-saturation counter.
    /// # Examples
    /// ```no_run
    /// let logging = whorl::logging::init()?;
    /// assert_eq!(logging.dropped_lines(), 0);
    /// # Ok::<(), whorl::logging::LoggingError>(())
    /// ```
    #[must_use]
    pub fn dropped_lines(&self) -> usize {
        self.dropped.dropped_lines()
    }
}

/// Installs JSON diagnostics with filtering and a bounded off-thread stderr writer.
///
/// # Returns
/// A guard to retain until after runtime shutdown. Missing `RUST_LOG` selects
/// `info`; valid directives may select individual targets or disable output.
/// JSON event fields remain at the top level alongside level/target metadata.
/// Lines are dropped on saturation rather than blocking callers.
/// # Errors
/// Returns [`LoggingError::Filter`] for invalid filter syntax or
/// [`LoggingError::Installed`] if a subscriber was already installed.
/// # Panics
/// Panics if the appender cannot create its dedicated writer thread.
/// # Examples
/// ```no_run
/// let logging = whorl::logging::init()?;
/// # Ok::<(), whorl::logging::LoggingError>(())
/// ```
pub fn init() -> Result<Logging, LoggingError> {
    let filter = EnvFilter::try_new(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()))?;
    let (subscriber, logging) = subscriber(io::stderr(), filter, BUFFERED_LINES);
    tracing::dispatcher::set_global_default(subscriber)?;
    Ok(logging)
}

fn subscriber(
    writer: impl io::Write + Send + 'static,
    filter: EnvFilter,
    capacity: usize,
) -> (tracing::Dispatch, Logging) {
    let (writer, guard) = NonBlockingBuilder::default()
        .buffered_lines_limit(capacity)
        .lossy(true)
        .finish(writer);
    let dropped = writer.error_counter();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .flatten_event(true)
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .log_internal_errors(false)
        .finish();
    (
        tracing::Dispatch::new(subscriber),
        Logging {
            _guard: guard,
            dropped,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::Duration;

    struct BlockedWriter {
        entered: mpsc::SyncSender<()>,
        release: mpsc::Receiver<()>,
    }

    impl io::Write for BlockedWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let _ = self.entered.send(());
            let _ = self.release.recv();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn blocked_sink_drops_lines_without_blocking_producers() {
        let (entered, writing) = mpsc::sync_channel(1);
        let (release, blocked) = mpsc::channel();
        let (dispatch, logging) = subscriber(
            BlockedWriter {
                entered,
                release: blocked,
            },
            EnvFilter::new("info"),
            2,
        );
        tracing::dispatcher::with_default(&dispatch, || {
            tracing::info!(event = "first", "first event");
        });
        writing.recv_timeout(Duration::from_secs(2)).unwrap();
        let (finished, done) = mpsc::channel();
        let producer = std::thread::spawn(move || {
            tracing::dispatcher::with_default(&dispatch, || {
                for _ in 0..100 {
                    tracing::info!(event = "bounded", "bounded event");
                }
                let _timer = crate::telemetry::StageMetrics::start("drop_test");
            });
            finished.send(()).unwrap();
        });
        let completed = done.recv_timeout(Duration::from_secs(2));
        drop(release);
        producer.join().unwrap();
        completed.unwrap();
        assert!(logging.dropped_lines() > 0);
    }

    #[test]
    fn shutdown_does_not_wait_forever_for_a_stalled_sink() {
        let (entered, writing) = mpsc::sync_channel(1);
        let (release, blocked) = mpsc::channel();
        let (dispatch, logging) = subscriber(
            BlockedWriter {
                entered,
                release: blocked,
            },
            EnvFilter::new("info"),
            2,
        );
        tracing::dispatcher::with_default(&dispatch, || {
            tracing::info!(event = "stalled", "stalled sink");
        });
        writing.recv_timeout(Duration::from_secs(2)).unwrap();
        let (finished, done) = mpsc::channel();
        let shutdown = std::thread::spawn(move || {
            drop(dispatch);
            drop(logging);
            finished.send(()).unwrap();
        });
        let completed = done.recv_timeout(Duration::from_secs(3));
        drop(release);
        shutdown.join().unwrap();
        completed.unwrap();
    }

    struct FailedWriter;
    impl io::Write for FailedWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("sink failed"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("sink failed"))
        }
    }

    #[test]
    fn failed_sink_does_not_panic_during_unwinding() {
        let (dispatch, _logging) = subscriber(FailedWriter, EnvFilter::new("info"), 2);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tracing::dispatcher::with_default(&dispatch, || {
                let _timer = crate::telemetry::StageMetrics::start("unwind_test");
                panic!("expected work failure");
            });
        }));
        assert!(result.is_err());
    }

    #[derive(Clone)]
    struct Captured(Arc<Mutex<Vec<u8>>>);
    impl io::Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn json_fields_and_filtering_survive_writer_handoff() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let (dispatch, logging) =
            subscriber(Captured(Arc::clone(&captured)), EnvFilter::new("info"), 8);
        tracing::dispatcher::with_default(&dispatch, || {
            tracing::debug!(event = "disabled", "not emitted");
            tracing::info!(event = "enabled", count = 3, "emitted event");
        });
        drop(dispatch);
        drop(logging);
        let output = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
        let rows: Vec<_> = output
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["event"], "enabled");
        assert_eq!(rows[0]["count"], 3);
    }
}
