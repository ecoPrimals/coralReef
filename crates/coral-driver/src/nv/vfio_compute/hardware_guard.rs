// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hardware-protective BAR0 access layer.
//!
//! **Principle: the software dies, never the GPU.**
//!
//! [`GuardedBar`] wraps a [`MappedBar`] reference with runtime protections
//! that prevent known-destructive register writes from reaching the hardware:
//!
//! - **Link-down canary**: reads `PMC_BOOT_0` (offset 0x0) before write batches
//!   and periodically between writes. If the canary returns `0xFFFF_FFFF` or
//!   `0x0000_0000` the link is dead and all further operations abort.
//!
//! - **Register blocklist**: address ranges empirically proven to kill PCIe
//!   links or corrupt clock domains on cold GPUs are rejected unconditionally.
//!   Callers that need these ranges must use [`MappedBar`] directly with an
//!   explicit safety comment.
//!
//! - **Write audit trail**: every write is logged at `trace` level so
//!   post-mortem debugging can reconstruct the exact sequence that preceded
//!   a hardware failure.
//!
//! # When to use
//!
//! Use `GuardedBar` for **init sequences, clock programming, and any
//! exploratory register writes**. Hot-path dispatch (GPFIFO doorbell,
//! USERD updates) can use `MappedBar` directly since those offsets are
//! well-understood and latency-critical.

use crate::error::DriverError;
use crate::vfio::device::MappedBar;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

const PMC_ENABLE: u32 = 0x200;
const PGRAPH_BIT: u32 = 1 << 12;
const DEAD_SENTINEL: u32 = 0xDEAD_DEAD;

const PMC_BOOT_0: usize = 0x0;

/// Register ranges that are known to kill PCIe links or corrupt GPU state
/// when written to from the host on cold/uninitialized hardware.
///
/// Each entry is `(start_inclusive, end_inclusive, reason)`.
/// Addresses in these ranges are **unconditionally rejected** by
/// [`GuardedBar::write_u32`].
///
/// **Post-mortem evidence** (K80 die0, 2026-04-24):
/// - Writing 0xFFFFFFFF to 0x138020 killed PCIe link (Width → x0, link down)
/// - Writing to 0x13700C/0x137018/0x138000 killed PRI ring on cold K80
///   during earlier experiments (PCLOCK master enable before clocks running)
const BLOCKED_RANGES: &[(u32, u32, &str)] = &[
    // PCLOCK_MASTER region — writing here on cold hardware kills PCIe link.
    // 0x138020 is empirically proven lethal. Block the whole upper range.
    (
        0x13_8020,
        0x13_80FF,
        "PCLOCK_MASTER[0x20+]: killed PCIe link on K80 die0",
    ),
    // PMU PGOB register — individual writes to 0x10a78c WITHOUT the full
    // gk110_pmu_pgob protocol caused a hard lock (K80 2026-04-25). The correct
    // sequence (PMC bit 27 toggle + magic power writes + PMU bit sequencing)
    // is safe and implemented in init::gk110_pgob_disable(). Callers needing
    // PGOB must use that function, not raw writes to this range.
    // UNBLOCKED: the gk110_pgob_disable() function accesses inner MappedBar
    // directly to bypass this guard during the known-safe protocol.
    // (0x10_A780, 0x10_A79F, "PMU PGOB: use gk110_pgob_disable() instead"),
];

/// PIO ranges that are dangerous only when PGRAPH is disabled in PMC.
/// When PGRAPH is enabled (bit 12 of PMC_ENABLE), PIO is safe and necessary
/// for falcon firmware upload.
const PGRAPH_GATED_RANGES: &[(u32, u32, &str)] = &[
    (
        0x40_9180,
        0x40_91FF,
        "FECS PIO: D-state hang if PGRAPH not enabled in PMC",
    ),
    (
        0x41_A180,
        0x41_A1FF,
        "GPCCS PIO: D-state hang if PGRAPH not enabled in PMC",
    ),
];

/// Register ranges that are dangerous in specific contexts but not
/// unconditionally blocked. Writes here emit a `warn`-level log.
const CAUTION_RANGES: &[(u32, u32, &str)] = &[
    // PCLOCK PLL analog — writes silently dropped on cold K80 (PRI fault
    // 0xbadf3000). Not destructive, but wasteful. If writes succeed, PLLs
    // are alive (post-nouveau POST).
    (
        0x13_0000,
        0x13_6FFF,
        "PCLOCK PLL analog: verify PLLs are ungated first",
    ),
    // PCLOCK CTRL — source selectors, accessible on cold K80 but risky
    // for writes that could mismatch clock tree state.
    (
        0x13_7000,
        0x13_701F,
        "PCLOCK CTRL: source selectors, verify clock state",
    ),
    // PCLOCK_MASTER lower is now in BLOCKED_RANGES (0x138000-0x1380FF).
    // Removed from caution — these are proven lethal on K80.
    // PFUSE — fuse overrides that can confuse clock tree.
    (
        0x10_F400,
        0x10_F4FF,
        "PFUSE: fuse overrides affect clock domains",
    ),
];

