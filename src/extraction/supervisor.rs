use super::{ExtractedDocument, ExtractionError, ExtractionLimits};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Supervisor for isolated extraction workers on Linux.
///
/// Each call creates a private temporary directory and process group. Linux
/// `prlimit` bounds CPU seconds, address space, output files and core dumps.
/// Deployment must additionally enforce the approved aggregate cgroup limits
/// (two CPUs, 4 GiB RAM, no swap) and a 128 MiB temporary filesystem.
/// Debug output omits the trusted executable path.
pub struct Extractor {
    worker: PathBuf,
    limits: ExtractionLimits,
    timeout: Duration,
}

impl std::fmt::Debug for Extractor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Extractor")
            .field("limits", &self.limits)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl Extractor {
    /// Configures the worker executable and per-file resource ceilings.
    ///
    /// # Arguments
    /// * `worker` - Absolute path to the trusted `whorl-extract` executable.
    /// * `limits` - Parsing ceilings; defaults are available through [`ExtractionLimits::default`].
    /// * `timeout` - Wall time allowed for the complete child run; must be nonzero.
    /// # Returns
    /// A supervisor; no process starts until [`Self::extract`] is called.
    /// # Errors
    /// Returns [`ExtractionError::ExtractionFailed`] for a relative worker path,
    /// zero timeout, or an output ceiling too large for bounded serialization.
    /// # Examples
    /// ```
    /// use whorl::extraction::{Extractor, ExtractionLimits};
    /// let extractor = Extractor::new("/usr/local/bin/whorl-extract".into(),
    ///     ExtractionLimits::default(), std::time::Duration::from_secs(60))?;
    /// # Ok::<(), whorl::extraction::ExtractionError>(())
    /// ```
    pub fn new(
        worker: PathBuf,
        limits: ExtractionLimits,
        timeout: Duration,
    ) -> Result<Self, ExtractionError> {
        if !worker.is_absolute() || timeout.is_zero() || limits.max_text_bytes > 8 * 1024 * 1024 {
            return Err(ExtractionError::ExtractionFailed);
        }
        Ok(Self {
            worker,
            limits,
            timeout,
        })
    }

    /// Extracts content using magic and package structure rather than a filename.
    ///
    /// # Arguments
    /// * `input` - Complete uploaded bytes; copied into private temporary storage.
    /// # Returns
    /// Nonempty text, embedded metadata and an OCR provenance flag.
    /// # Errors
    /// Returns [`ExtractionError::EmptyFile`] or [`ExtractionError::FileTooLarge`]
    /// before spawning. The worker can return [`ExtractionError::UnsupportedMediaType`],
    /// [`ExtractionError::MalformedFile`], [`ExtractionError::NoExtractableText`],
    /// or [`ExtractionError::LimitExceeded`]. [`ExtractionError::Timeout`] means
    /// the wall deadline elapsed. [`ExtractionError::ExtractionFailed`] covers
    /// missing tools, unsupported hosts, I/O and protocol errors. Each variant's
    /// display value is the stable API error code.
    ///
    /// Cancellation kills the process group, including native OCR children.
    /// The child is reaped by Tokio; temporary files are removed on completion
    /// or cancellation. Callers must acquire a CPU/admission slot before calling.
    /// # Examples
    /// ```no_run
    /// # async fn example(extractor: &whorl::extraction::Extractor) -> Result<(), whorl::extraction::ExtractionError> {
    /// let upload = tokio::fs::read("upload.pdf").await.unwrap();
    /// let document = extractor.extract(&upload).await?;
    /// assert!(!document.text.trim().is_empty());
    /// # Ok(()) }
    /// ```
    #[expect(
        clippy::too_many_lines,
        reason = "Subprocess lifetime and bounded protocol handling share one cancellation scope"
    )]
    pub async fn extract(&self, input: &[u8]) -> Result<ExtractedDocument, ExtractionError> {
        if input.is_empty() {
            return Err(ExtractionError::EmptyFile);
        }
        if input.len() as u64 > self.limits.max_input_bytes {
            return Err(ExtractionError::FileTooLarge);
        }
        if !cfg!(target_os = "linux") {
            return Err(ExtractionError::ExtractionFailed);
        }
        let directory = tempfile::tempdir().map_err(|_| ExtractionError::ExtractionFailed)?;
        let path = directory.path().join("input");
        tokio::fs::write(&path, input)
            .await
            .map_err(|_| ExtractionError::ExtractionFailed)?;
        let mut command = tokio::process::Command::new("prlimit");
        command
            .args([
                "--cpu=60:60",
                "--as=1073741824:1073741824",
                "--fsize=16777216:16777216",
                "--core=0:0",
                "--",
            ])
            .arg(&self.worker)
            .arg(&path)
            .env("TMPDIR", directory.path())
            .env("OMP_THREAD_LIMIT", "2")
            .env("RAYON_NUM_THREADS", "2")
            .env("RTEN_NUM_THREADS", "2")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command
            .spawn()
            .map_err(|_| ExtractionError::ExtractionFailed)?;
        let group = ProcessGroup(child.id().ok_or(ExtractionError::ExtractionFailed)?);
        let budget = self
            .limits
            .max_text_bytes
            .saturating_mul(6)
            .saturating_add(1024 * 1024);
        let result = tokio::time::timeout(self.timeout, async {
            let encoded =
                serde_json::to_vec(&self.limits).map_err(|_| ExtractionError::ExtractionFailed)?;
            let mut stdin = child
                .stdin
                .take()
                .ok_or(ExtractionError::ExtractionFailed)?;
            stdin
                .write_all(&encoded)
                .await
                .map_err(|_| ExtractionError::ExtractionFailed)?;
            drop(stdin);
            let stdout = child
                .stdout
                .take()
                .ok_or(ExtractionError::ExtractionFailed)?;
            let mut bytes = Vec::new();
            stdout
                .take(budget as u64 + 1)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| ExtractionError::ExtractionFailed)?;
            if bytes.len() > budget {
                return Err(ExtractionError::LimitExceeded);
            }
            let status = child
                .wait()
                .await
                .map_err(|_| ExtractionError::ExtractionFailed)?;
            if !status.success() {
                #[cfg(unix)]
                {
                    use std::os::unix::process::ExitStatusExt;
                    if matches!(
                        status.signal(),
                        Some(libc::SIGXCPU | libc::SIGXFSZ | libc::SIGKILL)
                    ) {
                        return Err(ExtractionError::LimitExceeded);
                    }
                }
                return Err(ExtractionError::ExtractionFailed);
            }
            let document: Result<ExtractedDocument, ExtractionError> =
                serde_json::from_slice(&bytes).map_err(|_| ExtractionError::ExtractionFailed)?;
            let document = document?;
            if document.text.len() > self.limits.max_text_bytes {
                return Err(ExtractionError::LimitExceeded);
            }
            if document.text.trim().is_empty() {
                return Err(ExtractionError::NoExtractableText);
            }
            Ok(document)
        })
        .await
        .unwrap_or(Err(ExtractionError::Timeout));
        drop(group);
        let _ = child.wait().await;
        result
    }
}

struct ProcessGroup(u32);

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Ok(pid) = i32::try_from(self.0) {
            // SAFETY: kill takes an integer process-group identifier and no pointers.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
    }
}
