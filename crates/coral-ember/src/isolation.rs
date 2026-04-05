// SPDX-License-Identifier: AGPL-3.0-only
//! MMIO operation isolation — process-level containment for GPU operations.
//!
//! GPU BAR0 operations (PRAMIN writes, PMC resets, falcon register access)
//! can hang the executing CPU core indefinitely via PCIe flow-control stalls.
//! A thread-level watchdog cannot interrupt a stuck volatile MMIO because the
//! stall is at the hardware level — the core's store buffer is waiting for
//! PCIe credits that never arrive.
//!
//! [`fork_isolated_mmio`] solves this with true process isolation:
//!
//! 1. `fork()` creates a child that inherits the BAR0 mmap
//! 2. The child executes the dangerous operation on its own CPU core
//! 3. The parent monitors via `waitpid(WNOHANG)` + timeout
//! 4. If the child hangs: parent sends `SIGKILL`, triggers a PCIe
//!    secondary bus reset (from the bridge side — always accessible),
//!    then reaps the child
//! 5. If the child succeeds: parent reads the result from a pipe
//!
//! Ember's process stays alive regardless. The device is marked faulted
//! and can be recovered via `ember.device.recover`.
//!
//! [`with_mmio_watchdog`] is retained as a lighter-weight fallback for
//! operations where fork overhead is undesirable and the CPU-stall risk
//! is lower (e.g., single register reads).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::sysfs;

/// Default timeout for dangerous MMIO operations.
pub const MMIO_WATCHDOG_TIMEOUT: Duration = Duration::from_secs(5);

/// Result of a fork-isolated MMIO operation.
#[derive(Debug)]
pub enum ForkResult {
    /// Child completed successfully. Pipe contains JSON result.
    Ok(Vec<u8>),
    /// Child timed out — bus reset was triggered, device is faulted.
    Timeout,
    /// Child exited with non-zero status (crashed, signaled, etc).
    ChildFailed { status: i32 },
    /// fork() itself failed.
    ForkFailed(std::io::Error),
    /// pipe() failed.
    PipeFailed(std::io::Error),
}

/// Run a dangerous MMIO operation in a forked child process.
///
/// The child inherits the parent's BAR0 mmap (same virtual addresses,
/// same physical mapping). If the child's CPU core gets stuck on an
/// MMIO operation, the parent detects the timeout and:
/// 1. Sends SIGKILL to the child
/// 2. Triggers a **raw** PCIe Secondary Bus Reset by toggling bit 6 of
///    the bridge's `PCI_BRIDGE_CONTROL` register via setpci (this avoids
///    the kernel's `pci_save_state()` which would hang on the stuck link)
/// 3. Reaps the child (the bus reset unblocks the stuck core, allowing
///    SIGKILL delivery)
///
/// Before forking, AER error reporting is **masked** on the parent bridge
/// to prevent the kernel's AER handler from chasing the hung device's config
/// space (which cascades into D-state lockups). AER is restored after the
/// operation completes regardless of outcome.
///
/// The `op` closure runs in the child process after `fork()`. It receives
/// a write end of a pipe; it should write its result as bytes and return.
/// The closure MUST NOT:
/// - Acquire Rust `Mutex`/`RwLock` (may be poisoned from other threads)
/// - Use `println!`/`eprintln!` (may deadlock on stdio lock)
/// - Allocate large amounts of memory (allocator locks may be held)
///
/// # Safety
///
/// Uses `libc::fork()` in a multi-threaded process. The child must
/// only perform async-signal-safe operations and call `_exit()`.
pub fn fork_isolated_mmio(
    bdf: &str,
    timeout: Duration,
    op: impl FnOnce(i32),
) -> ForkResult {
    // ── Phase 0: Mask AER on the bridge ──────────────────────────────────
    // The kernel's AER handler runs in a workqueue thread. When a PCIe
    // error arrives from the GPU, the AER handler reads the device's AER
    // registers — through the same stuck downstream link. That thread
    // enters D-state, cascading to system lockup. Masking AER report bits
    // prevents the kernel from even trying.
    let aer_saved = sysfs::mask_bridge_aer(bdf);

    let result = fork_isolated_mmio_inner(bdf, timeout, op);

    // ── Restore AER regardless of outcome ────────────────────────────────
    if let Some((ref bridge_bdf, original_val)) = aer_saved {
        sysfs::unmask_bridge_aer(bridge_bdf, original_val);
    }

    result
}

