//! Cross-process serialization for persist-before-write font operations.
use anyhow::{Context, Result, bail};
use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::Duration;

pub const ZOOM_LOCK_WAIT: Duration = Duration::from_secs(40);
const LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(50);

/// Owning the file descriptor owns the lock. Closing it releases flock, including
/// unwinding; process exit releases it in the kernel. Never unlink the lock file.
pub struct ZoomLock {
    _file: File,
}
impl ZoomLock {
    pub fn try_acquire(path: &Path) -> Result<Option<Self>> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)
            .with_context(|| format!("open Alacritty zoom lock {}", path.display()))?;
        // SAFETY: file owns a valid descriptor throughout the non-blocking call.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(Some(Self { _file: file }));
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
            return Ok(None);
        }
        Err(error).context("acquire Alacritty zoom lock")
    }

    pub async fn acquire(path: &Path) -> Result<Self> {
        let deadline = tokio::time::Instant::now() + ZOOM_LOCK_WAIT;
        let mut waiting = false;
        loop {
            if let Some(guard) = Self::try_acquire(path)? {
                return Ok(guard);
            }
            if !waiting {
                tracing::debug!(path = %path.display(), "waiting for Alacritty zoom operation lock");
                waiting = true;
            }
            if tokio::time::Instant::now() >= deadline {
                bail!(
                    "Alacritty zoom lock timeout after {}s: {}",
                    ZOOM_LOCK_WAIT.as_secs(),
                    path.display()
                );
            }
            tokio::time::sleep(LOCK_RETRY_INTERVAL).await;
        }
    }
}

#[cfg(test)]
#[path = "lock_tests.rs"]
mod tests;
