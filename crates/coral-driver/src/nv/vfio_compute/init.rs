// SPDX-License-Identifier: AGPL-3.0-only
//! VFIO GR init — BAR0 register writes and FECS channel methods.

use crate::ComputeDevice;
use crate::error::{DriverError, DriverResult};
use crate::gsp::{self, GrFirmwareBlobs, GrInitSequence};
use crate::vfio::device::MappedBar;

use super::super::pushbuf::PushBuf;
use super::{NvVfioComputeDevice, sm_to_chip};

/// Returns true if `val` matches an NVIDIA PRI ring error pattern.
/// These values appear when the target engine is unreachable (not POST-ed,
/// clock-gated, or PRI ring corrupted). Must not be treated as valid data.
fn is_pri_fault_value(val: u32) -> bool {
    crate::vfio::channel::registers::pri::is_pri_error(val)
}

impl NvVfioComputeDevice {
    /// Apply BAR0 GR init writes from NVIDIA firmware blobs.
    ///
    /// Parses `sw_bundle_init.bin` etc. from `/lib/firmware/nvidia/{chip}/gr/`,
    /// builds the init sequence, then applies the BAR0-targeted writes
    /// (PMC engine enable, FIFO enable, PGRAPH register programming).
    pub(super) fn apply_gr_bar0_init(bar0: &MappedBar, sm_version: u32) {
        let chip = sm_to_chip(sm_version);
        let blobs = match GrFirmwareBlobs::parse(chip) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(chip, error = %e, "GR firmware not available — skipping BAR0 GR init");
                return;
            }
        };

        let seq = if sm_version == 70 {
            GrInitSequence::for_gv100(&blobs)
        } else {
            GrInitSequence::from_blobs(&blobs)
        };

        let (bar0_writes, fecs_entries) = gsp::split_for_application(&seq);

        tracing::info!(
            chip,
            bar0_writes = bar0_writes.len(),
            fecs_entries = fecs_entries.len(),
            total = seq.len(),
            "sovereign VFIO GR init: applying {} BAR0 register writes",
            bar0_writes.len()
        );

        // Only apply writes with 4-byte-aligned offsets that fit within BAR0.
        let bar0_size = bar0.size() as u32;
        let writes: Vec<(u32, u32)> = bar0_writes
            .iter()
            .filter(|w| {
                if w.offset % 4 != 0 {
                    tracing::debug!(
                        chip,
                        offset = format!("{:#010x}", w.offset),
                        "skipping non-aligned BAR0 write"
                    );
                    return false;
                }
                if w.offset + 4 > bar0_size {
                    tracing::debug!(
                        chip,
                        offset = format!("{:#010x}", w.offset),
                        bar0_size = format!("{bar0_size:#010x}"),
                        "skipping out-of-range BAR0 write"
                    );
                    return false;
                }
                true
            })
            .map(|w| (w.offset, w.value))
            .collect();

        let (applied, failed) = bar0.apply_gr_bar0_writes(&writes);

        if failed > 0 {
            tracing::warn!(chip, applied, failed, "BAR0 GR init had write failures");
        } else {
            tracing::info!(chip, applied, "BAR0 GR init complete");
        }

        // Brief settle after engine enable writes.
        for w in &bar0_writes {
            if w.delay_us > 0 {
                std::thread::sleep(std::time::Duration::from_micros(u64::from(w.delay_us)));
            }
        }

        // Apply sw_nonctx.bin — GR engine MMIO configuration that FECS expects.
        // These are PGRAPH/GPC/TPC register writes (0x40xxxx-0x41xxxx) that
        // nouveau applies via gf100_gr_mmio(gr, gr->sw_nonctx) BEFORE
        // booting the falcons. Without these, FECS stalls during init.
        Self::apply_nonctx_writes(bar0, &blobs, chip);

        // Apply dynamic GR writes that depend on hardware register reads.
        // These can't be in sw_nonctx.bin since they're computed at runtime.
        Self::apply_dynamic_gr_init(bar0, sm_version);
    }

    /// Apply non-context register writes from `sw_nonctx.bin`.
    ///
    /// The file is packed u32 pairs `(BAR0_addr, value)`. These configure
    /// the GR engine (PGRAPH, GPC, TPC, SM registers) to the state that
    /// FECS firmware expects before it can initialize its command loop.
    fn apply_nonctx_writes(bar0: &MappedBar, blobs: &GrFirmwareBlobs, chip: &str) {
        if blobs.nonctx_data.is_empty() {
            tracing::debug!(chip, "no sw_nonctx data — skipping");
            return;
        }

        let bar0_size = bar0.size() as u32;
        let mut applied = 0u32;
        let mut skipped = 0u32;
        let data = &blobs.nonctx_data;

        for chunk in data.chunks_exact(8) {
            let addr = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            let value = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);

            if addr % 4 != 0 || addr + 4 > bar0_size {
                skipped += 1;
                continue;
            }

            if bar0.write_u32(addr as usize, value).is_ok() {
                applied += 1;
            } else {
                skipped += 1;
            }
        }

        tracing::info!(
            chip,
            applied,
            skipped,
            total = data.len() / 8,
            "sw_nonctx GR MMIO init complete"
        );
    }

    /// Apply dynamic GR init writes computed from hardware registers.
    ///
    /// Nouveau computes these at runtime in `gf100_gr_init()`:
    /// - GPC MMU addresses from PFB registers
    /// - Active LTC/FBP counts
    /// - SWDX PES mask from GPC topology
    /// - Interrupt/trap enables
    fn apply_dynamic_gr_init(bar0: &MappedBar, sm_version: u32) {
        let r = |addr: usize| bar0.read_u32(addr).unwrap_or(0);

        // gm200_gr_init_gpc_mmu (GV100 path):
        //   0x418880 = rd32(0x100c80) & 0xf0001fff
        //   0x4188b4 = rd32(0x100cc8)  — FB MMU write target
        //   0x4188b8 = rd32(0x100ccc)  — FB MMU read target
        //   0x4188b0 = rd32(0x100cc4)  — FB MMU base
        //   0x418890 = 0, 0x418894 = 0
        let gpc_mmu_cfg = r(0x100c80) & 0xf000_1fff;
        let _ = bar0.write_u32(0x418880, gpc_mmu_cfg);
        let _ = bar0.write_u32(0x418890, 0);
        let _ = bar0.write_u32(0x418894, 0);
        let _ = bar0.write_u32(0x4188b4, r(0x100cc8));
        let _ = bar0.write_u32(0x4188b8, r(0x100ccc));
        let _ = bar0.write_u32(0x4188b0, r(0x100cc4));

        // gm200_gr_init_num_active_ltcs:
        //   GPC_BCAST(0x08ac) = 0x4188ac = rd32(0x100800)
        //   GPC_BCAST(0x033c) = 0x41833c = rd32(0x100804)
        let _ = bar0.write_u32(0x4188ac, r(0x100800));
        let _ = bar0.write_u32(0x41833c, r(0x100804));

        // gp100_gr_init_rop_active_fbps:
        //   fbp_count = rd32(0x12006c) & 0xf
        //   mask(0x408850, 0xf, fbp_count)
        //   mask(0x408958, 0xf, fbp_count)
        let fbp_count = r(0x12006c) & 0xf;
        let cur_408850 = r(0x408850);
        let _ = bar0.write_u32(0x408850, (cur_408850 & !0xf) | fbp_count);
        let cur_408958 = r(0x408958);
        let _ = bar0.write_u32(0x408958, (cur_408958 & !0xf) | fbp_count);

        // GR FE power mode and SCC init (ctxgf100.c)
        let _ = bar0.write_u32(0x40802c, 1);

        // Interrupt/trap enables (gf100_gr_init):
        let _ = bar0.write_u32(0x400100, 0xffff_ffff);
        let _ = bar0.write_u32(0x40013c, 0xffff_ffff);
        let _ = bar0.write_u32(0x400124, 0x0000_0002);
        // Trap handler enables
        let _ = bar0.write_u32(0x404000, 0xc000_0000);
        let _ = bar0.write_u32(0x404600, 0xc000_0000);
        let _ = bar0.write_u32(0x408030, 0xc000_0000);
        let _ = bar0.write_u32(0x406018, 0xc000_0000);
        let _ = bar0.write_u32(0x404490, 0xc000_0000);
        let _ = bar0.write_u32(0x405840, 0xc000_0000);
        let _ = bar0.write_u32(0x405844, 0x00ff_ffff);
        // gm200_gr_init_ds_hww_esr_2
        let _ = bar0.write_u32(0x405848, 0xc000_0000);
        let cur_40584c = r(0x40584c);
        let _ = bar0.write_u32(0x40584c, cur_40584c | 1);
        // gk104_gr_init_sked_hww_esr
        let _ = bar0.write_u32(0x407020, 0x4000_0000);
        // Hub trap enables
        let _ = bar0.write_u32(0x400108, 0xffff_ffff);
        let _ = bar0.write_u32(0x400138, 0xffff_ffff);
        let _ = bar0.write_u32(0x400118, 0xffff_ffff);
        let _ = bar0.write_u32(0x400130, 0xffff_ffff);
        let _ = bar0.write_u32(0x40011c, 0xffff_ffff);
        let _ = bar0.write_u32(0x400134, 0xffff_ffff);

        // FECS exceptions (gp100_gr_init_fecs_exceptions)
        let _ = bar0.write_u32(0x409c24, 0x000e_0002);

        // GR enable: 0x400500 = 0x00010001 (after MMIO init)
        let _ = bar0.write_u32(0x400500, 0x0001_0001);

        tracing::info!(
            sm_version,
            fbp_count,
            gpc_mmu_cfg = format!("{gpc_mmu_cfg:#010x}"),
            "dynamic GR init complete"
        );
    }

    /// Restart FECS/GPCCS falcons after a warm handoff from nouveau.
    ///
    /// After `coralctl warm-fecs` + livepatch, both falcons are in HRESET
    /// (CPUCTL=0x10): firmware sits in IMEM but the CPU is held in hardware
    /// reset. The restart sequence:
    ///
    /// 1. Dump GR engine state for diagnostics
    /// 2. Re-apply GR engine enables (interrupt, trap, 0x400500)
    /// 3. ENGCTL release + IINVAL + STARTCPU on GPCCS then FECS
    /// 4. Try FECS method interface to set up GR context
    ///
    /// Boot order: GPCCS first (FECS expects GPCCS running), then FECS.
    pub fn restart_warm_falcons(&mut self) -> DriverResult<()> {
        use crate::vfio::channel::registers::falcon;
        use std::borrow::Cow;

        let r = |a: usize| self.bar0.read_u32(a).unwrap_or(0xDEAD_DEAD);
        let w = |a: usize, v: u32| {
            let _ = self.bar0.write_u32(a, v);
        };

        let fecs_cpuctl = r(falcon::FECS_BASE + falcon::CPUCTL);
        let fecs_sctl = r(falcon::FECS_BASE + falcon::SCTL);
        let fecs_pc = r(falcon::FECS_BASE + falcon::PC);
        let gr_enable = r(0x400500);
        let fecs_mb0 = r(falcon::FECS_BASE + falcon::MAILBOX0);
        let fecs_exci = r(falcon::FECS_BASE + falcon::EXCI);

        let halted = fecs_cpuctl & falcon::CPUCTL_HALTED != 0;
        let stopped = fecs_cpuctl & falcon::CPUCTL_STOPPED != 0;
        let hs_mode = (fecs_sctl >> 12) & 3 >= 2;

        tracing::info!(
            fecs_cpuctl = format_args!("{fecs_cpuctl:#010x}"),
            fecs_sctl = format_args!("{fecs_sctl:#010x}"),
            fecs_pc = format_args!("{fecs_pc:#06x}"),
            fecs_exci = format_args!("{fecs_exci:#010x}"),
            fecs_mb0 = format_args!("{fecs_mb0:#010x}"),
            gr_enable = format_args!("{gr_enable:#010x}"),
            halted,
            stopped,
            hs_mode,
            "warm restart: FECS state"
        );

        let fecs_dead = fecs_cpuctl == 0xDEAD_DEAD || fecs_cpuctl & 0xBADF_0000 == 0xBADF_0000;
        if fecs_dead {
            return Err(DriverError::SubmitFailed(Cow::Borrowed(
                "FECS unreachable (PRI timeout) — GPU is cold",
            )));
        }

        // Re-apply GR engine enable and interrupt registers before
        // starting falcons — FECS checks these during init.
        w(0x400100, 0xFFFF_FFFF);
        w(0x40013c, 0xFFFF_FFFF);
        w(0x400124, 0x0000_0002);
        w(0x409C24, 0x000E_0002);
        w(0x400500, 0x0001_0001);

        let gr_enable_after = r(0x400500);
        tracing::info!(
            gr_enable = format_args!("{gr_enable_after:#010x}"),
            "GR engine enable after re-apply"
        );

        // On warm handoff, FECS firmware is HALT'd (bit 4) — it ran during
        // nouveau and entered its idle HALT loop. On HS+ Volta, a HALT'd
        // falcon cannot be woken via STARTCPU. We try SWGEN0 first,
        // then STARTCPU as fallback.
        //
        // If ENGCTL = 1 (engine-local reset), we clear it first.

        let engctl = r(falcon::FECS_BASE + falcon::ENGCTL);
        if engctl & 1 != 0 {
            tracing::info!("FECS ENGCTL reset active — releasing");
            w(falcon::FECS_BASE + falcon::ENGCTL, 0x00);
            w(falcon::GPCCS_BASE + falcon::ENGCTL, 0x00);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        if halted {
            // CPUCTL bit 4: firmware executed HALT instruction (idle loop).
            // Try multiple wake strategies in order:

            tracing::info!("FECS firmware HALTED (bit4) — attempting wake strategies");

            // Strategy 1: Trigger SWGEN0 interrupt to wake firmware from HALT.
            // DON'T touch IRQMCLR — the firmware configured its own interrupt
            // mask during ACR boot. Clearing it would disable all interrupts
            // and prevent the firmware from waking.
            // Only clear pending interrupts, re-enable the mask, then trigger.
            w(falcon::FECS_BASE + falcon::IRQSCLR, 0xFFFF_FFFF);
            // Re-enable all interrupt sources in the mask (IRQMSET).
            w(falcon::FECS_BASE + 0x010, 0xFFFF_FFFF); // IRQMSET
            w(falcon::FECS_BASE + falcon::IRQMODE, 0xFC24);
            w(falcon::GPCCS_BASE + falcon::IRQSCLR, 0xFFFF_FFFF);
            w(falcon::GPCCS_BASE + 0x010, 0xFFFF_FFFF); // IRQMSET
            w(falcon::GPCCS_BASE + falcon::IRQMODE, 0xFC24);

            // Trigger SWGEN0 (bit 6) — host→falcon interrupt.
            w(falcon::FECS_BASE + falcon::IRQSSET, 1 << 6);
            w(falcon::GPCCS_BASE + falcon::IRQSSET, 1 << 6);
            std::thread::sleep(std::time::Duration::from_millis(50));

            let fecs_cpuctl_1 = r(falcon::FECS_BASE + falcon::CPUCTL);
            let fecs_irq_1 = r(falcon::FECS_BASE + falcon::IRQSTAT);
            let fecs_pc_1 = r(falcon::FECS_BASE + falcon::PC);
            let fecs_mb0_1 = r(falcon::FECS_BASE + falcon::MAILBOX0);

            tracing::info!(
                fecs_cpuctl = format_args!("{fecs_cpuctl_1:#010x}"),
                fecs_irq = format_args!("{fecs_irq_1:#010x}"),
                fecs_pc = format_args!("{fecs_pc_1:#06x}"),
                fecs_mb0 = format_args!("{fecs_mb0_1:#010x}"),
                "after SWGEN0 interrupt trigger"
            );

            // Strategy 2: If still halted, try STARTCPU via CPUCTL_ALIAS
            // (works on some falcons where CPUCTL is locked in HS mode).
            if fecs_cpuctl_1 & falcon::CPUCTL_HALTED != 0 {
                tracing::info!("FECS still halted — trying STARTCPU via CPUCTL_ALIAS");
                Self::warm_start_falcon(&self.bar0, falcon::GPCCS_BASE);
                Self::warm_start_falcon(&self.bar0, falcon::FECS_BASE);
                std::thread::sleep(std::time::Duration::from_millis(100));
            }

            let fecs_cpuctl_post = r(falcon::FECS_BASE + falcon::CPUCTL);
            let fecs_mb0_post = r(falcon::FECS_BASE + falcon::MAILBOX0);
            let fecs_pc_post = r(falcon::FECS_BASE + falcon::PC);
            let gpccs_cpuctl_post = r(falcon::GPCCS_BASE + falcon::CPUCTL);

            tracing::info!(
                fecs_cpuctl = format_args!("{fecs_cpuctl_post:#010x}"),
                fecs_mb0 = format_args!("{fecs_mb0_post:#010x}"),
                fecs_pc = format_args!("{fecs_pc_post:#06x}"),
                gpccs_cpuctl = format_args!("{gpccs_cpuctl_post:#010x}"),
                "post-wake falcon state"
            );
        } else if stopped {
            tracing::info!("FECS STOPPED (bit5) — method interface should be available");
        } else {
            tracing::info!("FECS running — proceeding directly");
        }

        // Clear stale PBDMA interrupts accumulated while FECS was halted.
        let pbdma_map = r(0x2004);
        for pid in 0..32_usize {
            if pbdma_map & (1 << pid) == 0 {
                continue;
            }
            let b = 0x0004_0000 + pid * 0x2000;
            let intr = r(b + 0x108);
            if intr != 0 {
                tracing::info!(
                    pbdma = pid,
                    intr = format_args!("{intr:#010x}"),
                    "clearing stale PBDMA interrupt"
                );
                w(b + 0x108, 0xFFFF_FFFF);
            }
        }
        w(0x2100, 0xFFFF_FFFF);

        // Exp 126: Reset FECS method interface status registers before
        // attempting GR context setup. Stale status from nouveau's last
        // method call can cause our first method to misinterpret the response.
        w(falcon::FECS_BASE + falcon::MTHD_STATUS, 0);
        w(falcon::FECS_BASE + falcon::MTHD_STATUS2, 0);
        tracing::info!("warm: cleared FECS method interface status registers");

        self.setup_gr_context_warm()
    }

    /// Release a falcon from engine reset and issue STARTCPU.
    ///
    /// During nouveau teardown, `gm200_flcn_fw_fini` writes ENGCTL=0x01
    /// (engine-local reset), which holds the CPU in HRESET regardless of
    /// CPUCTL writes. We must release ENGCTL first, then STARTCPU.
    ///
    /// The full sequence mirrors nouveau's `gm200_flcn_fw_boot`:
    /// 1. ENGCTL = 0x00 (release engine from reset)
    /// 2. Clear IRQSCLR (pending interrupts)
    /// 3. MAILBOX0/MAILBOX1 = 0 (clean state for firmware handshake)
    /// 4. CPUCTL = IINVAL | STARTCPU (invalidate icache + start CPU)
    /// 5. Also write CPUCTL_ALIAS for Volta HS compatibility
    fn warm_start_falcon(bar0: &MappedBar, base: usize) {
        use crate::vfio::channel::registers::falcon;

        let cpuctl = bar0.read_u32(base + falcon::CPUCTL).unwrap_or(0);
        let bootvec = bar0.read_u32(base + falcon::BOOTVEC).unwrap_or(0xDEAD);
        let engctl = bar0.read_u32(base + falcon::ENGCTL).unwrap_or(0xDEAD);
        let mailbox0 = bar0.read_u32(base + falcon::MAILBOX0).unwrap_or(0);

        tracing::info!(
            base = format_args!("{base:#x}"),
            cpuctl = format_args!("{cpuctl:#010x}"),
            bootvec = format_args!("{bootvec:#010x}"),
            engctl = format_args!("{engctl:#010x}"),
            mailbox0 = format_args!("{mailbox0:#010x}"),
            "warm_start_falcon: pre-release state"
        );

        // Step 1: Release engine from local reset if ENGCTL has reset bit set.
        // This is the gate that prevents STARTCPU from working.
        if engctl & 1 != 0 {
            tracing::info!(
                base = format_args!("{base:#x}"),
                "ENGCTL reset active — releasing"
            );
            let _ = bar0.write_u32(base + falcon::ENGCTL, 0x00);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        // Step 2: Clear pending interrupts and exceptions.
        let _ = bar0.write_u32(base + falcon::IRQSCLR, 0xFFFF_FFFF);

        // Step 3: Clean mailbox state for firmware handshake.
        let _ = bar0.write_u32(base + falcon::MAILBOX0, 0);
        let _ = bar0.write_u32(base + falcon::MAILBOX1, 0);

        // Step 4: IINVAL + STARTCPU — invalidate instruction cache and start.
        let start_val = falcon::CPUCTL_IINVAL | falcon::CPUCTL_STARTCPU; // 0x03
        let _ = bar0.write_u32(base + falcon::CPUCTL, start_val);
        // Also write CPUCTL_ALIAS — on Volta HS falcons, the primary CPUCTL
        // may be locked and only CPUCTL_ALIAS accepts STARTCPU.
        let _ = bar0.write_u32(base + falcon::CPUCTL_ALIAS, start_val);

        std::thread::sleep(std::time::Duration::from_millis(20));

        let cpuctl_after = bar0.read_u32(base + falcon::CPUCTL).unwrap_or(0xDEAD);
        let pc_after = bar0.read_u32(base + falcon::PC).unwrap_or(0);
        let exci_after = bar0.read_u32(base + falcon::EXCI).unwrap_or(0);
        let mailbox0_after = bar0.read_u32(base + falcon::MAILBOX0).unwrap_or(0);
        let engctl_after = bar0.read_u32(base + falcon::ENGCTL).unwrap_or(0xDEAD);

        tracing::info!(
            base = format_args!("{base:#x}"),
            cpuctl = format_args!("{cpuctl_after:#010x}"),
            pc = format_args!("{pc_after:#06x}"),
            exci = format_args!("{exci_after:#010x}"),
            engctl = format_args!("{engctl_after:#010x}"),
            mailbox0 = format_args!("{mailbox0_after:#010x}"),
            "warm_start_falcon: post-STARTCPU state"
        );
    }

    /// GR context setup that bypasses `fecs_is_alive()`.
    ///
    /// On Volta, the sticky CPUCTL HRESET bit causes `fecs_is_alive()`
    /// to return false even when FECS is running. This method calls the
    /// FECS method interface directly without the liveness gate.
    fn setup_gr_context_warm(&mut self) -> DriverResult<()> {
        use super::acr_boot::fecs_method;

        let image_size = match fecs_method::fecs_discover_image_size(&self.bar0) {
            Ok(sz) => sz,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "GR context setup failed after warm restart — FECS method interface \
                     may not be responding (firmware still initializing or TRAP'd)"
                );
                return Ok(());
            }
        };

        if image_size == 0 {
            tracing::warn!("FECS returned image_size=0 — method interface not responsive yet");
            return Ok(());
        }

        let alloc_size = (image_size as usize).max(4096);
        let (_handle, iova) = self.alloc_dma(alloc_size)?;

        fecs_method::fecs_init_exceptions(&self.bar0);
        fecs_method::fecs_set_watchdog_timeout(&self.bar0, 0x7FFF_FFFF)?;
        fecs_method::fecs_bind_pointer(&self.bar0, iova)?;
        fecs_method::fecs_wfi_golden_save(&self.bar0, iova)?;

        tracing::info!(
            image_size,
            iova = format_args!("{iova:#x}"),
            "GR context ready after warm falcon restart"
        );
        Ok(())
    }

    /// Restart FECS from frozen scheduling state (Exp 132 diesel engine).
    ///
    /// After `STOP_CTXSW` via ember, FECS is alive but not scheduling.
    /// PFIFO has been rebuilt with `warm_fecs` config and our new channel
    /// is on the runlist. This method:
    ///
    /// 1. Clears stale PBDMA interrupts
    /// 2. Resets FECS method interface status
    /// 3. Sends `START_CTXSW` (method 0x02) to resume scheduling
    /// 4. Sets up GR context (discover sizes, bind, golden save)
    pub fn restart_frozen_fecs(&mut self) -> DriverResult<()> {
        use crate::vfio::channel::registers::falcon;
        use super::acr_boot::fecs_method;

        let r = |a: usize| self.bar0.read_u32(a).unwrap_or(0xDEAD_DEAD);
        let w = |a: usize, v: u32| {
            let _ = self.bar0.write_u32(a, v);
        };

        let fecs_cpuctl = r(falcon::FECS_BASE + falcon::CPUCTL);
        let fecs_pc = r(falcon::FECS_BASE + falcon::PC);
        let fecs_mb0 = r(falcon::FECS_BASE + falcon::MAILBOX0);

        tracing::info!(
            fecs_cpuctl = format_args!("{fecs_cpuctl:#010x}"),
            fecs_pc = format_args!("{fecs_pc:#06x}"),
            fecs_mb0 = format_args!("{fecs_mb0:#010x}"),
            "restart_frozen_fecs: FECS state before START_CTXSW"
        );

        // Clear stale PBDMA interrupts accumulated during the swap.
        let pbdma_map = r(0x2004);
        for pid in 0..32_usize {
            if pbdma_map & (1 << pid) == 0 {
                continue;
            }
            let b = 0x0004_0000 + pid * 0x2000;
            let intr = r(b + 0x108);
            if intr != 0 {
                tracing::info!(
                    pbdma = pid,
                    intr = format_args!("{intr:#010x}"),
                    "clearing stale PBDMA interrupt"
                );
                w(b + 0x108, 0xFFFF_FFFF);
            }
        }
        w(0x2100, 0xFFFF_FFFF);

        // Reset FECS method interface status registers.
        w(falcon::FECS_BASE + falcon::MTHD_STATUS, 0);
        w(falcon::FECS_BASE + falcon::MTHD_STATUS2, 0);

        // Re-apply GR engine enable and interrupt registers.
        w(0x400100, 0xFFFF_FFFF);
        w(0x40013c, 0xFFFF_FFFF);
        w(0x400124, 0x0000_0002);
        w(0x409C24, 0x000E_0002);
        w(0x400500, 0x0001_0001);

        // Resume FECS scheduling — method 0x02 (START_CTXSW).
        // FECS will process the new runlist and schedule our channel.
        tracing::info!("restart_frozen_fecs: sending START_CTXSW (method 0x02)");
        match fecs_method::fecs_start_ctxsw(&self.bar0) {
            Ok(()) => {
                tracing::info!("restart_frozen_fecs: START_CTXSW success — scheduling resumed");
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "restart_frozen_fecs: START_CTXSW failed — FECS may need full restart"
                );
                return self.restart_warm_falcons();
            }
        }

        let fecs_cpuctl_post = r(falcon::FECS_BASE + falcon::CPUCTL);
        let fecs_mb0_post = r(falcon::FECS_BASE + falcon::MAILBOX0);
        tracing::info!(
            fecs_cpuctl = format_args!("{fecs_cpuctl_post:#010x}"),
            fecs_mb0 = format_args!("{fecs_mb0_post:#010x}"),
            "restart_frozen_fecs: post-START_CTXSW state"
        );

        // Set up GR context using the now-running FECS method interface.
        self.setup_gr_context_warm()
    }

    /// Submit FECS channel init methods via GPFIFO after channel creation.
    ///
    /// Builds a push buffer containing the GR context setup methods
    /// from `sw_bundle_init.bin` / `sw_method_init.bin` (entries with
    /// offsets <= 0x7FFC that are submittable as channel methods).
    ///
    /// If FECS firmware is already running (e.g. after a warm handoff
    /// from nouveau), the GR init methods may conflict with the running
    /// firmware's context. We skip channel init in that case — the
    /// firmware is already managing the GR engine.
    pub(super) fn apply_fecs_channel_init(&mut self) {
        use crate::vfio::channel::registers::falcon;
        let fecs_cpuctl = self
            .bar0
            .read_u32(falcon::FECS_BASE + falcon::CPUCTL)
            .unwrap_or(0xDEAD_DEAD);
        let fecs_mailbox0 = self
            .bar0
            .read_u32(falcon::FECS_BASE + falcon::MAILBOX0)
            .unwrap_or(0);

        // PRI faults (0xbad0xxxx / 0xbadfxxxx) indicate the register read
        // went through the PRI ring but the target engine is unreachable
        // (clock gated, not POST-ed, or ring corrupted). These must NOT
        // be interpreted as valid CPUCTL/mailbox values.
        let fecs_pri_fault = is_pri_fault_value(fecs_cpuctl);
        let mb0_pri_fault = is_pri_fault_value(fecs_mailbox0);

        if fecs_pri_fault || fecs_cpuctl == 0xDEAD_DEAD {
            tracing::warn!(
                fecs_cpuctl = format!("{fecs_cpuctl:#010x}"),
                fecs_mailbox0 = format!("{fecs_mailbox0:#010x}"),
                "FECS registers return PRI fault — GPU needs initialization \
                 (nvidia recipe or VBIOS devinit via glowplug)"
            );
            return;
        }

        let fecs_halted = fecs_cpuctl & falcon::CPUCTL_HALTED != 0;
        let fecs_stopped = fecs_cpuctl & falcon::CPUCTL_STOPPED != 0;
        let fecs_running = !fecs_halted && !fecs_stopped;
        let mb0_valid = !mb0_pri_fault && fecs_mailbox0 != 0;

        if fecs_running || mb0_valid {
            tracing::info!(
                fecs_cpuctl = format!("{fecs_cpuctl:#010x}"),
                fecs_mailbox0 = format!("{fecs_mailbox0:#010x}"),
                "FECS firmware already running — skipping channel init (warm handoff)"
            );
            return;
        }

        let chip = sm_to_chip(self.sm_version);
        let blobs = match GrFirmwareBlobs::parse(chip) {
            Ok(b) => b,
            Err(e) => {
                tracing::debug!(chip, error = %e, "firmware not available — skipping FECS init");
                return;
            }
        };

        let seq = if self.sm_version == 70 {
            GrInitSequence::for_gv100(&blobs)
        } else {
            GrInitSequence::from_blobs(&blobs)
        };

        let (_bar0, fecs) = gsp::split_for_application(&seq);

        let channel_methods: Vec<(u32, u32)> = fecs
            .iter()
            .filter(|w| {
                matches!(
                    w.category,
                    gsp::RegCategory::BundleInit | gsp::RegCategory::MethodInit
                )
            })
            .map(|w| (w.offset, w.value))
            .collect();

        if channel_methods.is_empty() {
            tracing::debug!(chip, "no FECS channel methods to submit");
            return;
        }

        tracing::info!(
            chip,
            entries = channel_methods.len(),
            "submitting FECS channel methods via GPFIFO"
        );

        let pb = PushBuf::gr_context_init(self.compute_class, &channel_methods);
        let pb_bytes = pb.as_bytes();

        let pb_result = (|| -> DriverResult<()> {
            let (pb_handle, pb_iova) = self.alloc_dma(pb_bytes.len())?;
            self.upload(pb_handle, 0, pb_bytes)?;

            let pb_size = u32::try_from(pb_bytes.len())
                .map_err(|_| DriverError::platform_overflow("FECS pushbuf size fits u32"))?;

            self.submit_pushbuf(pb_iova, pb_size)?;

            // Wait for FECS init to complete before returning the device
            // as ready for compute dispatch.
            self.poll_gpfifo_completion()?;

            let _ = self.free(pb_handle);
            Ok(())
        })();

        match pb_result {
            Ok(()) => tracing::info!(chip, "FECS channel init complete — GR engine ready"),
            Err(e) => {
                tracing::warn!(chip, error = %e, "FECS channel init failed (expected on cold VFIO — GR engine requires falcon firmware)")
            }
        }
    }
}