fn fork_isolated_mmio_inner(
    bdf: &str,
    timeout: Duration,
    op: impl FnOnce(i32),
) -> ForkResult {
    let mut pipe_fds = [0i32; 2];
    if unsafe { libc::pipe(pipe_fds.as_mut_ptr()) } != 0 {
        return ForkResult::PipeFailed(std::io::Error::last_os_error());
    }
    let (pipe_read, pipe_write) = (pipe_fds[0], pipe_fds[1]);

    let pid = unsafe { libc::fork() };
    match pid {
        -1 => {
            unsafe {
                libc::close(pipe_read);
                libc::close(pipe_write);
            }
            ForkResult::ForkFailed(std::io::Error::last_os_error())
        }

        0 => {
            // ═══ CHILD PROCESS ═══
            unsafe { libc::close(pipe_read); }
            op(pipe_write);
            unsafe {
                libc::close(pipe_write);
                libc::_exit(0);
            }
        }

        child_pid => {
            // ═══ PARENT PROCESS ═══
            unsafe { libc::close(pipe_write); }

            let start = std::time::Instant::now();
            let poll_interval = Duration::from_millis(50);

            loop {
                let mut status: libc::c_int = 0;
                let ret = unsafe {
                    libc::waitpid(child_pid, &mut status, libc::WNOHANG)
                };

                if ret == child_pid {
                    let result_bytes = read_pipe_result(pipe_read);
                    unsafe { libc::close(pipe_read); }

                    if libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0 {
                        return ForkResult::Ok(result_bytes);
                    }
                    return ForkResult::ChildFailed { status };
                }

                if ret == -1 {
                    unsafe { libc::close(pipe_read); }
                    return ForkResult::ChildFailed { status: -1 };
                }

                // ret == 0: child still running
                if start.elapsed() > timeout {
                    tracing::error!(
                        bdf,
                        child_pid,
                        timeout_ms = timeout.as_millis(),
                        "fork_isolated_mmio: child TIMED OUT — triggering raw SBR"
                    );

                    // SIGKILL first (won't take effect until bus reset
                    // unblocks the D-state core, but registers the intent)
                    unsafe { libc::kill(child_pid, libc::SIGKILL); }

                    // Raw SBR: toggle PCI_BRIDGE_CONTROL bit 6 directly via
                    // setpci. This NEVER reads the downstream device's config
                    // space, so it cannot hang even when the link is stuck.
                    if let Err(e) = sysfs::raw_bridge_sbr(bdf) {
                        tracing::error!(
                            bdf,
                            error = %e,
                            "raw SBR failed — falling back to kernel bridge reset"
                        );
                        // Fallback: try the kernel path (may hang if device
                        // link is stuck, but we have AER masked so it's safer)
                        if let Err(e2) = sysfs::pci_bridge_reset(bdf) {
                            tracing::error!(
                                bdf,
                                error = %e2,
                                "kernel bridge reset also failed"
                            );
                        }
                    }

                    // Reap the child (bus reset should unblock the stuck core)
                    for attempt in 0..40 {
                        let ret = unsafe {
                            libc::waitpid(child_pid, &mut status, libc::WNOHANG)
                        };
                        if ret == child_pid || ret == -1 {
                            break;
                        }
                        if attempt < 20 {
                            std::thread::sleep(Duration::from_millis(250));
                        } else {
                            std::thread::sleep(Duration::from_millis(500));
                        }
                    }

                    unsafe { libc::close(pipe_read); }
                    return ForkResult::Timeout;
                }

                std::thread::sleep(poll_interval);
            }
        }
    }
}

/// Read all available bytes from a raw fd (non-blocking-safe, used in parent
/// after child exits to drain the result pipe).
pub fn read_pipe_result(fd: i32) -> Vec<u8> {
    let mut buf = vec![0u8; 8192];
    let mut result = Vec::new();
    loop {
        let n = unsafe {
            libc::read(fd, buf.as_mut_ptr().cast(), buf.len())
        };
        if n <= 0 {
            break;
        }
        result.extend_from_slice(&buf[..n as usize]);
    }
    result
}

// ─── Thread-level watchdog (lightweight fallback) ──────────────────────────

