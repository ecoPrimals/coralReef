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

// ─── Graceful Shutdown ──────────────────────────────────────────────────────

/// Global flag set by the SIGTERM handler. Checked by the watchdog thread
/// to trigger graceful shutdown (disable bus master, remove socket, exit).
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Returns `true` if a SIGTERM has been received.
pub fn shutdown_requested() -> bool {
    SHUTDOWN_REQUESTED.load(Ordering::Acquire)
}

/// Install a SIGTERM handler that sets [`SHUTDOWN_REQUESTED`].
///
/// The handler only sets an `AtomicBool` (async-signal-safe). The actual
/// cleanup work happens in the watchdog thread that polls this flag.
pub fn install_sigterm_handler() {
    extern "C" fn sigterm_handler(_sig: libc::c_int) {
        SHUTDOWN_REQUESTED.store(true, Ordering::Release);
    }

    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = sigterm_handler as *const () as usize;
        sa.sa_flags = libc::SA_RESTART;
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaction(libc::SIGTERM, &sa, std::ptr::null_mut());
    }
    tracing::info!("SIGTERM handler installed — graceful shutdown enabled");
}

/// Disable PCI bus mastering via sysfs config space write.
///
/// Clears bit 2 of the PCI Command Register (offset 0x04) to prevent the
/// GPU from initiating DMA transactions. This is critical after a bus reset
/// — a misbehaving GPU with bus master enabled can poison the PCIe fabric
/// even after SBR completes.
///
/// Uses sysfs (`/sys/bus/pci/devices/{bdf}/config`) which works even when
/// BAR0 is stuck, since it goes through the host bridge, not the device.
pub fn enable_bus_master_via_sysfs(bdf: &str) {
    let config_path = format!(
        "{}/bus/pci/devices/{bdf}/config",
        coral_driver::linux_paths::sysfs_root()
    );

    let mut config = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&config_path)
    {
        Ok(f) => f,
        Err(e) => {
            tracing::error!(bdf, error = %e, "enable_bus_master: cannot open PCI config");
            return;
        }
    };

    use std::io::{Read, Seek, SeekFrom, Write};

    if config.seek(SeekFrom::Start(4)).is_err() {
        return;
    }
    let mut cmd_bytes = [0u8; 2];
    if config.read_exact(&mut cmd_bytes).is_err() {
        return;
    }
    let cmd = u16::from_le_bytes(cmd_bytes);
    if cmd & 0x04 != 0 {
        tracing::debug!(bdf, cmd = format_args!("{cmd:#06x}"), "bus master already enabled");
        return;
    }

    let new_cmd = cmd | 0x04;
    if config.seek(SeekFrom::Start(4)).is_err() {
        return;
    }
    if config.write_all(&new_cmd.to_le_bytes()).is_err() {
        tracing::error!(bdf, "enable_bus_master: write failed");
        return;
    }
    tracing::info!(bdf, old = format_args!("{cmd:#06x}"), new = format_args!("{new_cmd:#06x}"), "PCI bus master ENABLED");
}

/// Disable PCI bus master (clear bit 2 in the command register).
///
/// Uses sysfs (`/sys/bus/pci/devices/{bdf}/config`) which works even when
/// BAR0 is stuck, since it goes through the host bridge, not the device.
pub fn disable_bus_master_via_sysfs(bdf: &str) {
    let config_path = format!(
        "{}/bus/pci/devices/{bdf}/config",
        coral_driver::linux_paths::sysfs_root()
    );

    let mut config = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&config_path)
    {
        Ok(f) => f,
        Err(e) => {
            tracing::error!(bdf, error = %e, "disable_bus_master: cannot open PCI config");
            return;
        }
    };

    use std::io::{Read, Seek, SeekFrom, Write};

    if config.seek(SeekFrom::Start(4)).is_err() {
        tracing::error!(bdf, "disable_bus_master: seek to command register failed");
        return;
    }
    let mut cmd_bytes = [0u8; 2];
    if config.read_exact(&mut cmd_bytes).is_err() {
        tracing::error!(bdf, "disable_bus_master: read command register failed");
        return;
    }
    let cmd = u16::from_le_bytes(cmd_bytes);
    if cmd & 0x04 == 0 {
        tracing::debug!(bdf, cmd = format_args!("{cmd:#06x}"), "bus master already disabled");
        return;
    }

    let new_cmd = cmd & !0x04;
    if config.seek(SeekFrom::Start(4)).is_err() {
        tracing::error!(bdf, "disable_bus_master: seek for write failed");
        return;
    }
    if config.write_all(&new_cmd.to_le_bytes()).is_err() {
        tracing::error!(bdf, "disable_bus_master: write command register failed");
        return;
    }
    tracing::warn!(
        bdf,
        old_cmd = format_args!("{cmd:#06x}"),
        new_cmd = format_args!("{new_cmd:#06x}"),
        "bus master DISABLED via sysfs"
    );
}

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
    fork_isolated_mmio_opt(bdf, timeout, true, op)
}