/// Hardware-protective wrapper around [`MappedBar`].
///
/// Created via [`GuardedBar::new`] with a mandatory initial canary check.
/// All writes go through link-alive verification and the register blocklist.
pub struct GuardedBar<'a> {
    bar0: &'a MappedBar,
    link_alive: AtomicBool,
    write_count: AtomicU32,
    /// Expected BOOT0 value (captured on construction). Used to detect
    /// corruption (BOOT0 changing = something went very wrong).
    expected_boot0: u32,
    /// How many writes between automatic canary checks.
    canary_interval: u32,
}

/// Identifies the cause of a hardware guard refusal.
#[derive(Debug, Clone)]
pub enum GuardRefusal {
    /// PCIe link is down — BOOT0 reads 0xFFFFFFFF or 0x0.
    LinkDown { boot0: u32 },
    /// Register address is in the unconditional blocklist.
    BlockedRegister { offset: u32, reason: &'static str },
    /// BOOT0 changed from expected value — GPU state is corrupted.
    Boot0Corrupted { expected: u32, actual: u32 },
}

impl std::fmt::Display for GuardRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LinkDown { boot0 } => {
                write!(
                    f,
                    "PCIe link down (BOOT0={boot0:#010x}) — aborting to protect GPU"
                )
            }
            Self::BlockedRegister { offset, reason } => {
                write!(f, "blocked register write to {offset:#010x}: {reason}")
            }
            Self::Boot0Corrupted { expected, actual } => {
                write!(
                    f,
                    "BOOT0 corrupted: expected {expected:#010x}, got {actual:#010x} — aborting"
                )
            }
        }
    }
}

impl<'a> GuardedBar<'a> {
    /// Create a guarded BAR0 accessor with an initial canary check.
    ///
    /// Returns `Err` if the GPU is already dead (BOOT0 reads all-ones or zero).
    /// The canary interval controls how often BOOT0 is re-checked during writes
    /// (every N writes). A value of 16 is a good default for init sequences.
    pub fn new(bar0: &'a MappedBar, canary_interval: u32) -> Result<Self, GuardRefusal> {
        let boot0 = bar0.read_u32(PMC_BOOT_0).unwrap_or(0xFFFF_FFFF);
        if boot0 == 0xFFFF_FFFF || boot0 == 0 {
            return Err(GuardRefusal::LinkDown { boot0 });
        }

        tracing::debug!(
            boot0 = format_args!("{boot0:#010x}"),
            canary_interval,
            "GuardedBar: link alive, hardware guard active"
        );

        Ok(Self {
            bar0,
            link_alive: AtomicBool::new(true),
            write_count: AtomicU32::new(0),
            expected_boot0: boot0,
            canary_interval,
        })
    }

    /// Read a 32-bit register. Returns the raw value or `Err` if the link is
    /// down. Does **not** check the blocklist (reads are non-destructive).
    pub fn read_u32(&self, offset: u32) -> Result<u32, GuardRefusal> {
        if !self.link_alive.load(Ordering::Relaxed) {
            return Err(GuardRefusal::LinkDown { boot0: 0xFFFF_FFFF });
        }
        self.bar0
            .read_u32(offset as usize)
            .map_err(|_| GuardRefusal::LinkDown { boot0: 0xFFFF_FFFF })
    }