/// Run `op` with a background watchdog that triggers a bus reset on timeout.
///
/// Lighter-weight than [`fork_isolated_mmio`] but cannot protect against
/// CPU-core-level PCIe stalls. Use only for operations with low stall risk
/// (e.g., single register reads where the PCIe timeout will return 0xFFFFFFFF).
pub fn with_mmio_watchdog<R>(bdf: &str, timeout: Duration, op: impl FnOnce() -> R) -> (R, bool) {
    let completed = Arc::new(AtomicBool::new(false));
    let watchdog_fired = Arc::new(AtomicBool::new(false));

    let bdf_owned = bdf.to_string();
    let completed_wd = Arc::clone(&completed);
    let fired_wd = Arc::clone(&watchdog_fired);

    let watchdog = std::thread::Builder::new()
        .name(format!("mmio-wd-{bdf}"))
        .spawn(move || {
            let start = std::time::Instant::now();
            let check_interval = Duration::from_millis(100);

            while start.elapsed() < timeout {
                std::thread::sleep(check_interval);
                if completed_wd.load(Ordering::Acquire) {
                    return;
                }
            }

            if completed_wd.load(Ordering::Acquire) {
                return;
            }

            fired_wd.store(true, Ordering::Release);

            tracing::error!(
                bdf = %bdf_owned,
                timeout_ms = timeout.as_millis(),
                "MMIO WATCHDOG FIRED — triggering raw SBR to unblock stuck MMIO"
            );

            // Use raw SBR (setpci) to avoid kernel's pci_save_state
            // which would hang if the downstream link is stuck.
            match sysfs::raw_bridge_sbr(&bdf_owned) {
                Ok(()) => {
                    tracing::info!(bdf = %bdf_owned, "raw SBR complete — stuck MMIO should unblock");
                }
                Err(e) => {
                    tracing::error!(
                        bdf = %bdf_owned,
                        error = %e,
                        "raw SBR FAILED — attempting kernel bridge reset"
                    );
                    if let Err(e2) = sysfs::pci_bridge_reset(&bdf_owned) {
                        tracing::error!(
                            bdf = %bdf_owned,
                            error = %e2,
                            "kernel bridge reset also FAILED — system may require reboot"
                        );
                    }
                }
            }
        })
        .expect("spawn MMIO watchdog thread");

    let result = op();

    completed.store(true, Ordering::Release);
    let _ = watchdog.join();

    let fired = watchdog_fired.load(Ordering::Acquire);
    if fired {
        tracing::warn!(bdf, "MMIO operation returned after watchdog fired — device was bus-reset");
    }

    (result, fired)
}

// ─── Operation Safety Tiers ─────────────────────────────────────────────────

/// Danger classification for MMIO operations. Each tier specifies the
/// required isolation level and applicable protections.
///
/// When an MMIO handler declares its tier, the dispatcher automatically
/// applies the appropriate isolation:
///
/// | Tier | Isolation | Protections |
/// |------|-----------|-------------|
/// | `RegisterIo` | Thread watchdog | preflight_check + circuit breaker |
/// | `BulkVram` | Fork isolation | AER mask + raw SBR + sequencer check |
/// | `EngineReset` | Fork isolation | AER mask + raw SBR + sequencing rules |
/// | `FalconBind` | Fork isolation | Full: AER + SBR + ITFEN + DMACTL |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationTier {
    /// Single register read/write. PCIe timeout returns 0xFFFF_FFFF.
    /// Low risk — uses thread-level watchdog.
    RegisterIo,

    /// Bulk PRAMIN writes to GPU VRAM via the BAR0 window. Can exhaust
    /// PCIe posted-write credits on unresponsive hardware.
    /// **MUST happen BEFORE any engine reset** (PRAMIN-first ordering rule).
    BulkVram,

    /// Engine reset (PMC toggle). Changes GPU internal state.
    /// Must NOT be concurrent with BulkVram writes.
    /// After this, only RegisterIo is safe until device returns to Pristine.
    EngineReset,

    /// Falcon context bind (DMA operation). Requires ITFEN + DMACTL.
    /// Highest risk — full fork isolation with all protections.
    FalconBind,
}

impl OperationTier {
    /// Whether this tier requires fork-level process isolation.
    pub fn requires_fork_isolation(&self) -> bool {
        matches!(
            self,
            Self::BulkVram | Self::EngineReset | Self::FalconBind
        )
    }

    /// Whether this tier requires AER masking on the bridge.
    pub fn requires_aer_mask(&self) -> bool {
        matches!(
            self,
            Self::BulkVram | Self::EngineReset | Self::FalconBind
        )
    }

    /// Recommended timeout for this tier.
    pub fn timeout(&self) -> Duration {
        match self {
            Self::RegisterIo => Duration::from_secs(2),
            Self::BulkVram => Duration::from_secs(10),
            Self::EngineReset => Duration::from_secs(5),
            Self::FalconBind => Duration::from_secs(10),
        }
    }
}

// ─── Canary Test Protocol ─────────────────────────────────────────────────

/// Result of a canary probe.
#[derive(Debug, Clone)]
pub struct CanaryResult {
    /// Whether the canary survived.
    pub survived: bool,
    /// What the canary was testing.
    pub probe_name: String,
    /// Output from the canary (if it survived).
    pub output: Option<Vec<u8>>,
    /// How long the canary ran before completing or being killed.
    pub elapsed: Duration,
    /// Whether a bus reset was triggered.
    pub bus_reset_triggered: bool,
}

