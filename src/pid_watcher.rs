//! Async process exit detection using Linux pidfd.
//!
//! Uses `pidfd_open()` (Linux 5.3+) for poll-free, event-driven process exit notification.
//! Falls back to polling `/proc/<pid>` if pidfd is unavailable (shouldn't happen on Ubuntu 20.04+).

use std::future::Future;
use std::pin::Pin;

/// Watch a process by PID and return a future that completes when the process exits.
///
/// # Arguments
/// * `pid` - The process ID to watch (does not need to be a child of this process)
///
/// # Returns
/// A future that resolves when the process exits. If the process has already exited
/// or the PID is invalid, the future resolves immediately.
pub fn watch_pid(pid: u32) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(async move {
        match async_pidfd::AsyncPidFd::from_pid(pid as libc::pid_t) {
            Ok(pidfd) => {
                // wait() returns Result<ExitStatus, io::Error> - we only care that it exited
                let _ = pidfd.wait().await;
            }
            Err(e) => {
                // ESRCH means process already exited or doesn't exist
                if e.raw_os_error() == Some(libc::ESRCH) {
                    tracing::debug!("pid {pid} already exited (ESRCH)");
                    return;
                }
                // Fallback to /proc polling for other errors (shouldn't happen on supported kernels)
                tracing::warn!("pidfd_open failed for pid {pid}: {e}, falling back to polling");
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
                        break;
                    }
                }
            }
        }
    })
}