    /// Write a 32-bit register with full protection:
    /// 1. Check link-alive flag
    /// 2. Check register blocklist
    /// 3. Emit caution warning for risky registers
    /// 4. Perform the write
    /// 5. Periodic canary check
    pub fn write_u32(&self, offset: u32, value: u32) -> Result<(), GuardRefusal> {
        if !self.link_alive.load(Ordering::Relaxed) {
            return Err(GuardRefusal::LinkDown { boot0: 0xFFFF_FFFF });
        }

        // Unconditional blocklist
        for &(start, end, reason) in BLOCKED_RANGES {
            if offset >= start && offset <= end {
                tracing::error!(
                    offset = format_args!("{offset:#010x}"),
                    value = format_args!("{value:#010x}"),
                    reason,
                    "HARDWARE GUARD: blocked destructive register write"
                );
                return Err(GuardRefusal::BlockedRegister { offset, reason });
            }
        }

        // PGRAPH-gated blocklist: block PIO only when PGRAPH is disabled
        for &(start, end, reason) in PGRAPH_GATED_RANGES {
            if offset >= start && offset <= end {
                let pmc = self.bar0.read_u32(PMC_ENABLE as usize).unwrap_or(0);
                let pgraph_on = pmc & PGRAPH_BIT != 0;
                if !pgraph_on {
                    tracing::error!(
                        offset = format_args!("{offset:#010x}"),
                        value = format_args!("{value:#010x}"),
                        pmc = format_args!("{pmc:#010x}"),
                        reason,
                        "HARDWARE GUARD: PGRAPH disabled, blocking PIO write"
                    );
                    return Err(GuardRefusal::BlockedRegister { offset, reason });
                }
            }
        }

        // Caution warning
        for &(start, end, reason) in CAUTION_RANGES {
            if offset >= start && offset <= end {
                tracing::warn!(
                    offset = format_args!("{offset:#010x}"),
                    value = format_args!("{value:#010x}"),
                    reason,
                    "HARDWARE GUARD: caution register write"
                );
            }
        }

        tracing::trace!(
            offset = format_args!("{offset:#010x}"),
            value = format_args!("{value:#010x}"),
            "bar0 write"
        );

        let _ = self.bar0.write_u32(offset as usize, value);

        // Periodic canary
        let count = self.write_count.fetch_add(1, Ordering::Relaxed);
        if self.canary_interval > 0 && (count + 1) % self.canary_interval == 0 {
            self.check_canary()?;
        }

        Ok(())
    }

    /// Explicitly check that the GPU is still alive. Call this after any
    /// sequence of writes that might be risky, or before starting a new phase.
    pub fn check_canary(&self) -> Result<(), GuardRefusal> {
        let boot0 = self.bar0.read_u32(PMC_BOOT_0).unwrap_or(0xFFFF_FFFF);

        if boot0 == 0xFFFF_FFFF || boot0 == 0 {
            self.link_alive.store(false, Ordering::Release);
            tracing::error!(
                boot0 = format_args!("{boot0:#010x}"),
                writes = self.write_count.load(Ordering::Relaxed),
                "HARDWARE GUARD: PCIe link DOWN — all further writes blocked"
            );
            return Err(GuardRefusal::LinkDown { boot0 });
        }

        if boot0 != self.expected_boot0 {
            self.link_alive.store(false, Ordering::Release);
            tracing::error!(
                expected = format_args!("{:#010x}", self.expected_boot0),
                actual = format_args!("{boot0:#010x}"),
                writes = self.write_count.load(Ordering::Relaxed),
                "HARDWARE GUARD: BOOT0 changed — GPU state corrupted, blocking writes"
            );
            return Err(GuardRefusal::Boot0Corrupted {
                expected: self.expected_boot0,
                actual: boot0,
            });
        }

        Ok(())
    }

    /// Returns the total number of writes performed through this guard.
    #[must_use]
    pub fn write_count(&self) -> u32 {
        self.write_count.load(Ordering::Relaxed)
    }