/// Like [`fork_isolated_mmio`] but allows skipping the pre-fork bus master
/// disable. PRAMIN writes need bus master ON because the GPU's PRAMIN
/// engine uses an internal DMA path (gated by Bus Master Enable) to drain
/// its write buffer to VRAM.
pub fn fork_isolated_mmio_bus_master_on(
    bdf: &str,
    timeout: Duration,
    op: impl FnOnce(i32),
) -> ForkResult {
    fork_isolated_mmio_opt(bdf, timeout, false, op)
}

fn fork_isolated_mmio_opt(
    bdf: &str,
    timeout: Duration,
    disable_bus_master: bool,
    op: impl FnOnce(i32),
) -> ForkResult {
    // AER is kept masked for the device's entire lifetime by PcieArmor
    // (armed at device acquisition, disarmed at release). Per-operation
    // AER cycling was removed because it caused a setpci/sysfs I/O storm
    // that overwhelmed the PCIe root complex during rapid batch operations.

    // Pre-open bridge config fd for SBR escalation in the timeout handler.
    let bridge_fd = sysfs::find_parent_bridge(bdf).and_then(|bridge_bdf| {
        pre_open_bridge_sbr_fd(&bridge_bdf)
    });

    fork_isolated_mmio_inner(bdf, timeout, op, bridge_fd, disable_bus_master)
}

/// Pre-open the bridge's sysfs config file for SBR, returning
/// (fd, current_bridge_control_value). The fd is seeked to offset 0x3E.
fn pre_open_bridge_sbr_fd(bridge_bdf: &str) -> Option<(std::fs::File, u16)> {
    use std::io::{Read, Seek, SeekFrom};

    let config_path = format!(
        "{}/bus/pci/devices/{bridge_bdf}/config",
        coral_driver::linux_paths::sysfs_root()
    );
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&config_path)
        .ok()?;
    f.seek(SeekFrom::Start(0x3E)).ok()?;
    let mut ctrl = [0u8; 2];
    f.read_exact(&mut ctrl).ok()?;
    Some((f, u16::from_le_bytes(ctrl)))
}

/// Trigger SBR using a pre-opened bridge config fd.
///
/// This is the fast path for the fork timeout handler. Since the fd was
/// opened and seeked before the child started, this function only needs
/// to do two small pwrite(2) calls — no file open, no path resolution,
/// minimal kernel code path. This is critical when the AMD IOHUB is
/// partially stalled by a downstream PCIe error.
fn fast_sbr_via_fd(fd: &mut std::fs::File, ctrl_val: u16) -> Result<(), std::io::Error> {
    use std::io::{Seek, SeekFrom, Write};

    // Assert SBR (bit 6)
    fd.seek(SeekFrom::Start(0x3E))?;
    fd.write_all(&(ctrl_val | 0x0040).to_le_bytes())?;

    std::thread::sleep(Duration::from_millis(100));

    // De-assert SBR
    fd.seek(SeekFrom::Start(0x3E))?;
    fd.write_all(&(ctrl_val & !0x0040).to_le_bytes())?;

    std::thread::sleep(Duration::from_millis(500));
    Ok(())
}

