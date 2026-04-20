// SPDX-License-Identifier: AGPL-3.0-or-later

//! Process-level fork isolation for BAR0 MMIO operations.
//!
//! GPU hardware registers accessed via VFIO BAR0 mmap can hang indefinitely
//! (kernel D-state) when the hardware is in a bad state — e.g., partially
//! initialized memory controllers, powered-down engines, or firmware stalls.
//! A D-state thread cannot be killed; only the entire process can be torn down.
//!
//! [`fork_isolated_raw`] runs a closure in a forked child process. If the child
//! hangs (exceeds `timeout`), the parent sends `SIGKILL`. The parent's VFIO
//! fds, mmap regions, and threads are unaffected.
//!
//! # Safety
//!
//! `fork()` in a multi-threaded program is dangerous: only the calling thread
//! survives in the child, and any mutex held by another thread becomes
//! permanently locked. The child must therefore:
//!
//! - Avoid heap allocation (`malloc` may deadlock on its internal mutex)
//! - Avoid locking any mutex
//! - Only perform raw MMIO reads/writes (volatile ptr ops on mmap'd BAR0)
//! - Communicate results via a pre-allocated pipe

use std::os::fd::{AsFd, BorrowedFd};
use std::time::Duration;

use rustix::io::{read, write};
use rustix::pipe::{PipeFlags, pipe_with};
use rustix::process::{Signal, WaitOptions, kill_process, waitpid};

/// Result of a fork-isolated operation.
#[derive(Debug)]
pub enum IsolationResult<T> {
    /// Child completed successfully and returned a value.
    Ok(T),
    /// Child was killed after exceeding the timeout (probable D-state).
    Timeout,
    /// Child exited with a non-zero status or was killed by a signal.
    ChildFailed {
        /// Exit code (normal exit) or negative signal number.
        status: i32,
    },
    /// Fork or pipe setup failed.
    ForkError(std::io::Error),
}

impl<T> IsolationResult<T> {
    /// `true` only for [`IsolationResult::Ok`].
    #[must_use]
    pub fn is_ok(&self) -> bool {
        matches!(self, IsolationResult::Ok(_))
    }

    /// `true` when the child was killed due to timeout (D-state).
    #[must_use]
    pub fn is_timeout(&self) -> bool {
        matches!(self, IsolationResult::Timeout)
    }
}

