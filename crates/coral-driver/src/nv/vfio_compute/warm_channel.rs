// SPDX-License-Identifier: AGPL-3.0-or-later
//! Warm falcon restart and FECS channel initialization methods.

use crate::ComputeDevice;
use crate::error::{DriverError, DriverResult};
use crate::gsp::{self, GrFirmwareBlobs, GrInitSequence};
use crate::vfio::device::MappedBar;

use super::super::pushbuf::PushBuf;
use super::{NvVfioComputeDevice, sm_to_chip};

impl NvVfioComputeDevice {
    /// Restart FECS/GPCCS falcons after a warm handoff from nouveau.
    ///
    /// After `coralctl warm-fecs` + livepatch, both falcons should be in
    /// HALTED state (firmware running, context-switch loop idle). If the
    /// livepatch was incomplete and falcons fell into HRESET, we attempt
    /// ENGCTL release + STARTCPU recovery (GPCCS first, then FECS).
    ///
    /// 1. Dump GR engine state for diagnostics
    /// 2. Re-apply GR engine enables (interrupt, trap, 0x400500)
    /// 3. If HRESET: STARTCPU on GPCCS then FECS (recovery path)
    /// 4. FECS method interface → GR context setup
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

        let gpccs_cpuctl = r(falcon::GPCCS_BASE + falcon::CPUCTL);
        let gpccs_hreset = gpccs_cpuctl & falcon::CPUCTL_HRESET != 0;

        let halted = fecs_cpuctl & falcon::CPUCTL_HALTED != 0;
        let hreset = fecs_cpuctl & falcon::CPUCTL_HRESET != 0;
        let hs_mode = (fecs_sctl >> 12) & 3 >= 2;

        tracing::info!(
            fecs_cpuctl = format_args!("{fecs_cpuctl:#010x}"),
            fecs_sctl = format_args!("{fecs_sctl:#010x}"),
            fecs_pc = format_args!("{fecs_pc:#06x}"),
            fecs_exci = format_args!("{fecs_exci:#010x}"),
            fecs_mb0 = format_args!("{fecs_mb0:#010x}"),
            gpccs_cpuctl = format_args!("{gpccs_cpuctl:#010x}"),
            gr_enable = format_args!("{gr_enable:#010x}"),
            halted,
            hreset,
            hs_mode,
            "warm restart: falcon state"
        );

        let fecs_dead = fecs_cpuctl == 0xDEAD_DEAD || fecs_cpuctl & 0xBADF_0000 == 0xBADF_0000;
        if fecs_dead {
            return Err(DriverError::SubmitFailed(Cow::Borrowed(
                "FECS unreachable (PRI timeout) — GPU is cold",
            )));
        }

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

        if hreset {
            tracing::warn!(
                "FECS in HRESET — livepatch did not fully prevent self-reset. \
                 Attempting STARTCPU recovery (GPCCS first, then FECS)."
            );
            if gpccs_hreset {
                tracing::info!("GPCCS also in HRESET — starting GPCCS first");
                Self::warm_start_falcon(&self.bar0, falcon::GPCCS_BASE);
            }
            Self::warm_start_falcon(&self.bar0, falcon::FECS_BASE);

            let fecs_cpuctl_post = r(falcon::FECS_BASE + falcon::CPUCTL);
            let still_hreset = fecs_cpuctl_post & falcon::CPUCTL_HRESET != 0;
            let now_halted = fecs_cpuctl_post & falcon::CPUCTL_HALTED != 0;
            tracing::info!(
                fecs_cpuctl = format_args!("{fecs_cpuctl_post:#010x}"),
                still_hreset,
                now_halted,
                "FECS state after STARTCPU recovery"
            );
            if still_hreset {
                tracing::warn!(
                    "FECS still in HRESET after STARTCPU — HS mode prevents host restart. \
                     Only SEC2/ACR can boot HS falcons on Volta."
                );
            }
        } else if halted {
            tracing::info!("FECS HALTED (not HRESET) — restarting falcon for method interface");
            let gpccs_halted = gpccs_cpuctl & falcon::CPUCTL_HALTED != 0;
            if gpccs_halted {
                tracing::info!("GPCCS also HALTED — starting GPCCS first");
                Self::warm_start_falcon(&self.bar0, falcon::GPCCS_BASE);
            }
            Self::warm_start_falcon(&self.bar0, falcon::FECS_BASE);

            let fecs_cpuctl_post = r(falcon::FECS_BASE + falcon::CPUCTL);
            let fecs_pc_post = r(falcon::FECS_BASE + falcon::PC);
            let fecs_mb0_post = r(falcon::FECS_BASE + falcon::MAILBOX0);
            let fecs_exci_post = r(falcon::FECS_BASE + falcon::EXCI);
            tracing::info!(
                fecs_cpuctl = format_args!("{fecs_cpuctl_post:#010x}"),
                fecs_pc = format_args!("{fecs_pc_post:#06x}"),
                fecs_mb0 = format_args!("{fecs_mb0_post:#010x}"),
                fecs_exci = format_args!("{fecs_exci_post:#010x}"),
                "FECS state after warm STARTCPU"
            );
        }

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