fn fork_isolated_mmio_inner(
    bdf: &str,
    timeout: Duration,
    op: impl FnOnce(i32),
    mut bridge_fd: Option<(std::fs::File, u16)>,
    disable_bus_master: bool,
) -> ForkResult {
    // Crash-surviving trace — fsync'd after every write so we can
    // read the exact last successful step after a hard lockup + reboot.
    fn trace(bdf: &str, msg: &str) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/var/lib/coralreef/traces/ember_fork_trace.log")
        {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(f, "[{ts}] [{bdf}] {msg}");
        }
    }

    // ── Pre-fork hardening ──
    //
    // Disable the NMI watchdog before any MMIO fork. If the child stalls
    // on a PRAMIN write, the child's core hangs in kernel mode. With the
    // NMI watchdog enabled, the kernel detects the "hard lockup" and
    // triggers a panic. The panic handler tries to sync filesystems via
    // NVMe — which is behind the same frozen NBIO. The sync stalls,
    // freezing the panic handler, and the entire system is bricked.
    //
    // With NMI watchdog disabled, the stuck core stays stuck silently.
    // The other cores (and the parent process) remain alive and can
    // return a clean error to the caller.
    let _ = std::fs::write("/proc/sys/kernel/nmi_watchdog", "0");

    trace(bdf, &format!("FORK_START timeout={}ms nmi_wd=off", timeout.as_millis()));

    let mut pipe_fds = [0i32; 2];
    if unsafe { libc::pipe(pipe_fds.as_mut_ptr()) } != 0 {
        trace(bdf, "PIPE_FAILED");
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
            trace(bdf, "FORK_FAILED");
            ForkResult::ForkFailed(std::io::Error::last_os_error())
        }

        0 => {
            // ═══ CHILD PROCESS ═══
            unsafe { libc::close(pipe_read); }
            // Bus master toggle runs in the child — if sysfs stalls on a
            // degraded NBIO, only this (disposable) child process freezes.
            if disable_bus_master {
                disable_bus_master_via_sysfs(bdf);
            } else {
                enable_bus_master_via_sysfs(bdf);
            }
            op(pipe_write);
            unsafe {
                libc::close(pipe_write);
                libc::_exit(0);
            }
        }

        child_pid => {
            // ═══ PARENT PROCESS ═══
            //
            // CRITICAL: NO trace()/fsync() calls between fork() and the
            // poll loop. The child may stall the NBIO within microseconds
            // of starting (PRAMIN MMIO write). Any fsync here would block
            // on NVMe (same NBIO), preventing the timeout from ever firing.
            //
            // All post-fork traces are deferred to fire-and-forget threads
            // that cannot block the main polling/timeout path.
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
                    let elapsed_ms = start.elapsed().as_millis();

                    if libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0 {
                        let bdf_s = bdf.to_string();
                        let n_bytes = result_bytes.len();
                        let _ = std::thread::Builder::new()
                            .name("fork-trace-ok".into())
                            .spawn(move || trace(&bdf_s, &format!(
                                "CHILD_OK elapsed={elapsed_ms}ms bytes={n_bytes}"
                            )));
                        return ForkResult::Ok(result_bytes);
                    }
                    let bdf_s = bdf.to_string();
                    let _ = std::thread::Builder::new()
                        .name("fork-trace-fail".into())
                        .spawn(move || trace(&bdf_s, &format!(
                            "CHILD_FAILED status={status} elapsed={elapsed_ms}ms"
                        )));
                    return ForkResult::ChildFailed { status };
                }

                if ret == -1 {
                    unsafe { libc::close(pipe_read); }
                    let bdf_s = bdf.to_string();
                    let _ = std::thread::Builder::new()
                        .name("fork-trace-err".into())
                        .spawn(move || trace(&bdf_s, "WAITPID_ERROR"));
                    return ForkResult::ChildFailed { status: -1 };
                }

                // ret == 0: child still running
                if start.elapsed() > timeout {
                    // ── ZERO-I/O TIMEOUT HANDLER ──
                    //
                    // When the child stalls on a PRAMIN write, the GPU freezes the
                    // AMD NBIO's posted-write credit pool. This freezes ALL I/O
                    // through that NBIO: ECAM, CF8/CFC I/O ports, NVMe, everything.
                    //
                    // ANY I/O attempt from the parent (SBR, sysfs, trace writes,
                    // even I/O port instructions) will stall the parent's core too,
                    // cascading into a full system freeze.
                    //
                    // Strategy: SIGKILL the child, briefly try non-blocking reap,
                    // close our fd, and return IMMEDIATELY. No SBR, no disk I/O,
                    // no tracing. The stuck child core is a resource leak, but the
                    // rest of the system survives. Ember marks the device faulted
                    // and the user triggers a warm cycle to recover.
                    unsafe { libc::kill(child_pid, libc::SIGKILL); }

                    // Brief non-blocking reap attempt (5 tries, pure syscalls)
                    let mut reaped = false;
                    for _ in 0..5 {
                        let ret = unsafe {
                            libc::waitpid(child_pid, &mut status, libc::WNOHANG)
                        };
                        if ret == child_pid || ret == -1 {
                            reaped = true;
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    let _ = reaped;

                    unsafe { libc::close(pipe_read); }

                    // Attempt trace ONLY after returning from the critical path.
                    // If NVMe is unfrozen (child died cleanly), this works.
                    // If NVMe is frozen, this is a fire-and-forget attempt —
                    // we've already committed to returning Timeout.
                    let _ = std::thread::Builder::new()
                        .name("timeout-trace".into())
                        .spawn(move || {
                            // Best-effort trace on a throwaway thread.
                            // If this thread stalls on I/O, main thread is unaffected.
                            use std::io::Write;
                            if let Ok(mut f) = std::fs::OpenOptions::new()
                                .create(true).append(true)
                                .open("/var/lib/coralreef/traces/ember_fork_trace.log")
                            {
                                let ts = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default().as_millis();
                                let _ = writeln!(f,
                                    "[{ts}] TIMEOUT child_pid={child_pid} (zero-IO handler, no SBR)"
                                );
                            }
                        });

                    return ForkResult::Timeout;
                }

                std::thread::sleep(poll_interval);
            }
        }
    }
}

