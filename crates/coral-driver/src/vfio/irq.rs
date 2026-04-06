// SPDX-License-Identifier: AGPL-3.0-or-later

//! VFIO IRQ wiring — enable GPU-to-host interrupt notification via eventfd.
//!
//! PCI devices signal interrupts via MSI/MSI-X. VFIO routes these to an
//! `eventfd` that the host can poll/read to detect GPU-initiated events
//! (e.g. SEC2 MSGQ response ready, falcon halt, DMA completion).
//!
//! ## Protocol
//!
//! 1. Create an `eventfd(0, EFD_NONBLOCK)`.
//! 2. `VFIO_DEVICE_GET_IRQ_INFO` to discover IRQ count for INTX/MSI/MSIX.
//! 3. `VFIO_DEVICE_SET_IRQS` with `DATA_EVENTFD | ACTION_TRIGGER` to arm.
//! 4. `poll()` or `read(8)` on the eventfd to block until IRQ fires.
//! 5. After handling, optionally unmask INTX with `ACTION_UNMASK`.
//!
//! ## Nouveau reference
//!
//! `nouveau` uses MSI index 0 for all engine interrupts. The handler reads
//! `PMC_INTR(0)` at BAR0+0x000100 to dispatch to per-engine handlers.

use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd, RawFd};

use rustix::event::{EventfdFlags, PollFd, PollFlags, eventfd, poll};
use rustix::time::Timespec;

use crate::error::DriverError;
use crate::vfio::ioctl;

/// VFIO IRQ index constants (from `<linux/vfio.h>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum VfioIrqIndex {
    /// PCI INTX (legacy INTx pin interrupt).
    Intx = 0,
    /// PCI MSI.
    Msi = 1,
    /// PCI MSI-X.
    Msix = 2,
    /// Error reporting interrupt.
    Err = 3,
    /// Device-level request interrupt.
    Req = 4,
}

/// VFIO IRQ info for one index (from `VFIO_DEVICE_GET_IRQ_INFO`).
#[derive(Debug, Clone)]
pub struct VfioIrqInfo {
    /// IRQ index queried.
    pub index: VfioIrqIndex,
    /// Number of IRQ vectors at this index.
    pub count: u32,
    /// Capability flags.
    pub flags: u32,
}

/// VFIO IRQ SET flags (from `<linux/vfio.h>`).
const VFIO_IRQ_SET_DATA_EVENTFD: u32 = 1 << 2;
const VFIO_IRQ_SET_ACTION_TRIGGER: u32 = 1 << 5;

/// Kernel ABI struct for `VFIO_DEVICE_GET_IRQ_INFO`.
#[repr(C)]
#[derive(Debug, Default)]
struct VfioIrqInfoRaw {
    argsz: u32,
    flags: u32,
    index: u32,
    count: u32,
}

/// Kernel ABI struct for `VFIO_DEVICE_SET_IRQS` with a single eventfd.
#[repr(C)]
struct VfioIrqSetEventfd {
    argsz: u32,
    flags: u32,
    index: u32,
    start: u32,
    count: u32,
    fd: i32,
}

/// Query IRQ info for a specific index.
pub fn get_irq_info(
    device_fd: BorrowedFd<'_>,
    index: VfioIrqIndex,
) -> Result<VfioIrqInfo, DriverError> {
    let mut info = VfioIrqInfoRaw {
        argsz: std::mem::size_of::<VfioIrqInfoRaw>() as u32,
        index: index as u32,
        ..Default::default()
    };
    ioctl::device_get_irq_info(device_fd, &mut info)?;
    Ok(VfioIrqInfo {
        index,
        count: info.count,
        flags: info.flags,
    })
}

/// Arm a VFIO IRQ to trigger on a newly created eventfd.
///
/// Returns the eventfd that will be signaled when the IRQ fires.
pub fn arm_irq_eventfd(
    device_fd: BorrowedFd<'_>,
    index: VfioIrqIndex,
    vector: u32,
) -> Result<OwnedFd, DriverError> {
    let efd = eventfd(0, EventfdFlags::NONBLOCK)
        .map_err(|e| DriverError::DeviceNotFound(format!("eventfd: {e}").into()))?;

    let mut set = VfioIrqSetEventfd {
        argsz: std::mem::size_of::<VfioIrqSetEventfd>() as u32,
        flags: VFIO_IRQ_SET_DATA_EVENTFD | VFIO_IRQ_SET_ACTION_TRIGGER,
        index: index as u32,
        start: vector,
        count: 1,
        fd: efd.as_raw_fd(),
    };

    ioctl::device_set_irqs(device_fd, &mut set)?;

    tracing::info!(?index, vector, "VFIO IRQ armed on eventfd");

    Ok(efd)
}