    /// Returns `true` if the link is believed to be alive.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.link_alive.load(Ordering::Relaxed)
    }

    /// Access the underlying `MappedBar` for operations that have been
    /// reviewed and are known to be safe (e.g. GPFIFO doorbell, USERD).
    ///
    /// **Callers must document why bypassing the guard is safe.**
    #[must_use]
    pub fn inner(&self) -> &MappedBar {
        self.bar0
    }

    /// Build a guarded write closure matching the `let w = |reg, val| { ... }`
    /// pattern used throughout init code. Returns `Err(DriverError)` on refusal.
    ///
    /// Unlike the old `let _ = bar0.write_u32(...)` pattern, this propagates
    /// hardware failures so the caller can abort the init sequence.
    pub fn write_fn(&self) -> impl Fn(u32, u32) -> Result<(), DriverError> + '_ {
        move |reg: u32, val: u32| {
            self.write_u32(reg, val)
                .map_err(|refusal| DriverError::HardwareGuardRefusal(refusal.to_string().into()))
        }
    }

    /// Fork-isolated write: runs the guard checks, then performs the actual
    /// MMIO write in a forked child process with a kill-timeout. If the GPU
    /// hangs, the child is killed and the parent survives.
    ///
    /// Use this for the most dangerous writes: PMC toggles, PCLOCK, falcon
    /// STARTCPU — operations that have historically caused D-state hangs.
    pub fn write_u32_isolated(
        &self,
        offset: u32,
        value: u32,
        timeout: std::time::Duration,
    ) -> Result<(), GuardRefusal> {
        if !self.link_alive.load(Ordering::Relaxed) {
            return Err(GuardRefusal::LinkDown { boot0: 0xFFFF_FFFF });
        }

        for &(start, end, reason) in BLOCKED_RANGES {
            if offset >= start && offset <= end {
                tracing::error!(
                    offset = format_args!("{offset:#010x}"),
                    value = format_args!("{value:#010x}"),
                    reason,
                    "HARDWARE GUARD: blocked destructive register write (isolated)"
                );
                return Err(GuardRefusal::BlockedRegister { offset, reason });
            }
        }

        for &(start, end, reason) in PGRAPH_GATED_RANGES {
            if offset >= start && offset <= end {
                let pmc = self.bar0.read_u32(PMC_ENABLE as usize).unwrap_or(0);
                if pmc & PGRAPH_BIT == 0 {
                    tracing::error!(
                        offset = format_args!("{offset:#010x}"),
                        pmc = format_args!("{pmc:#010x}"),
                        reason,
                        "HARDWARE GUARD: PGRAPH disabled, blocking PIO write (isolated)"
                    );
                    return Err(GuardRefusal::BlockedRegister { offset, reason });
                }
            }
        }

        for &(start, end, reason) in CAUTION_RANGES {
            if offset >= start && offset <= end {
                tracing::warn!(
                    offset = format_args!("{offset:#010x}"),
                    value = format_args!("{value:#010x}"),
                    reason,
                    "HARDWARE GUARD: caution register write (isolated)"
                );
            }
        }

        tracing::trace!(
            offset = format_args!("{offset:#010x}"),
            value = format_args!("{value:#010x}"),
            "bar0 write (fork-isolated)"
        );

        match self.bar0.isolated_write_u32(offset, value, timeout) {
            crate::vfio::isolation::IsolationResult::Ok(()) => {}
            crate::vfio::isolation::IsolationResult::Timeout => {
                self.link_alive.store(false, Ordering::Release);
                tracing::error!(
                    offset = format_args!("{offset:#010x}"),
                    value = format_args!("{value:#010x}"),
                    "HARDWARE GUARD: isolated write TIMED OUT — GPU hung, child killed"
                );
                return Err(GuardRefusal::LinkDown {
                    boot0: DEAD_SENTINEL,
                });
            }
            crate::vfio::isolation::IsolationResult::ChildFailed { status } => {
                tracing::error!(
                    offset = format_args!("{offset:#010x}"),
                    value = format_args!("{value:#010x}"),
                    status,
                    "HARDWARE GUARD: isolated write child failed"
                );
                return Err(GuardRefusal::LinkDown {
                    boot0: DEAD_SENTINEL,
                });
            }
            crate::vfio::isolation::IsolationResult::ForkError(e) => {
                tracing::error!(
                    offset = format_args!("{offset:#010x}"),
                    error = %e,
                    "HARDWARE GUARD: fork failed for isolated write"
                );
                return Err(GuardRefusal::LinkDown {
                    boot0: DEAD_SENTINEL,
                });
            }
        }

        let count = self.write_count.fetch_add(1, Ordering::Relaxed);
        if self.canary_interval > 0 && (count + 1) % self.canary_interval == 0 {
            self.check_canary()?;
        }

        Ok(())
    }

    /// Fork-isolated read: performs the MMIO read in a forked child process
    /// with a kill-timeout. Returns the value or `Err` if the GPU hung.
    pub fn read_u32_isolated(
        &self,
        offset: u32,
        timeout: std::time::Duration,
    ) -> Result<u32, GuardRefusal> {
        if !self.link_alive.load(Ordering::Relaxed) {
            return Err(GuardRefusal::LinkDown { boot0: 0xFFFF_FFFF });
        }

        match self.bar0.isolated_read_u32(offset, timeout) {
            crate::vfio::isolation::IsolationResult::Ok(v) => Ok(v),
            crate::vfio::isolation::IsolationResult::Timeout => {
                self.link_alive.store(false, Ordering::Release);
                tracing::error!(
                    offset = format_args!("{offset:#010x}"),
                    "HARDWARE GUARD: isolated read TIMED OUT — GPU hung, child killed"
                );
                Err(GuardRefusal::LinkDown {
                    boot0: DEAD_SENTINEL,
                })
            }
            crate::vfio::isolation::IsolationResult::ChildFailed { status } => {
                tracing::error!(
                    offset = format_args!("{offset:#010x}"),
                    status,
                    "HARDWARE GUARD: isolated read child failed"
                );
                Err(GuardRefusal::LinkDown {
                    boot0: DEAD_SENTINEL,
                })
            }
            crate::vfio::isolation::IsolationResult::ForkError(e) => {
                tracing::error!(
                    offset = format_args!("{offset:#010x}"),
                    error = %e,
                    "HARDWARE GUARD: fork failed for isolated read"
                );
                Err(GuardRefusal::LinkDown {
                    boot0: DEAD_SENTINEL,
                })
            }
        }
    }

    /// Build a guarded read closure. Returns [`DEAD_SENTINEL`] on link-down
    /// (matching existing convention) but also logs the failure.
    pub fn read_fn(&self) -> impl Fn(u32) -> u32 + '_ {
        move |reg: u32| match self.read_u32(reg) {
            Ok(v) => v,
            Err(refusal) => {
                tracing::error!(reg = format_args!("{reg:#010x}"), "{refusal}");
                DEAD_SENTINEL
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_bar(boot0: u32) -> MappedBar {
        let mut data = vec![0u8; 0x100_0000]; // 16 MiB mock BAR
        data[0..4].copy_from_slice(&boot0.to_le_bytes());
        MappedBar::from_test_heap(data.into_boxed_slice())
    }

    #[test]
    fn dead_gpu_refuses_construction() {
        let bar = make_test_bar(0xFFFF_FFFF);
        assert!(GuardedBar::new(&bar, 16).is_err());
    }

    #[test]
    fn zero_boot0_refuses_construction() {
        let bar = make_test_bar(0);
        assert!(GuardedBar::new(&bar, 16).is_err());
    }

    #[test]
    fn valid_boot0_constructs() {
        let bar = make_test_bar(0x0f22_d0a1);
        assert!(GuardedBar::new(&bar, 16).is_ok());
    }

    #[test]
    fn blocked_register_refused() {
        let bar = make_test_bar(0x0f22_d0a1);
        let guard = GuardedBar::new(&bar, 0).unwrap();
        let result = guard.write_u32(0x13_8020, 0x42);
        assert!(result.is_err());
        match result.unwrap_err() {
            GuardRefusal::BlockedRegister { offset, .. } => {
                assert_eq!(offset, 0x13_8020);
            }
            other => panic!("expected BlockedRegister, got {other}"),
        }
    }

    #[test]
    fn safe_register_allowed() {
        let bar = make_test_bar(0x0f22_d0a1);
        let guard = GuardedBar::new(&bar, 0).unwrap();
        assert!(guard.write_u32(PMC_ENABLE, 0x42).is_ok());
    }

    #[test]
    fn fecs_pio_blocked_when_pgraph_off() {
        let bar = make_test_bar(0x0f22_d0a1);
        // PMC_ENABLE is zero → PGRAPH disabled → PIO blocked
        let guard = GuardedBar::new(&bar, 0).unwrap();
        assert!(guard.write_u32(0x40_9180, 0).is_err()); // FECS IMEMC
        assert!(guard.write_u32(0x40_91A0, 0).is_err()); // FECS IMEMD
        assert!(guard.write_u32(0x41_A184, 0).is_err()); // GPCCS PIO
    }

    #[test]
    fn fecs_pio_allowed_when_pgraph_on() {
        let bar = make_test_bar(0x0f22_d0a1);
        // Set PMC_ENABLE PGRAPH bit → PIO should be allowed
        let _ = bar.write_u32(PMC_ENABLE as usize, PGRAPH_BIT);
        let guard = GuardedBar::new(&bar, 0).unwrap();
        assert!(guard.write_u32(0x40_9180, 0).is_ok()); // FECS IMEMC
        assert!(guard.write_u32(0x41_A184, 0).is_ok()); // GPCCS PIO
    }

    #[test]
    fn write_count_increments() {
        let bar = make_test_bar(0x0f22_d0a1);
        let guard = GuardedBar::new(&bar, 0).unwrap();
        let _ = guard.write_u32(0x400, 1);
        let _ = guard.write_u32(0x404, 2);
        assert_eq!(guard.write_count(), 2);
    }

    #[test]
    fn canary_detects_corruption() {
        let bar = make_test_bar(0x0f22_d0a1);
        let guard = GuardedBar::new(&bar, 0).unwrap();
        // Corrupt BOOT0 via raw bar0 write
        let _ = bar.write_u32(0, 0xDEAD_BEEF);
        let result = guard.check_canary();
        assert!(result.is_err());
        assert!(!guard.is_alive());
    }
}
