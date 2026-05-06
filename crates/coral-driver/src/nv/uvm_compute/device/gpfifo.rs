// SPDX-License-Identifier: AGPL-3.0-or-later
//! GPFIFO submission, completion polling, and Blackwell semaphore fences.

use crate::error::{DriverError, DriverResult};
use crate::mmio::VolatilePtr;

use super::super::types::{
    GPFIFO_ENTRIES, USERD_GP_GET_OFFSET, USERD_GP_PUT_OFFSET, gpfifo_entry, uvm_cache_line_flush,
};
use super::NvUvmComputeDevice;

impl NvUvmComputeDevice {
    /// Write a GPFIFO entry and ring the USERD doorbell.
    pub(in crate::nv::uvm_compute) fn submit_gpfifo(
        &mut self,
        push_buf_va: u64,
        length_dwords: u32,
    ) -> DriverResult<()> {
        let entry = gpfifo_entry(push_buf_va, length_dwords);
        let slot = (self.gp_put % GPFIFO_ENTRIES) as usize;
        let entry_offset = slot * 8;

        if self.gpfifo_cpu_addr == 0 {
            return Err(DriverError::SubmitFailed(
                "GPFIFO ring not CPU-mapped".into(),
            ));
        }

        let gpfifo_slot = (self.gpfifo_cpu_addr + entry_offset as u64) as *mut u64;
        // SAFETY: gpfifo_cpu_addr is a valid kernel mmap'd address (from rm_map_memory).
        // entry_offset < GPFIFO_SIZE is guaranteed by the modulo above.
        let vol = unsafe { VolatilePtr::new(gpfifo_slot) };
        vol.write(entry);

        self.gp_put = self.gp_put.wrapping_add(1);

        if self.userd_cpu_addr == 0 {
            return Err(DriverError::SubmitFailed("USERD not CPU-mapped".into()));
        }

        // Flush GPFIFO entry from CPU cache so GPU DMA sees it.
        // SAFETY: gpfifo_slot..+8 is within the valid GPFIFO mapping.
        unsafe {
            uvm_cache_line_flush(gpfifo_slot as *const u8);
        }

        let doorbell = (self.userd_cpu_addr + USERD_GP_PUT_OFFSET as u64) as *mut u32;
        // SAFETY: userd_cpu_addr is a valid kernel mmap'd address.
        // GP_PUT offset (0x8C) is within the 4096-byte USERD page.
        let vol = unsafe { VolatilePtr::new(doorbell) };
        vol.write(self.gp_put);

        // Flush USERD page from CPU cache so GPU sees GP_PUT update.
        // SAFETY: doorbell points within the valid USERD mapping.
        unsafe {
            uvm_cache_line_flush(doorbell as *const u8);
        }

        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

        // Ring the USERMODE doorbell to notify the GPU.
        // SAFETY: doorbell_addr is a valid mmap'd BAR0 USERMODE page.
        // Offset 0x90 = NV_USERMODE_NOTIFY_CHANNEL_PENDING.
        if self.doorbell_addr != 0 {
            let db = (self.doorbell_addr + 0x90) as *mut u32;
            unsafe { VolatilePtr::new(db).write(self.work_submit_token) };
        }

        tracing::debug!(
            gp_put = self.gp_put,
            push_buf_va = format_args!("0x{push_buf_va:016X}"),
            length_dwords,
            "GPFIFO entry submitted"
        );
        Ok(())
    }

