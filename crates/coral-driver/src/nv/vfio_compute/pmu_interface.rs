// SPDX-License-Identifier: AGPL-3.0-only
//! PMU falcon mailbox interface — firmware-agnostic command/response protocol.
//!
//! The PMU (Power Management Unit) falcon is the GPU's internal "BIOS" controller.
//! On GV100, it manages clock gating, power sequencing, PRI access gates, and the
//! secure boot chain. Unlike FECS/GPCCS (which halt when idle), the PMU stays alive
//! in a WAITING state after nouveau loads its firmware.
//!
//! # Protocol
//!
//! GV100 uses a register-based mailbox protocol:
//!
//! ```text
//! Host → PMU:
//!   1. Write command word to MAILBOX0
//!   2. Optionally write argument to MAILBOX1
//!   3. Raise host→falcon IRQ via IRQSSET (bit 6 = EXT_MBOX0)
//!
//! PMU → Host:
//!   1. PMU writes response to MAILBOX0/MAILBOX1
//!   2. PMU raises falcon→host IRQ (visible in IRQSTAT)
//!   3. Host polls MAILBOX0 for response or checks IRQSTAT
//! ```
//!
//! Newer architectures (Turing+, Ampere+) use queue-based RPC through DMEM.
//! This module currently targets the GV100 register-based protocol but is
//! designed to be extended for queue-based transports.
//!
//! # Usage
//!
//! ```text
//! let pmu = PmuInterface::attach(&bar0)?;
//! println!("PMU state: {:?}", pmu.state());
//!
//! // Send a command and wait for response
//! let resp = pmu.mailbox_exchange(&bar0, 0x1234, Some(0x5678), Duration::from_millis(100))?;
//! ```

use std::time::{Duration, Instant};

use crate::error::{DriverError, DriverResult};
use crate::vfio::channel::registers::falcon;
use crate::vfio::device::MappedBar;

/// PMU falcon runtime state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PmuState {
    /// PMU is actively executing firmware.
    Running,
    /// PMU is idle, waiting for work (bit 5 of CPUCTL). This is the normal
    /// state when nouveau's PMU firmware is loaded and idle.
    Waiting,
    /// PMU has halted (bit 4 of CPUCTL). Firmware exited or faulted.
    Halted,
    /// PMU is in hard reset (both bits 4+5 set).
    Reset,
    /// PMU registers are inaccessible (PRI gated, returns 0xBADxxxxx).
    Inaccessible,
}

impl std::fmt::Display for PmuState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Running => write!(f, "RUNNING"),
            Self::Waiting => write!(f, "WAITING"),
            Self::Halted => write!(f, "HALTED"),
            Self::Reset => write!(f, "RESET"),
            Self::Inaccessible => write!(f, "INACCESSIBLE"),
        }
    }
}

/// PMU mailbox snapshot — captures the full register state at a point in time.
#[derive(Debug, Clone)]
pub struct PmuSnapshot {
    /// CPUCTL register raw value.
    pub cpuctl: u32,
    /// MAILBOX0 register value.
    pub mailbox0: u32,
    /// MAILBOX1 register value.
    pub mailbox1: u32,
    /// IRQSTAT register value (pending interrupts).
    pub irqstat: u32,
    /// Program counter snapshot.
    pub pc: u32,
    /// Decoded PMU state.
    pub state: PmuState,
}

/// Response from a mailbox exchange.
#[derive(Debug, Clone)]
pub struct MailboxResponse {
    /// MAILBOX0 value after the exchange completed.
    pub mbox0: u32,
    /// MAILBOX1 value after the exchange completed.
    pub mbox1: u32,
    /// Time elapsed waiting for the response.
    pub elapsed: Duration,
}

/// PMU falcon mailbox interface.
///
/// Encapsulates the register-based mailbox protocol for communicating with
/// the PMU firmware via BAR0 MMIO. The interface is firmware-agnostic: it
/// handles the transport (register read/write, IRQ signaling, polling) while
/// the caller defines the command semantics.
///
/// Analogous to how `toadStool` interfaces with platform firmware agnostic
/// of the specific BIOS/UEFI implementation.
pub struct PmuInterface {
    /// Cached PMU state from last probe.
    state: PmuState,
    /// Whether the queue-based protocol is available (Turing+).
    queues_available: bool,
}

/// IRQ bit for host→falcon mailbox 0 notification.
/// From envytools: EXT_MBOX0 is typically bit 6 on falcon interrupt routing.
const IRQSSET_EXT_MBOX0: u32 = 1 << 6;

