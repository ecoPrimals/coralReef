// SPDX-License-Identifier: AGPL-3.0-or-later

use super::VbiosInterpreter;

impl VbiosInterpreter<'_> {
    pub(super) fn bar0_rd32(&mut self, reg: u32) -> u32 {
        let r = reg as usize;
        if !r.is_multiple_of(4) || r >= 0x0100_0000 {
            return 0xDEAD_DEAD;
        }
        let val = self.bar0.read_u32(r).unwrap_or(0xDEAD_DEAD);

        if crate::vfio::channel::registers::pri::is_pri_error(val) {
            self.stats.pri_faults += 1;
            self.pri_consecutive_faults += 1;
            let domain = crate::vfio::channel::registers::pri::domain_name(r).to_string();
            *self.pri_domain_faults.entry(domain).or_insert(0) += 1;

            if self.pri_consecutive_faults >= self.pri_fault_threshold {
                self.attempt_pri_recovery();
            }
        } else {
            self.pri_consecutive_faults = 0;
        }

        val
    }

    pub(super) fn bar0_wr32(&mut self, reg: u32, val: u32) {
        let r = reg as usize;
        if !self.execute || r >= 0x0100_0000 || !r.is_multiple_of(4) {
            return;
        }

        // Backpressure check: skip writes to domains with 3+ consecutive faults
        let domain = crate::vfio::channel::registers::pri::domain_name(r).to_string();
        if let Some(&faults) = self.pri_domain_faults.get(&domain)
            && faults >= 3
        {
            self.stats.writes_skipped_pri += 1;
            return;
        }

        // Backpressure check: if bus is heavily faulted, pause before writing
        if self.pri_consecutive_faults >= self.pri_fault_threshold * 2 {
            self.stats.writes_skipped_pri += 1;
            return;
        }

        let _ = self.bar0.write_u32(r, val);
        self.stats.writes_applied += 1;
    }

    pub(super) fn bar0_mask(&mut self, reg: u32, mask: u32, val: u32) -> u32 {
        let cur = self.bar0_rd32(reg);
        if crate::vfio::channel::registers::pri::is_pri_error(cur) {
            return cur;
        }
        self.bar0_wr32(reg, (cur & mask) | val);
        cur
    }

    /// Attempt to clear PRI bus faults and resume operations.
    pub(super) fn attempt_pri_recovery(&mut self) {
        self.stats.pri_recoveries += 1;

        // Ack PRIV_RING faults
        let _ = self.bar0.write_u32(
            crate::vfio::channel::registers::pri::PRIV_RING_COMMAND,
            crate::vfio::channel::registers::pri::PRIV_RING_CMD_ACK,
        );

        // Clear PMC INTR PRIV_RING bit
        let pmc_intr = self
            .bar0
            .read_u32(crate::vfio::channel::registers::pri::PMC_INTR)
            .unwrap_or(0);
        if pmc_intr & crate::vfio::channel::registers::pri::PMC_INTR_PRIV_RING_BIT != 0 {
            let _ = self.bar0.write_u32(
                crate::vfio::channel::registers::pri::PMC_INTR,
                crate::vfio::channel::registers::pri::PMC_INTR_PRIV_RING_BIT,
            );
        }

        std::thread::sleep(std::time::Duration::from_millis(10));
        self.pri_consecutive_faults = 0;

        // Re-probe: if BOOT0 reads clean, reset domain faults
        let boot0 = self.bar0.read_u32(0).unwrap_or(0xFFFF_FFFF);
        if !crate::vfio::channel::registers::pri::is_pri_error(boot0) && boot0 != 0xFFFF_FFFF {
            self.pri_domain_faults.clear();
        }
    }
}
