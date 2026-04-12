// SPDX-License-Identifier: AGPL-3.0-only
//! VFIO GR init — BAR0 register writes and FECS channel methods.

use crate::ComputeDevice;
use crate::error::{DriverError, DriverResult};
use crate::gsp::{self, GrFirmwareBlobs, GrInitSequence};
use crate::vfio::device::MappedBar;

use super::super::pushbuf::PushBuf;
use super::{NvVfioComputeDevice, sm_to_chip};

/// Apply BAR0 GR init writes from NVIDIA firmware blobs.
///
/// Parses `sw_bundle_init.bin` etc. from `/lib/firmware/nvidia/{chip}/gr/`,
/// builds the init sequence, then applies the BAR0-targeted writes
/// (PMC engine enable, FIFO enable, PGRAPH register programming).
///
/// Returns `(bar0_applied, bar0_failed, fecs_entry_count)`.
pub(super) fn apply_gr_bar0_init(bar0: &MappedBar, sm_version: u32) -> (u32, u32, usize) {
    let chip = sm_to_chip(sm_version);
    let blobs = match GrFirmwareBlobs::parse(chip) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(chip, error = %e, "GR firmware not available — skipping BAR0 GR init");
            return (0, 0, 0);
        }
    };

    let seq = if sm_version == 70 {
        GrInitSequence::for_gv100(&blobs)
    } else {
        GrInitSequence::from_blobs(&blobs)
    };

    let (bar0_writes, fecs_entries) = gsp::split_for_application(&seq);
    let fecs_count = fecs_entries.len();

    tracing::info!(
        chip,
        bar0_writes = bar0_writes.len(),
        fecs_entries = fecs_count,
        total = seq.len(),
        "sovereign VFIO GR init: applying {} BAR0 register writes",
        bar0_writes.len()
    );

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

    for w in &bar0_writes {
        if w.delay_us > 0 {
            std::thread::sleep(std::time::Duration::from_micros(u64::from(w.delay_us)));
        }
    }

    apply_nonctx_writes(bar0, &blobs, chip);
    apply_dynamic_gr_init(bar0, sm_version);

    (applied as u32, failed as u32, fecs_count)
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

impl NvVfioComputeDevice {
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
        let hreset = fecs_cpuctl & falcon::CPUCTL_HRESET != 0;
        let hs_mode = (fecs_sctl >> 12) & 3 >= 2;

        tracing::info!(
            fecs_cpuctl = format_args!("{fecs_cpuctl:#010x}"),
            fecs_sctl = format_args!("{fecs_sctl:#010x}"),
            fecs_pc = format_args!("{fecs_pc:#06x}"),
            fecs_exci = format_args!("{fecs_exci:#010x}"),
            fecs_mb0 = format_args!("{fecs_mb0:#010x}"),
            gr_enable = format_args!("{gr_enable:#010x}"),
            halted,
            hreset,
            hs_mode,
            "warm restart: FECS state"
        );

        let fecs_dead = fecs_cpuctl == 0xDEAD_DEAD || fecs_cpuctl & 0xBADF_0000 == 0xBADF_0000;
        if fecs_dead {
            return Err(DriverError::SubmitFailed(Cow::Borrowed(
                "FECS unreachable (PRI timeout) — GPU is cold",
            )));
        }

        if hreset {
            tracing::warn!(
                "FECS in HRESET — livepatch did not prevent self-reset. \
                 Ensure livepatch is ENABLED after nouveau init and BEFORE teardown."
            );
        }

        // Re-apply GR engine enable and interrupt registers
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

        // If FECS is HALTED (not HRESET), the method interface should
        // work — FECS is in its context-switch handler waiting for work.
        // With the runlist-frozen livepatch, FECS thinks channels still
        // exist and stays responsive.
        if halted && !hreset {
            tracing::info!("FECS HALTED (not HRESET) — method interface should be available");
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
    #[expect(dead_code, reason = "reserved for warm handoff boot path")]
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
