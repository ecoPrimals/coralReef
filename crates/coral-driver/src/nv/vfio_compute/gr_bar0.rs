// SPDX-License-Identifier: AGPL-3.0-or-later
//! GR BAR0 init — firmware-blob-driven register writes and dynamic GR configuration.

use super::{NvVfioComputeDevice, sm_to_chip};
use crate::gsp::{self, GrFirmwareBlobs, GrInitSequence};
use crate::vfio::device::MappedBar;

impl NvVfioComputeDevice {
    /// Apply BAR0 GR init writes from NVIDIA firmware blobs.
    ///
    /// Parses `sw_bundle_init.bin` etc. from `/lib/firmware/nvidia/{chip}/gr/`,
    /// builds the init sequence, then applies the BAR0-targeted writes
    /// (PMC engine enable, FIFO enable, PGRAPH register programming).
    pub(crate) fn apply_gr_bar0_init(bar0: &MappedBar, sm_version: u32) {
        let chip = sm_to_chip(sm_version);
        let blobs = match GrFirmwareBlobs::parse(chip) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(chip, error = %e, "GR firmware not available — skipping BAR0 GR init");
                return;
            }
        };

        let profile = crate::nv::generation::profile_for_sm(sm_version);
        let seq = GrInitSequence::for_profile(&blobs, profile);

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
}