    /// Poll for GPFIFO completion.
    ///
    /// On Volta-Hopper: reads `GP_GET` from the USERD page (GPU writes it).
    /// On Blackwell+: reads the semaphore fence value from system memory
    /// (GPU writes it via SEM_RELEASE in the push buffer). Blackwell removed
    /// `GP_GET` from the USERD control struct (clca6f: entire 0x00-0x8B is Ignored).
    pub(in crate::nv::uvm_compute) fn poll_gpfifo_completion(&self) -> DriverResult<()> {
        if self.uses_semaphore_fence {
            return self.poll_fence_completion();
        }

        if self.userd_cpu_addr == 0 || self.gp_put == 0 {
            return Ok(());
        }

        let gp_get_ptr = (self.userd_cpu_addr + USERD_GP_GET_OFFSET as u64) as *mut u32;

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            // SAFETY: `gp_get_ptr` points into the kernel-mapped USERD page (`userd_cpu_addr`);
            // cache flush matches write-back from GPU before the volatile GP_GET poll.
            unsafe { uvm_cache_line_flush(gp_get_ptr as *const u8) };
            // SAFETY: Same mapping; GP_GET lies within the USERD page; volatile read matches GPU DMA updates.
            let gp_get = unsafe { VolatilePtr::new(gp_get_ptr).read() };
            if gp_get >= self.gp_put {
                return Ok(());
            }
            if std::time::Instant::now() > deadline {
                let errnotif = self.read_error_notifier();
                return Err(DriverError::SubmitFailed(
                    format!(
                        "GPFIFO completion timeout: GP_GET={gp_get} GP_PUT={} errnotif=[{errnotif}]",
                        self.gp_put
                    )
                    .into(),
                ));
            }
            std::hint::spin_loop();
            std::thread::sleep(std::time::Duration::from_micros(10));
        }
    }

    /// Poll the semaphore fence for Blackwell+ completion.
    ///
    /// After the fence advances, we also check the error notifier — on
    /// Blackwell the fence release is a separate GPFIFO entry that the
    /// PBDMA may process even after the compute engine reports an error,
    /// which would make a failed dispatch appear to succeed.
    fn poll_fence_completion(&self) -> DriverResult<()> {
        if self.fence_cpu_addr == 0 || self.fence_value == 0 {
            return Ok(());
        }

        let fence_ptr = self.fence_cpu_addr as *mut u32;
        let expected = self.fence_value;

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            // SAFETY: `fence_cpu_addr` is RM/UVM mapped fence memory; flush before observing GPU semaphore writes.
            unsafe { uvm_cache_line_flush(fence_ptr as *const u8) };
            // SAFETY: Same mapping as above; dword at `fence_ptr` is the semaphore fence counter.
            let current = unsafe { VolatilePtr::new(fence_ptr).read() };
            if current >= expected {
                // Fence advanced — but check if an async error was reported.
                let errnotif = self.read_error_notifier();
                if errnotif.contains("status=0xFFFF") {
                    return Err(DriverError::SubmitFailed(
                        format!(
                            "Blackwell dispatch error (fence OK but errnotif set): \
                             fence={current} expected={expected} errnotif=[{errnotif}]"
                        )
                        .into(),
                    ));
                }
                return Ok(());
            }
            if std::time::Instant::now() > deadline {
                let errnotif = self.read_error_notifier();
                return Err(DriverError::SubmitFailed(
                    format!(
                        "Blackwell fence timeout: fence={current} expected={expected} errnotif=[{errnotif}]"
                    )
                    .into(),
                ));
            }
            std::hint::spin_loop();
            std::thread::sleep(std::time::Duration::from_micros(10));
        }
    }

    /// Read the GPU error notifier and return a diagnostic string.
    ///
    /// The NVIDIA error notifier is a 16-byte struct:
    /// - `[0:7]`  timestamp (nanoseconds)
    /// - `[8:11]` info32 (error-specific data)
    /// - `[12:13]` info16 (error-specific data)
    /// - `[14:15]` status (0 = OK, 0x8000+ = error)
    pub(super) fn read_error_notifier(&self) -> String {
        if self.errnotif_cpu_addr == 0 {
            return "error notifier not mapped".to_string();
        }
        let base = self.errnotif_cpu_addr as *const u32;
        // SAFETY: errnotif_cpu_addr is a valid 4096-byte mmap from RM.
        // We read 4 dwords (16 bytes) — one NvNotification entry.
        let (ts_lo, ts_hi, info32, status_word) = unsafe {
            (
                VolatilePtr::new(base.cast_mut()).read(),
                VolatilePtr::new(base.add(1).cast_mut()).read(),
                VolatilePtr::new(base.add(2).cast_mut()).read(),
                VolatilePtr::new(base.add(3).cast_mut()).read(),
            )
        };
        let info16 = status_word & 0xFFFF;
        let status = (status_word >> 16) & 0xFFFF;
        format!(
            "ts=0x{ts_hi:08X}_{ts_lo:08X} info32=0x{info32:08X} info16=0x{info16:04X} status=0x{status:04X}"
        )
    }

    /// Submit a semaphore release GPFIFO entry for Blackwell fence tracking.
    ///
    /// Uses the compute engine's `SET_REPORT_SEMAPHORE_A/B/C/D` methods
    /// (byte offsets 0x1B00–0x1B0C from `clcec0.h`) on **subchannel 0**
    /// where the compute class is bound via `SET_OBJECT`. The compute
    /// engine processes the RELEASE only after all prior dispatches on
    /// this subchannel have completed, providing a proper execution fence.
    pub(in crate::nv::uvm_compute) fn submit_fence_release(&mut self) -> DriverResult<()> {
        if !self.uses_semaphore_fence || self.fence_pb_cpu_addr == 0 {
            return Ok(());
        }

        self.fence_value += 1;
        let fv = self.fence_value;
        let fva = self.fence_gpu_va;
        let pb = self.fence_pb_cpu_addr as *mut u32;

        // SET_REPORT_SEMAPHORE_A (0x1B00): addr[39:32]
        // SET_REPORT_SEMAPHORE_B (0x1B04): addr[31:0]
        // SET_REPORT_SEMAPHORE_C (0x1B08): payload (32-bit)
        // SET_REPORT_SEMAPHORE_D (0x1B0C): OPERATION=RELEASE | STRUCTURE_SIZE=ONE_WORD
        //
        // D encoding: OPERATION[1:0]=0 (RELEASE), FLUSH_DISABLE[2]=0,
        //             STRUCTURE_SIZE[28]=1 (ONE_WORD) → 0x10000000
        let subchan = self.compute_subchannel;
        const REPORT_SEM_A: u32 = 0x1B00;
        const SEM_D_RELEASE_ONE_WORD: u32 = 1 << 28;

        // 5-dword push buffer: 1 header + 4 data words.
        // SAFETY: fence_pb_cpu_addr is a valid 4096-byte mmap. We write 20 bytes.
        unsafe {
            let header = (1_u32 << 29) | (4 << 16) | (subchan << 13) | (REPORT_SEM_A >> 2);
            VolatilePtr::new(pb).write(header);
            {
                VolatilePtr::new(pb.add(1)).write((fva >> 32) as u32);
                VolatilePtr::new(pb.add(2)).write((fva & 0xFFFF_FFFF) as u32);
            }
            VolatilePtr::new(pb.add(3)).write(fv);
            VolatilePtr::new(pb.add(4)).write(SEM_D_RELEASE_ONE_WORD);
            uvm_cache_line_flush(pb as *const u8);
        }

        self.submit_gpfifo(self.fence_pb_gpu_va, 5)?;
        tracing::debug!(
            fence_value = fv,
            "Blackwell fence release submitted (subchan 0, compute engine)"
        );
        Ok(())
    }
}