/// Run a canary probe — a lightweight, disposable test of a dangerous
/// operation before committing to the full operation.
///
/// The canary runs in a forked child with a tight timeout. If it dies,
/// the parent reports the failure without the system locking up. The
/// caller can then decide whether to proceed, retry, or abort.
///
/// This is the "test the waters" pattern: send in a canary before
/// sending in the miners.
pub fn canary_probe(
    bdf: &str,
    probe_name: &str,
    timeout: Duration,
    op: impl FnOnce(i32),
) -> CanaryResult {
    let start = std::time::Instant::now();

    tracing::info!(bdf, probe = probe_name, "canary: probing");

    let result = fork_isolated_mmio(bdf, timeout, op);
    let elapsed = start.elapsed();

    match result {
        ForkResult::Ok(output) => {
            tracing::info!(
                bdf,
                probe = probe_name,
                elapsed_ms = elapsed.as_millis(),
                "canary: SURVIVED"
            );
            CanaryResult {
                survived: true,
                probe_name: probe_name.to_string(),
                output: Some(output),
                elapsed,
                bus_reset_triggered: false,
            }
        }
        ForkResult::Timeout => {
            tracing::warn!(
                bdf,
                probe = probe_name,
                elapsed_ms = elapsed.as_millis(),
                "canary: KILLED (timeout + bus reset)"
            );
            CanaryResult {
                survived: false,
                probe_name: probe_name.to_string(),
                output: None,
                elapsed,
                bus_reset_triggered: true,
            }
        }
        ForkResult::ChildFailed { status } => {
            tracing::warn!(
                bdf,
                probe = probe_name,
                status,
                elapsed_ms = elapsed.as_millis(),
                "canary: DIED (child failed)"
            );
            CanaryResult {
                survived: false,
                probe_name: probe_name.to_string(),
                output: None,
                elapsed,
                bus_reset_triggered: false,
            }
        }
        ForkResult::ForkFailed(e) => {
            tracing::error!(
                bdf,
                probe = probe_name,
                error = %e,
                "canary: FAILED (fork error)"
            );
            CanaryResult {
                survived: false,
                probe_name: probe_name.to_string(),
                output: None,
                elapsed,
                bus_reset_triggered: false,
            }
        }
        ForkResult::PipeFailed(e) => {
            tracing::error!(
                bdf,
                probe = probe_name,
                error = %e,
                "canary: FAILED (pipe error)"
            );
            CanaryResult {
                survived: false,
                probe_name: probe_name.to_string(),
                output: None,
                elapsed,
                bus_reset_triggered: false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watchdog_does_not_fire_on_fast_op() {
        let (result, fired) =
            with_mmio_watchdog("test:00:00.0", Duration::from_secs(2), || 42);
        assert_eq!(result, 42);
        assert!(!fired);
    }

    #[test]
    fn watchdog_fires_on_slow_op() {
        let (result, fired) = with_mmio_watchdog(
            "test:ff:ff.0",
            Duration::from_millis(200),
            || {
                std::thread::sleep(Duration::from_secs(2));
                99
            },
        );
        assert_eq!(result, 99);
        assert!(fired);
    }

    #[test]
    fn fork_isolation_fast_op_succeeds() {
        let result = fork_isolated_mmio(
            "test:00:00.0",
            Duration::from_secs(2),
            |pipe_fd| {
                let msg = b"OK";
                unsafe {
                    libc::write(pipe_fd, msg.as_ptr().cast(), msg.len());
                }
            },
        );
        assert!(matches!(result, ForkResult::Ok(_)));
    }

    #[test]
    fn operation_tier_register_io_properties() {
        let tier = OperationTier::RegisterIo;
        assert!(!tier.requires_fork_isolation());
        assert!(!tier.requires_aer_mask());
        assert_eq!(tier.timeout(), Duration::from_secs(2));
    }

    #[test]
    fn operation_tier_bulk_vram_requires_fork() {
        let tier = OperationTier::BulkVram;
        assert!(tier.requires_fork_isolation());
        assert!(tier.requires_aer_mask());
        assert_eq!(tier.timeout(), Duration::from_secs(10));
    }

    #[test]
    fn operation_tier_engine_reset_requires_fork() {
        let tier = OperationTier::EngineReset;
        assert!(tier.requires_fork_isolation());
        assert!(tier.requires_aer_mask());
    }

    #[test]
    fn operation_tier_falcon_bind_is_highest_risk() {
        let tier = OperationTier::FalconBind;
        assert!(tier.requires_fork_isolation());
        assert!(tier.requires_aer_mask());
        assert_eq!(tier.timeout(), Duration::from_secs(10));
    }

    #[test]
    fn canary_probe_fast_op_survives() {
        let result = canary_probe(
            "test:00:00.0",
            "boot0_read",
            Duration::from_secs(2),
            |pipe_fd| {
                let msg = b"ALIVE";
                unsafe {
                    libc::write(pipe_fd, msg.as_ptr().cast(), msg.len());
                }
            },
        );
        assert!(result.survived);
        assert_eq!(result.probe_name, "boot0_read");
        assert!(!result.bus_reset_triggered);
        assert!(result.output.is_some());
    }
}