/// Run `f` in a forked child process with a kill-timeout safety net.
///
/// `f` receives the write end of a pipe. It should write its result bytes
/// to this fd, then return. The function exits the child via `exit_group(0)`.
/// If `f` hangs, the parent kills the child after `timeout`.
///
/// Returns the bytes written by the child (up to `max_result_bytes`).
///
/// # Safety
///
/// The caller must ensure `f` is async-signal-safe: no heap allocation,
/// no mutex locking, no stdio. Only raw MMIO + pipe write.
pub fn fork_isolated_raw(
    timeout: Duration,
    max_result_bytes: usize,
    f: impl FnOnce(BorrowedFd<'_>),
) -> IsolationResult<Vec<u8>> {
    let (pipe_read, pipe_write): (std::os::fd::OwnedFd, std::os::fd::OwnedFd) =
        match pipe_with(PipeFlags::CLOEXEC) {
            Ok(pair) => pair,
            Err(e) => return IsolationResult::ForkError(std::io::Error::from(e)),
        };

    // SAFETY: fork in multi-threaded context. The child must only do
    // async-signal-safe operations (raw MMIO + pipe write + _exit).
    let fork_result = unsafe { rustix::runtime::kernel_fork() };

    match fork_result {
        Err(e) => IsolationResult::ForkError(std::io::Error::from(e)),
        Ok(rustix::runtime::Fork::Child(_)) => {
            // ── Child process ──
            drop(pipe_read);
            f(pipe_write.as_fd());
            drop(pipe_write);
            rustix::runtime::exit_group(0)
        }
        Ok(rustix::runtime::Fork::ParentOf(child_pid)) => {
            // ── Parent process ──
            drop(pipe_write);

            let deadline = std::time::Instant::now() + timeout;
            let poll_interval = Duration::from_millis(10);

            loop {
                match waitpid(Some(child_pid), WaitOptions::NOHANG) {
                    Ok(Some((_pid, status))) => {
                        let bytes = read_pipe(&pipe_read, max_result_bytes);
                        return classify_exit(status, bytes);
                    }
                    Ok(None) => {
                        if std::time::Instant::now() >= deadline {
                            let _ = kill_process(child_pid, Signal::KILL);
                            let _ = waitpid(Some(child_pid), WaitOptions::empty());
                            return IsolationResult::Timeout;
                        }
                        std::thread::sleep(poll_interval);
                    }
                    Err(_) => return IsolationResult::ChildFailed { status: -1 },
                }
            }
        }
    }
}

fn read_pipe(pipe_fd: &impl AsFd, max_bytes: usize) -> Vec<u8> {
    let mut buf = vec![0u8; max_bytes];
    match read(pipe_fd, &mut buf) {
        Ok(n) => {
            buf.truncate(n);
            buf
        }
        Err(_) => Vec::new(),
    }
}

fn classify_exit(status: rustix::process::WaitStatus, bytes: Vec<u8>) -> IsolationResult<Vec<u8>> {
    if status.exited() && status.exit_status() == Some(0) {
        IsolationResult::Ok(bytes)
    } else if status.signaled() {
        let sig = status.terminating_signal().unwrap_or(-1);
        IsolationResult::ChildFailed { status: -sig }
    } else {
        let code = status.exit_status().unwrap_or(-1);
        IsolationResult::ChildFailed { status: code }
    }
}

/// Fork-isolated BAR0 register read.
///
/// Reads a single 32-bit register at `offset` from the BAR0 mmap pointer.
/// If the read hangs (D-state), the child is killed after `timeout`.
///
/// # Safety
///
/// `bar0_ptr` must point to a valid mmap'd BAR0 region at least
/// `offset + 4` bytes long.
pub unsafe fn fork_isolated_mmio_read(
    bar0_ptr: *const u8,
    offset: u32,
    timeout: Duration,
) -> IsolationResult<u32> {
    let ptr_val = bar0_ptr as usize;

    let result = fork_isolated_raw(timeout, 4, move |pipe_fd| {
        // SAFETY: same mmap in child (COW pages, same virtual address).
        let bar0 = ptr_val as *const u8;
        let reg_ptr = unsafe { bar0.add(offset as usize).cast::<u32>() };
        let value = unsafe { std::ptr::read_volatile(reg_ptr) };
        let _ = write(pipe_fd, &value.to_le_bytes());
    });

    match result {
        IsolationResult::Ok(ref bytes) if bytes.len() == 4 => {
            IsolationResult::Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        }
        IsolationResult::Ok(_) => IsolationResult::ChildFailed { status: -1 },
        IsolationResult::Timeout => IsolationResult::Timeout,
        IsolationResult::ChildFailed { status } => IsolationResult::ChildFailed { status },
        IsolationResult::ForkError(e) => IsolationResult::ForkError(e),
    }
}

/// Fork-isolated BAR0 register write.
///
/// Writes a single 32-bit value to the BAR0 register at `offset`.
/// If the write hangs (D-state), the child is killed after `timeout`.
///
/// # Safety
///
/// `bar0_ptr` must point to a valid mmap'd BAR0 region at least
/// `offset + 4` bytes long.
pub unsafe fn fork_isolated_mmio_write(
    bar0_ptr: *mut u8,
    offset: u32,
    value: u32,
    timeout: Duration,
) -> IsolationResult<()> {
    let ptr_val = bar0_ptr as usize;

    let result = fork_isolated_raw(timeout, 1, move |pipe_fd| {
        let bar0 = ptr_val as *mut u8;
        let reg_ptr = unsafe { bar0.add(offset as usize).cast::<u32>() };
        unsafe { std::ptr::write_volatile(reg_ptr, value) };
        let _ = write(pipe_fd, &[1u8]);
    });

    match result {
        IsolationResult::Ok(ref bytes) if !bytes.is_empty() && bytes[0] == 1 => {
            IsolationResult::Ok(())
        }
        IsolationResult::Ok(_) => IsolationResult::ChildFailed { status: -1 },
        IsolationResult::Timeout => IsolationResult::Timeout,
        IsolationResult::ChildFailed { status } => IsolationResult::ChildFailed { status },
        IsolationResult::ForkError(e) => IsolationResult::ForkError(e),
    }
}

/// Fork-isolated BAR0 batch operation.
///
/// Reads/writes multiple registers in a single fork. Each operation is
/// `(offset, Option<value>)`: `None` = read, `Some(v)` = write.
///
/// Returns one `u32` per operation (reads return the register value,
/// writes return the written value as confirmation).
///
/// # Safety
///
/// `bar0_ptr` must point to a valid mmap'd BAR0 region large enough
/// for all referenced offsets.
pub unsafe fn fork_isolated_mmio_batch(
    bar0_ptr: *mut u8,
    ops: &[(u32, Option<u32>)],
    timeout: Duration,
) -> IsolationResult<Vec<u32>> {
    let ptr_val = bar0_ptr as usize;
    let max_bytes = ops.len() * 4;
    let ops_copy: Vec<(u32, Option<u32>)> = ops.to_vec();

    let result = fork_isolated_raw(timeout, max_bytes, move |pipe_fd| {
        let bar0 = ptr_val as *mut u8;
        for &(offset, maybe_val) in &ops_copy {
            let reg_ptr = unsafe { bar0.add(offset as usize).cast::<u32>() };
            let result_val = match maybe_val {
                Some(v) => {
                    unsafe { std::ptr::write_volatile(reg_ptr, v) };
                    v
                }
                None => unsafe { std::ptr::read_volatile(reg_ptr) },
            };
            let _ = write(pipe_fd, &result_val.to_le_bytes());
        }
    });

    match result {
        IsolationResult::Ok(bytes) => {
            let values = bytes
                .chunks_exact(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            IsolationResult::Ok(values)
        }
        IsolationResult::Timeout => IsolationResult::Timeout,
        IsolationResult::ChildFailed { status } => IsolationResult::ChildFailed { status },
        IsolationResult::ForkError(e) => IsolationResult::ForkError(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fork_isolated_raw_success() {
        let result = fork_isolated_raw(Duration::from_secs(5), 16, |pipe_fd| {
            let _ = write(pipe_fd, b"hello");
        });
        match result {
            IsolationResult::Ok(data) => assert_eq!(&data, b"hello"),
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    #[test]
    fn fork_isolated_raw_timeout() {
        let result = fork_isolated_raw(Duration::from_millis(200), 4, |_pipe_fd| {
            std::thread::sleep(Duration::from_secs(60));
        });
        assert!(result.is_timeout(), "expected Timeout, got {result:?}");
    }

    #[test]
    fn fork_isolated_raw_child_crash() {
        let result = fork_isolated_raw(Duration::from_secs(5), 4, |_pipe_fd| {
            rustix::runtime::exit_group(42);
        });
        match result {
            IsolationResult::ChildFailed { status } => assert_eq!(status, 42),
            other => panic!("expected ChildFailed(42), got {other:?}"),
        }
    }

    #[test]
    fn fork_isolated_raw_empty_result() {
        let result = fork_isolated_raw(Duration::from_secs(5), 4, |_pipe_fd| {});
        match result {
            IsolationResult::Ok(data) => assert!(data.is_empty()),
            other => panic!("expected Ok([]), got {other:?}"),
        }
    }

    #[test]
    fn fork_isolated_raw_large_payload() {
        let result = fork_isolated_raw(Duration::from_secs(5), 1024, |pipe_fd| {
            let data = [0xABu8; 256];
            let _ = write(pipe_fd, &data);
        });
        match result {
            IsolationResult::Ok(data) => {
                assert_eq!(data.len(), 256);
                assert!(data.iter().all(|&b| b == 0xAB));
            }
            other => panic!("expected Ok(256 bytes), got {other:?}"),
        }
    }
}