/// Parse a bridge BDF like "0000:00:01.3" into a CF8/CFC config address
/// for the dword containing PCI_BRIDGE_CONTROL (offset 0x3C).
/// Returns `None` if the bridge is not on bus 0 (I/O port config only
/// reaches bus 0 directly without Type 1 forwarding).
fn parse_bridge_io_port_config_addr(bridge_bdf: &str) -> Option<u32> {
    let parts: Vec<&str> = bridge_bdf.split(':').collect();
    if parts.len() != 3 { return None; }
    let bus = u8::from_str_radix(parts[1], 16).ok()?;
    if bus != 0 { return None; }
    let devfn: Vec<&str> = parts[2].split('.').collect();
    if devfn.len() != 2 { return None; }
    let dev = u8::from_str_radix(devfn[0], 16).ok()?;
    let func = u8::from_str_radix(devfn[1], 16).ok()?;
    Some(0x8000_0000 | (u32::from(dev) << 11) | (u32::from(func) << 8) | 0x3C)
}

/// Execute SBR using only CPU I/O port instructions.
/// `config_addr` is the CF8 address for the dword at offset 0x3C
/// (which contains Bridge Control at bytes 2-3).
/// Returns true if the SBR sequence completed.
///
/// This function does ZERO external I/O — no sysfs, no disk, no memory-mapped
/// PCIe. It uses only `in`/`out` x86 instructions through the CPU's I/O bus.
/// ioperm must have been acquired before calling this.
#[cfg(target_arch = "x86_64")]
fn inline_io_port_sbr(config_addr: u32) -> bool {
    unsafe {
        // Read current dword at offset 0x3C
        std::arch::asm!("out dx, eax", in("dx") 0xCF8u16, in("eax") config_addr, options(nomem, nostack));
        let dword: u32;
        std::arch::asm!("in eax, dx", out("eax") dword, in("dx") 0xCFCu16, options(nomem, nostack));

        // Assert SBR (bit 6 of Bridge Control = bit 22 of dword)
        std::arch::asm!("out dx, eax", in("dx") 0xCF8u16, in("eax") config_addr, options(nomem, nostack));
        let sbr_set = dword | (1 << 22);
        std::arch::asm!("out dx, eax", in("dx") 0xCFCu16, in("eax") sbr_set, options(nomem, nostack));
    }

    std::thread::sleep(Duration::from_millis(100));

    unsafe {
        // De-assert SBR
        std::arch::asm!("out dx, eax", in("dx") 0xCF8u16, in("eax") config_addr, options(nomem, nostack));
        let dword2: u32;
        std::arch::asm!("in eax, dx", out("eax") dword2, in("dx") 0xCFCu16, options(nomem, nostack));
        let sbr_clear = dword2 & !(1 << 22);
        std::arch::asm!("out dx, eax", in("dx") 0xCF8u16, in("eax") config_addr, options(nomem, nostack));
        std::arch::asm!("out dx, eax", in("dx") 0xCFCu16, in("eax") sbr_clear, options(nomem, nostack));
    }

    std::thread::sleep(Duration::from_millis(500));
    true
}