impl PmuInterface {
    /// Attach to the PMU falcon by probing its state via BAR0.
    ///
    /// Returns `Ok(Self)` if the PMU is reachable, or an error if the
    /// PMU registers are inaccessible (PRI gated).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the PMU falcon is inaccessible.
    pub fn attach(bar0: &MappedBar) -> DriverResult<Self> {
        let state = Self::probe_state(bar0);
        if state == PmuState::Inaccessible {
            return Err(DriverError::IoctlFailed {
                name: "pmu_attach",
                errno: libc_errno::ENODEV,
            });
        }

        let queues_available = Self::probe_queues(bar0);

        tracing::info!(
            state = %state,
            queues = queues_available,
            "PMU interface attached"
        );

        Ok(Self {
            state,
            queues_available,
        })
    }

    /// Current PMU state (cached from last probe or exchange).
    #[must_use]
    pub const fn state(&self) -> PmuState {
        self.state
    }

    /// Whether queue-based RPC is available (Turing+ firmware).
    #[must_use]
    pub const fn queues_available(&self) -> bool {
        self.queues_available
    }

    /// Re-probe PMU state from hardware.
    pub fn refresh(&mut self, bar0: &MappedBar) -> PmuState {
        self.state = Self::probe_state(bar0);
        self.state
    }

    /// Take a full snapshot of PMU registers.
    #[must_use]
    pub fn snapshot(bar0: &MappedBar) -> PmuSnapshot {
        let r = |offset: usize| bar0.read_u32(falcon::PMU_BASE + offset).unwrap_or(0xDEAD_DEAD);
        let cpuctl = r(falcon::CPUCTL);
        PmuSnapshot {
            cpuctl,
            mailbox0: r(falcon::MAILBOX0),
            mailbox1: r(falcon::MAILBOX1),
            irqstat: r(falcon::IRQSTAT),
            pc: r(falcon::PC),
            state: Self::decode_cpuctl(cpuctl),
        }
    }

    /// Read MAILBOX0 register.
    #[must_use]
    pub fn read_mbox0(bar0: &MappedBar) -> u32 {
        bar0.read_u32(falcon::PMU_BASE + falcon::MAILBOX0)
            .unwrap_or(0xDEAD_DEAD)
    }

    /// Read MAILBOX1 register.
    #[must_use]
    pub fn read_mbox1(bar0: &MappedBar) -> u32 {
        bar0.read_u32(falcon::PMU_BASE + falcon::MAILBOX1)
            .unwrap_or(0xDEAD_DEAD)
    }

    /// Write MAILBOX0 register.
    pub fn write_mbox0(bar0: &MappedBar, value: u32) -> DriverResult<()> {
        bar0.write_u32(falcon::PMU_BASE + falcon::MAILBOX0, value)
            .map_err(|_| DriverError::MmapFailed("PMU MBOX0 write failed".into()))
    }

    /// Write MAILBOX1 register.
    pub fn write_mbox1(bar0: &MappedBar, value: u32) -> DriverResult<()> {
        bar0.write_u32(falcon::PMU_BASE + falcon::MAILBOX1, value)
            .map_err(|_| DriverError::MmapFailed("PMU MBOX1 write failed".into()))
    }

    /// Raise a host→PMU interrupt to notify the firmware of a new command.
    ///
    /// Writes to IRQSSET to trigger the EXT_MBOX0 interrupt line,
    /// waking the PMU from its WAITING state to process the mailbox.
    pub fn raise_irq(bar0: &MappedBar) -> DriverResult<()> {
        bar0.write_u32(falcon::PMU_BASE + falcon::IRQSSET, IRQSSET_EXT_MBOX0)
            .map_err(|_| DriverError::MmapFailed("PMU IRQSSET write failed".into()))
    }