        if engctl & 1 != 0 {
            tracing::info!(
                base = format_args!("{base:#x}"),
                "ENGCTL reset active — releasing"
            );
            let _ = bar0.write_u32(base + falcon::ENGCTL, 0x00);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let _ = bar0.write_u32(base + falcon::IRQSCLR, 0xFFFF_FFFF);

        let _ = bar0.write_u32(base + falcon::MAILBOX0, 0);
        let _ = bar0.write_u32(base + falcon::MAILBOX1, 0);

        let start_val = falcon::CPUCTL_IINVAL | falcon::CPUCTL_STARTCPU;
        let _ = bar0.write_u32(base + falcon::CPUCTL, start_val);
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
    pub(super) fn setup_gr_context_warm(&mut self) -> DriverResult<()> {
        use super::acr_boot::fecs_method;

        // Internal firmware writes ctx_size to 0x409804 during boot;
        // external firmware requires a method call (0x10) to discover it.
        // Try the register first, fall back to the method interface.
        let reg_ctx_size = self.bar0.read_u32(0x0040_9804).unwrap_or(0);
        let image_size = if reg_ctx_size > 0
            && reg_ctx_size != 0xDEAD_DEAD
            && reg_ctx_size & 0xBAD0_0000 != 0xBAD0_0000
        {
            tracing::info!(
                ctx_size = reg_ctx_size,
                ctx_hex = format_args!("{reg_ctx_size:#010x}"),
                "GR context size from 0x409804 (internal firmware path)"
            );
            reg_ctx_size as usize
        } else {
            match fecs_method::fecs_discover_image_size(&self.bar0) {
                Ok(sz) if sz > 0 => sz as usize,
                Ok(_) => {
                    tracing::warn!("FECS returned image_size=0 — method interface not responsive");
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "GR context setup failed — FECS method interface not responding"
                    );
                    return Ok(());
                }
            }
        };

        if image_size == 0 {
            return Ok(());
        }

        // Nouveau reserves CB_RESERVED (0x80000 = 512 KiB) before the context image
        // for global context buffers (bundle CB, pagepool, attrib CB).
        const CB_RESERVED: usize = 0x0008_0000;
        let alloc_size = (CB_RESERVED + image_size).max(4096);
        let (_handle, ctx_iova) = self.alloc_dma(alloc_size)?;

        // Write the GR context buffer address into the channel instance block.
        // Nouveau: inst[0x210] = lower_32(ctx_vaddr + CB_RESERVED) | 4
        //          inst[0x214] = upper_32(ctx_vaddr + CB_RESERVED)
        let ctx_vaddr = ctx_iova + CB_RESERVED as u64;
        self.channel.write_gr_context_ptr(ctx_vaddr, 4);

        let is_internal = reg_ctx_size > 0
            && reg_ctx_size != 0xDEAD_DEAD
            && reg_ctx_size & 0xBAD0_0000 != 0xBAD0_0000;

        if is_internal {
            // Internal firmware: the context pointer is already in the instance
            // block. Nouveau's bind (method 0x01) relies on a timing race with
            // the boot flag in 0x409800 that doesn't work over VFIO. Instead,
            // write the channel binding data to FECS scratch registers (the
            // firmware reads them asynchronously) and wake it via 0x409840.
            let inst_iova = self.channel.instance_iova();
            let bind_data = 0x8000_0000 | (inst_iova >> 12) as u32;

            let _ = self.bar0.write_u32(0x0040_9840, 0x8000_0000);
            let _ = self.bar0.write_u32(0x0040_9500, bind_data);
            let _ = self.bar0.write_u32(0x0040_9504, 0x0000_0001);

            tracing::info!(
                inst_iova = format_args!("{inst_iova:#x}"),
                bind_data = format_args!("{bind_data:#010x}"),
                "FECS internal: wrote bind channel regs (fire-and-forget)"
            );

            // Give FECS time to process the bind before proceeding
            std::thread::sleep(std::time::Duration::from_millis(50));

            // Try context save via fake ctxsw interrupt
            match fecs_method::fecs_internal_save_context(&self.bar0) {
                Ok(()) => {}
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "FECS internal context save failed (non-fatal — \
                         context will be initialized on first channel load)"
                    );
                }
            }
        } else {
            // External firmware: methods 0x03/0x09, poll 0x409800 bit 4
            fecs_method::fecs_init_exceptions(&self.bar0);
            if let Err(e) = fecs_method::fecs_set_watchdog_timeout(&self.bar0, 0x7FFF_FFFF) {
                tracing::debug!(error = %e, "watchdog timeout method failed (non-fatal)");
            }
            fecs_method::fecs_bind_pointer(&self.bar0, ctx_iova)?;
            fecs_method::fecs_wfi_golden_save(&self.bar0, ctx_iova)?;
        }

        tracing::info!(
            image_size,
            ctx_iova = format_args!("{ctx_iova:#x}"),
            ctx_vaddr = format_args!("{ctx_vaddr:#x}"),
            is_internal,
            "GR context ready after warm falcon restart"
        );
        Ok(())
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
        let fecs_halted = fecs_cpuctl & falcon::CPUCTL_HALTED != 0;
        let fecs_hreset = fecs_cpuctl & falcon::CPUCTL_HRESET != 0;
        let fecs_running = !fecs_halted && !fecs_hreset && fecs_cpuctl != 0xDEAD_DEAD;

        if fecs_running || fecs_mailbox0 != 0 {
            tracing::info!(
                fecs_cpuctl = format!("{fecs_cpuctl:#010x}"),
                fecs_mailbox0 = format!("{fecs_mailbox0:#010x}"),
                "FECS firmware already running — skipping channel init (warm handoff)"
            );
            return;
        }

        let profile = crate::nv::generation::profile_for_sm(self.sm_version);
        if profile.page_table_format == crate::nv::generation::PageTableFormat::V1TwoLevel {
            tracing::info!(
                "Kepler FECS boot handled by kepler_cold_init — skipping channel method path"
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

        let profile = crate::nv::generation::profile_for_sm(self.sm_version);
        let seq = GrInitSequence::for_profile(&blobs, profile);

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