#[cfg(not(target_arch = "x86_64"))]
fn inline_io_port_sbr(_config_addr: u32) -> bool { false }

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
            // ZERO-I/O: no SBR, no sysfs, no blocking trace from the
            // watchdog thread. If the op stalled a core, any I/O risks
            // the same NBIO freeze cascade. Fire-and-forget trace only.
            let bdf_trace = bdf_owned.clone();
            let timeout_ms = timeout.as_millis();
            let _ = std::thread::Builder::new()
                .name("wd-trace".into())
                .spawn(move || {
                    tracing::error!(
                        bdf = %bdf_trace,
                        timeout_ms,
                        "MMIO WATCHDOG FIRED (zero-IO: no SBR attempted)"
                    );
                });
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
/// | `RegisterIo` | Fork isolation | preflight + circuit breaker |
/// | `BulkVram` | Fork isolation | AER mask + raw SBR + sequencer check |
/// | `EngineReset` | Fork isolation | AER mask + raw SBR + sequencing rules |
/// | `FalconBind` | Fork isolation | Full: AER + SBR + ITFEN + DMACTL |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationTier {
    /// Single register read/write. PCIe timeout returns 0xFFFF_FFFF.
    /// Uses fork isolation — thread watchdog cannot protect against
    /// core-level PCIe stalls that cascade through the AMD NBIO.
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
        true
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

// ─── GPU Decontamination ──────────────────────────────────────────────────