    /// Perform a mailbox command/response exchange.
    ///
    /// Writes `cmd` to MAILBOX0 (and optionally `arg` to MAILBOX1), raises
    /// the host→PMU interrupt, then polls MAILBOX0 until it changes or the
    /// timeout expires.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the PMU doesn't respond within `timeout`.
    pub fn mailbox_exchange(
        &mut self,
        bar0: &MappedBar,
        cmd: u32,
        arg: Option<u32>,
        timeout: Duration,
    ) -> DriverResult<MailboxResponse> {
        let initial_mbox0 = Self::read_mbox0(bar0);

        Self::write_mbox0(bar0, cmd)?;
        if let Some(a) = arg {
            Self::write_mbox1(bar0, a)?;
        }

        Self::raise_irq(bar0)?;

        let start = Instant::now();
        loop {
            if start.elapsed() >= timeout {
                self.state = Self::probe_state(bar0);
                return Err(DriverError::IoctlFailed {
                    name: "pmu_mailbox_timeout",
                    errno: libc_errno::ETIMEDOUT,
                });
            }

            let current = Self::read_mbox0(bar0);
            if current != cmd && current != initial_mbox0 {
                self.state = Self::probe_state(bar0);
                return Ok(MailboxResponse {
                    mbox0: current,
                    mbox1: Self::read_mbox1(bar0),
                    elapsed: start.elapsed(),
                });
            }

            std::thread::sleep(Duration::from_micros(50));
        }
    }

    /// Poll MAILBOX0 until a specific bit pattern appears or timeout expires.
    ///
    /// Useful for waiting on firmware status flags (e.g., devinit completion
    /// is signaled by bit 13 of MBOX0).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] on timeout.
    pub fn poll_mbox0_bits(
        bar0: &MappedBar,
        mask: u32,
        expected: u32,
        timeout: Duration,
    ) -> DriverResult<u32> {
        let start = Instant::now();
        loop {
            let val = Self::read_mbox0(bar0);
            if val & mask == expected {
                return Ok(val);
            }
            if start.elapsed() >= timeout {
                return Err(DriverError::IoctlFailed {
                    name: "pmu_poll_timeout",
                    errno: libc_errno::ETIMEDOUT,
                });
            }
            std::thread::sleep(Duration::from_micros(100));
        }
    }

    /// Probe the current PMU falcon state from CPUCTL.
    fn probe_state(bar0: &MappedBar) -> PmuState {
        let cpuctl = bar0
            .read_u32(falcon::PMU_BASE + falcon::CPUCTL)
            .unwrap_or(0xDEAD_DEAD);
        Self::decode_cpuctl(cpuctl)
    }

    /// Decode CPUCTL register bits into a [`PmuState`].
    fn decode_cpuctl(cpuctl: u32) -> PmuState {
        if cpuctl & 0xBAD0_0000 == 0xBAD0_0000 {
            return PmuState::Inaccessible;
        }
        let bit4 = cpuctl & (1 << 4) != 0;
        let bit5 = cpuctl & (1 << 5) != 0;
        match (bit4, bit5) {
            (false, false) => PmuState::Running,
            (false, true) => PmuState::Waiting,
            (true, false) => PmuState::Halted,
            (true, true) => PmuState::Reset,
        }
    }

    /// Check if queue-based registers are accessible (Turing+ firmware).
    fn probe_queues(bar0: &MappedBar) -> bool {
        const QUEUE_HEAD_0: usize = falcon::PMU_BASE + 0x4A0;
        let val = bar0.read_u32(QUEUE_HEAD_0).unwrap_or(0xBADF_5040);
        val & 0xBAD0_0000 != 0xBAD0_0000
    }
}

/// Known errno values used by the interface, avoiding a libc dependency.
mod libc_errno {
    pub const ENODEV: i32 = 19;
    pub const ETIMEDOUT: i32 = 110;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pmu_state_display() {
        assert_eq!(PmuState::Running.to_string(), "RUNNING");
        assert_eq!(PmuState::Waiting.to_string(), "WAITING");
        assert_eq!(PmuState::Halted.to_string(), "HALTED");
        assert_eq!(PmuState::Reset.to_string(), "RESET");
        assert_eq!(PmuState::Inaccessible.to_string(), "INACCESSIBLE");
    }

    #[test]
    fn decode_cpuctl_states() {
        assert_eq!(PmuInterface::decode_cpuctl(0x00000000), PmuState::Running);
        assert_eq!(PmuInterface::decode_cpuctl(0x00000020), PmuState::Waiting);
        assert_eq!(PmuInterface::decode_cpuctl(0x00000010), PmuState::Halted);
        assert_eq!(PmuInterface::decode_cpuctl(0x00000030), PmuState::Reset);
        assert_eq!(
            PmuInterface::decode_cpuctl(0xBAD00200),
            PmuState::Inaccessible
        );
    }

    #[test]
    fn irqsset_bit_is_correct() {
        assert_eq!(IRQSSET_EXT_MBOX0, 0x40);
    }

    #[test]
    fn libc_errno_constants() {
        assert_eq!(libc_errno::ENODEV, 19);
        assert_eq!(libc_errno::ETIMEDOUT, 110);
    }
}