/// A wired VFIO interrupt — holds the eventfd and provides wait/poll.
pub struct VfioIrq {
    eventfd: OwnedFd,
    index: VfioIrqIndex,
    vector: u32,
    fires: u64,
}

impl VfioIrq {
    /// Arm a VFIO device IRQ and return a handle for waiting.
    pub fn arm(
        device_fd: BorrowedFd<'_>,
        index: VfioIrqIndex,
        vector: u32,
    ) -> Result<Self, DriverError> {
        let eventfd = arm_irq_eventfd(device_fd, index, vector)?;
        Ok(Self {
            eventfd,
            index,
            vector,
            fires: 0,
        })
    }

    /// Non-blocking check: has the IRQ fired since last read?
    pub fn poll_irq(&mut self) -> bool {
        let mut buf = [0u8; 8];
        match rustix::io::read(&self.eventfd, &mut buf) {
            Ok(8) => {
                let count = u64::from_ne_bytes(buf);
                self.fires += count;
                true
            }
            _ => false,
        }
    }

    /// Blocking wait for the next IRQ (with timeout in ms, -1 for infinite).
    ///
    /// Returns `true` if an IRQ fired, `false` on timeout.
    pub fn wait(&mut self, timeout_ms: i32) -> bool {
        let mut pfds = [PollFd::new(&self.eventfd, PollFlags::IN)];
        let timeout = if timeout_ms < 0 {
            None
        } else {
            Some(Timespec {
                tv_sec: (timeout_ms / 1000) as i64,
                tv_nsec: ((timeout_ms % 1000) as i64) * 1_000_000,
            })
        };
        match poll(&mut pfds, timeout.as_ref()) {
            Ok(n) if n > 0 => self.poll_irq(),
            _ => false,
        }
    }

    /// Total number of IRQ fires observed.
    pub fn fire_count(&self) -> u64 {
        self.fires
    }

    /// The raw eventfd for integration with external epoll loops.
    pub fn eventfd_raw(&self) -> RawFd {
        self.eventfd.as_raw_fd()
    }

    /// IRQ index this handle is wired to.
    pub fn index(&self) -> VfioIrqIndex {
        self.index
    }

    /// Vector within the IRQ index.
    pub fn vector(&self) -> u32 {
        self.vector
    }
}

impl AsFd for VfioIrq {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.eventfd.as_fd()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn irq_index_repr_matches_kernel_abi() {
        assert_eq!(VfioIrqIndex::Intx as u32, 0);
        assert_eq!(VfioIrqIndex::Msi as u32, 1);
        assert_eq!(VfioIrqIndex::Msix as u32, 2);
        assert_eq!(VfioIrqIndex::Err as u32, 3);
        assert_eq!(VfioIrqIndex::Req as u32, 4);
    }

    #[test]
    fn irq_info_raw_layout() {
        assert!(std::mem::size_of::<VfioIrqInfoRaw>() >= 16);
        assert_eq!(std::mem::align_of::<VfioIrqInfoRaw>(), 4);
    }

    #[test]
    fn irq_set_eventfd_layout() {
        assert!(std::mem::size_of::<VfioIrqSetEventfd>() >= 24);
        assert_eq!(std::mem::align_of::<VfioIrqSetEventfd>(), 4);
    }

    #[test]
    fn irq_set_flags_match_vfio_h() {
        assert_eq!(VFIO_IRQ_SET_DATA_EVENTFD, 1 << 2);
        assert_eq!(VFIO_IRQ_SET_ACTION_TRIGGER, 1 << 5);
        assert_eq!(
            VFIO_IRQ_SET_DATA_EVENTFD | VFIO_IRQ_SET_ACTION_TRIGGER,
            0x24
        );
    }

    #[test]
    fn irq_info_construction() {
        let info = VfioIrqInfo {
            index: VfioIrqIndex::Msi,
            count: 32,
            flags: 0x07,
        };
        assert_eq!(info.index, VfioIrqIndex::Msi);
        assert_eq!(info.count, 32);
        assert_eq!(info.flags, 0x07);
    }

    #[test]
    fn irq_index_debug_format() {
        assert_eq!(format!("{:?}", VfioIrqIndex::Intx), "Intx");
        assert_eq!(format!("{:?}", VfioIrqIndex::Msix), "Msix");
    }

    #[test]
    fn irq_index_equality() {
        assert_eq!(VfioIrqIndex::Msi, VfioIrqIndex::Msi);
        assert_ne!(VfioIrqIndex::Msi, VfioIrqIndex::Msix);
    }

    #[test]
    fn irq_info_raw_default() {
        let raw = VfioIrqInfoRaw::default();
        assert_eq!(raw.argsz, 0);
        assert_eq!(raw.flags, 0);
        assert_eq!(raw.index, 0);
        assert_eq!(raw.count, 0);
    }
}