/// Decontaminate a GPU after an experiment that may have left internal state
/// dirty (e.g. falcon halted, PRI ring errors, FBIF misconfigured).
///
/// Uses a two-level strategy:
/// - **Level 1 (soft reset)**: Global PMC engine reset + PRI ring re-init +
///   PRAMIN write-readback canary. This resets all GPU engines without touching
///   the PCI layer, so ember's BAR0 mapping and VFIO device stay alive.
/// - **Level 2 (SBR)**: If soft reset fails (child times out), the fork parent
///   triggers PCIe SBR as a last resort.
pub fn decontaminate_gpu(
    bdf: &str,
    bar0_ptr: usize,
    bar0_size: usize,
) -> DecontaminateResult {
    tracing::info!(bdf, "GPU decontamination: soft reset (PMC + PRI re-init)");

    let bdf_owned = bdf.to_string();
    let fork_result = fork_isolated_mmio(
        &bdf_owned,
        Duration::from_secs(5),
        |pipe_fd| {
            let bar0 = unsafe {
                coral_driver::vfio::device::MappedBar::from_raw(
                    bar0_ptr as *mut u8,
                    bar0_size,
                )
            };

            let mut status = "unknown";

            // Step 1: Selective PMC engine reset — toggle ONLY falcon/graph
            // engines, never PFB (memory controller).
            //
            // Writing 0 to PMC_ENABLE would disable PFB, power-cycling the
            // HBM2 memory controller and losing its training state. After
            // restoring, the MC is degraded: single PRAMIN writes work but
            // bulk writes overwhelm the un-initialized write buffer and stall
            // at ~256 words, causing PCIe credit exhaustion and system lockup.
            //
            // GV100 PMC_ENABLE bit map (relevant):
            //   bit  8 = PFB/PFBSP — NEVER disable (memory controller)
            //   bit 12 = PGRAPH    — safe to reset
            //   bit 20 = SEC2      — safe to reset  
            //   bit 28 = CE        — safe to reset
            //   bit 29 = (varies)  — safe to reset
            const FALCON_RESET_MASK: u32 = (1 << 12) | (1 << 20) | (1 << 28) | (1 << 29);
            let pmc_orig = bar0.read_u32(0x200).unwrap_or(0);
            if pmc_orig != 0 && pmc_orig != 0xFFFF_FFFF {
                let pmc_partial = pmc_orig & !FALCON_RESET_MASK;
                let _ = bar0.write_u32(0x200, pmc_partial);
                std::thread::sleep(Duration::from_millis(20));
                let _ = bar0.write_u32(0x200, pmc_orig);
                std::thread::sleep(Duration::from_millis(20));
            }

            // Step 2: PRI ring re-init. After global PMC reset, the PRI ring
            // master needs re-initialization to re-enumerate the ring.
            // 0x01 = init command, 0x02 = ack/drain.
            let _ = bar0.write_u32(0x12004C, 0x01);
            std::thread::sleep(Duration::from_millis(10));
            let _ = bar0.write_u32(0x12004C, 0x02);
            std::thread::sleep(Duration::from_millis(5));

            // Step 3: BOOT0 liveness check
            let boot0 = bar0.read_u32(0x000).unwrap_or(0xDEAD_DEAD);
            let pmc_after = bar0.read_u32(0x200).unwrap_or(0);

            if boot0 == 0xDEAD_DEAD || boot0 == 0xFFFF_FFFF {
                status = "dead";
            } else if boot0 & 0xFFF0_0000 == 0xBAD0_0000 {
                status = "pri_bad";
            } else {
                // Step 4: PRAMIN canary — verify the write path is functional.
                // Set BAR0_WINDOW to page 0, write a canary, read it back.
                let saved_win = bar0.read_u32(0x1700).unwrap_or(0);
                let _ = bar0.write_u32(0x1700, 0x0000_0001_u32);
                let original = bar0.read_u32(0x0070_0000).unwrap_or(0xDEAD_DEAD);
                let canary = 0xDEC0_CAFE_u32;
                let _ = bar0.write_u32(0x0070_0000, canary);
                let _ = bar0.read_u32(0x000); // flush posted write
                let readback = bar0.read_u32(0x0070_0000).unwrap_or(0xDEAD_DEAD);
                let _ = bar0.write_u32(0x0070_0000, original); // restore
                let _ = bar0.write_u32(0x1700, saved_win);

                if readback == canary {
                    status = "clean";
                } else {
                    status = "pramin_stall";
                }
            }

            let json = serde_json::json!({
                "status": status,
                "boot0": boot0,
                "pmc": pmc_after,
            });
            if let Ok(bytes) = serde_json::to_vec(&json) {
                unsafe {
                    libc::write(pipe_fd, bytes.as_ptr().cast(), bytes.len());
                }
            }
            std::mem::forget(bar0);
        },
    );

    match fork_result {
        ForkResult::Ok(pipe_data) => {
            let parsed: serde_json::Value =
                serde_json::from_slice(&pipe_data).unwrap_or_default();
            let status = parsed
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let boot0 = parsed.get("boot0").and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
            let pmc = parsed.get("pmc").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

            tracing::info!(
                bdf,
                boot0 = format_args!("{boot0:#010x}"),
                pmc = format_args!("{pmc:#010x}"),
                status,
                "GPU soft-reset decontamination complete"
            );

            match status {
                "clean" => DecontaminateResult::Clean,
                "pramin_stall" => {
                    tracing::warn!(bdf, "PRAMIN canary failed — device degraded (needs warm cycle, zero-IO from parent)");
                    DecontaminateResult::SbrTriggered
                }
                _ => DecontaminateResult::StillDirty,
            }
        }
        ForkResult::Timeout => {
            tracing::warn!(
                bdf,
                "GPU soft-reset child timed out (zero-IO: no SBR from parent)"
            );
            DecontaminateResult::SbrTriggered
        }
        _ => {
            tracing::error!(bdf, "GPU decontamination fork failed");
            DecontaminateResult::StillDirty
        }
    }
}

/// Result of a GPU decontamination attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecontaminateResult {
    /// Soft reset succeeded: all engines reset, PRI ring re-inited, and
    /// PRAMIN write-readback canary verified. Safe for next experiment.
    Clean,
    /// Soft reset was insufficient. SBR fallback fired. Ember's BAR0
    /// mapping may be invalid — device needs re-init and warm cycle.
    SbrTriggered,
    /// Decontamination failed entirely — SBR not possible or fork failed.
    StillDirty,
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
        assert!(tier.requires_fork_isolation());
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
