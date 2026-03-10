//! Async process exit detection using Linux pidfd.
//!
//! Uses `pidfd_open()` (Linux 5.3+) for poll-free, event-driven process exit notification.
//! Falls back to polling `/proc/<pid>` if pidfd is unavailable (shouldn't happen on Ubuntu 20.04+).

use std::future::Future;
use std::pin::Pin;

/// Read the starttime field (field 22) from `/proc/<pid>/stat`.
///
/// The starttime is the number of clock ticks after system boot when the process started.
/// It is unique per PID lifetime and can be used to detect PID recycling.
///
/// Returns `None` if the process does not exist or the file cannot be parsed.
fn read_proc_starttime(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // Field 2 (comm) is in parentheses and may contain spaces or nested parens.
    // Use the last ')' to safely locate the end of the comm field.
    let after_comm = stat
        .rfind(')')?
        .checked_add(1)
        .and_then(|i| stat.get(i..))?;
    // Fields after comm (1-indexed per proc(5)):
    //   3=state, 4=ppid, 5=pgrp, 6=session, 7=tty_nr, 8=tpgid,
    //   9=flags, 10=minflt, 11=cminflt, 12=majflt, 13=cmajflt,
    //   14=utime, 15=stime, 16=cutime, 17=cstime, 18=priority,
    //   19=nice, 20=num_threads, 21=itrealvalue, 22=starttime  <- index 19 (0-based)
    after_comm.split_whitespace().nth(19)?.parse().ok()
}

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
                // Record the process starttime so we can detect PID recycling: if the OS
                // reuses this PID for a new process, /proc/<pid> will still exist but the
                // starttime will differ, and we must treat the original process as exited.
                let initial_starttime = read_proc_starttime(pid);
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    match read_proc_starttime(pid) {
                        None => break, // /proc/<pid>/stat gone — process exited
                        Some(current_starttime) => {
                            if let Some(initial) = initial_starttime {
                                if current_starttime != initial {
                                    // PID was recycled by a new unrelated process
                                    tracing::debug!(
                                        "pid {pid} starttime changed (PID recycled), treating as exited"
                                    );
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_proc_starttime_self() {
        // The current process must have a valid, non-zero starttime.
        let pid = std::process::id();
        let starttime = read_proc_starttime(pid);
        assert!(starttime.is_some(), "should read starttime for own PID");
        assert_ne!(starttime.unwrap(), 0, "starttime should be non-zero");
    }

    #[test]
    fn read_proc_starttime_nonexistent() {
        // PID 0 is the idle task and has no /proc/0/stat entry readable by user processes.
        // Any sufficiently large PID that doesn't exist also works.
        // We use a PID that is astronomically unlikely to exist.
        let starttime = read_proc_starttime(u32::MAX);
        assert!(starttime.is_none(), "non-existent PID should return None");
    }

    #[test]
    fn read_proc_starttime_is_stable() {
        // Reading starttime twice for the same live process must yield the same value.
        let pid = std::process::id();
        let first = read_proc_starttime(pid);
        let second = read_proc_starttime(pid);
        assert_eq!(first, second, "starttime must be stable across reads");
    }
}
