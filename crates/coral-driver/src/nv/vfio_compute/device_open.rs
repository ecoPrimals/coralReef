// SPDX-License-Identifier: AGPL-3.0-or-later
//! VFIO compute device constructors and SM identity resolution.

use crate::error::{DriverError, DriverResult};
use crate::gsp::RegisterAccess;
use crate::vfio::device::VfioDevice;
use crate::vfio::dma::DmaBuffer;

use super::NvVfioComputeDevice;
use super::layout::{
    GPFIFO_IOVA, GUARD_PAGE_IOVA, SLM_IOVA, SLM_SIZE,
    USER_IOVA_BASE, USERD_IOVA, apply_error_to_driver, bar0_reg, gpfifo,
};

impl NvVfioComputeDevice {
    /// Resolve SM version and compute class from BOOT0, validating against
    /// caller-supplied hints. Pass `sm_version=0` to auto-detect; pass a
    /// nonzero value to assert it matches hardware.
    ///
    /// Accepts any [`RegisterAccess`] implementation (for example VFIO
    /// [`MappedBar`](crate::vfio::device::MappedBar) or unit-test doubles).
    fn resolve_sm(
        regs: &dyn RegisterAccess,
        bdf: &str,
        caller_sm: u32,
        caller_class: u32,
    ) -> DriverResult<(u32, u32)> {
        let boot0 = regs
            .read_u32(bar0_reg::BOOT0 as u32)
            .map_err(apply_error_to_driver)?;
        let hw_sm = crate::nv::identity::boot0_to_sm(boot0);

        let sm =
            if caller_sm == 0 {
                match hw_sm {
                    Some(sm) => {
                        tracing::info!(
                            bdf,
                            boot0 = format_args!("{boot0:#010x}"),
                            sm,
                            "SM auto-detected from BOOT0"
                        );
                        sm
                    }
                    None => {
                        return Err(DriverError::OpenFailed(format!(
                        "BOOT0 {boot0:#010x} maps to unknown chipset — cannot auto-detect SM. \
                         Pass an explicit sm_version or add the chipset to boot0_to_sm()."
                    ).into()));
                    }
                }
            } else {
                if let Some(hw) = hw_sm {
                    if hw != caller_sm {
                        return Err(DriverError::OpenFailed(
                            format!(
                                "SM mismatch: caller passed sm={caller_sm} but BOOT0 {boot0:#010x} \
                         decodes to sm={hw}. Wrong SM corrupts GPU state — aborting."
                            )
                            .into(),
                        ));
                    }
                } else {
                    tracing::warn!(
                        bdf,
                        boot0 = format_args!("{boot0:#010x}"),
                        caller_sm,
                        "BOOT0 chipset unknown — trusting caller-supplied SM"
                    );
                }
                caller_sm
            };

        let compute_class = if caller_class == 0 {
            crate::nv::identity::sm_to_compute_class(sm)
        } else {
            caller_class
        };

        tracing::info!(
            bdf,
            boot0 = format_args!("{boot0:#010x}"),
            sm,
            compute_class = format_args!("{compute_class:#06x}"),
            "VFIO GPU identity resolved"
        );

        Ok((sm, compute_class))
    }

    /// Opens an NVIDIA VFIO compute device by PCI BDF.
    ///
    /// Pass `sm_version=0` and `compute_class=0` to auto-detect from BOOT0.
    /// Nonzero values are validated against the hardware register.
    pub fn open(bdf: &str, sm_version: u32, compute_class: u32) -> DriverResult<Self> {
        let device = VfioDevice::open(bdf)?;
        Self::open_with_device(device, bdf, sm_version, compute_class)
    }

    /// Open using legacy VFIO group path only, bypassing iommufd.
    ///
    /// Kepler (GK210/K80) dies when iommufd attach triggers an implicit
    /// device reset. The legacy group path may avoid this on some kernels.
    pub fn open_legacy(bdf: &str, sm_version: u32, compute_class: u32) -> DriverResult<Self> {
        let device = VfioDevice::open_legacy(bdf)?;
        Self::open_with_device(device, bdf, sm_version, compute_class)
    }

    fn open_with_device(
        device: VfioDevice,
        bdf: &str,
        sm_version: u32,
        compute_class: u32,
    ) -> DriverResult<Self> {
        let container = device.dma_backend();
        let bar0 = device.map_bar(0)?;

        let (sm_version, compute_class) = Self::resolve_sm(&bar0, bdf, sm_version, compute_class)?;

        let profile = crate::nv::generation::profile_for_sm(sm_version);
        let seq = super::boot_sequence::boot_sequence_for(profile, sm_version);

        // Cold init: PRI ring + clocks + PGRAPH reset + firmware load.
        // IMPORTANT: We do NOT perform D3→D0 or SBR here. The VFIO subsystem
        // already performed FLR when binding the device, which gives us a clean
        // register state while preserving DRAM controller initialization from
        // BIOS POST.
        seq.cold_init(&bar0)?;

        let gpfifo_ring = DmaBuffer::new(container.clone(), gpfifo::RING_SIZE, GPFIFO_IOVA)?;
        let userd = DmaBuffer::new(container.clone(), 4096, USERD_IOVA)?;

        let slm_buf = DmaBuffer::new(container.clone(), SLM_SIZE, SLM_IOVA)
            .map_err(|e| {
                tracing::warn!("SLM pool allocation failed (non-fatal): {e}");
                e
            })
            .ok();

        #[expect(
            clippy::cast_possible_truncation,
            reason = "GPFIFO entries constant always fits u32"
        )]
        let channel = seq.create_channel(
            container.clone(),
            &bar0,
            GPFIFO_IOVA,
            gpfifo::ENTRIES as u32,
            USERD_IOVA,
            0,
        )?;

        let caps = profile.to_capabilities();
        let sem_fence = seq.uses_semaphore_fence();
        let (fence_buf, fence_pb_buf) = seq.alloc_fence_buffers(container.clone())?;

        let mut dev = Self {
            device,
            bar0,
            sm_version,
            compute_class,
            gpfifo_ring,
            gpfifo_put: 0,
            userd,
            channel,
            next_handle: 1,
            next_iova: USER_IOVA_BASE,
            container,
            buffers: std::collections::HashMap::new(),
            inflight: Vec::new(),
            caps,
            uses_semaphore_fence: sem_fence,
            fence_buf,
            fence_pb_buf,
            fence_value: 0,
            slm_buf,
            guard_page: None,
        };

        dev.apply_fecs_channel_init();

        Ok(dev)
    }

    /// Opens from pre-existing VFIO fds (received from coral-ember via `SCM_RIGHTS`).
    ///
    /// Pass `sm_version=0` and `compute_class=0` to auto-detect from BOOT0.
    /// Nonzero values are validated against the hardware register.
    pub fn open_from_fds(
        bdf: &str,
        fds: crate::vfio::ReceivedVfioFds,
        sm_version: u32,
        compute_class: u32,
    ) -> DriverResult<Self> {
        let device = VfioDevice::from_received(bdf, fds)?;
        let container = device.dma_backend();
        let bar0 = device.map_bar(0)?;

        let (sm_version, compute_class) = Self::resolve_sm(&bar0, bdf, sm_version, compute_class)?;

        let profile = crate::nv::generation::profile_for_sm(sm_version);
        let seq = super::boot_sequence::boot_sequence_for(profile, sm_version);

        // Ember FD handoff: apply GR init (FECS firmware already loaded by ember).
        NvVfioComputeDevice::apply_gr_bar0_init(&bar0, sm_version);

        let gpfifo_ring = DmaBuffer::new(container.clone(), gpfifo::RING_SIZE, GPFIFO_IOVA)?;
        let userd = DmaBuffer::new(container.clone(), 4096, USERD_IOVA)?;

        let slm_buf = DmaBuffer::new(container.clone(), SLM_SIZE, SLM_IOVA)
            .map_err(|e| {
                tracing::warn!("SLM pool allocation failed (non-fatal): {e}");
                e
            })
            .ok();

        #[expect(
            clippy::cast_possible_truncation,
            reason = "GPFIFO entries constant always fits u32"
        )]
        let channel = seq.create_channel(
            container.clone(),
            &bar0,
            GPFIFO_IOVA,
            gpfifo::ENTRIES as u32,
            USERD_IOVA,
            0,
        )?;

        let caps = profile.to_capabilities();
        let sem_fence = seq.uses_semaphore_fence();
        let (fence_buf, fence_pb_buf) = seq.alloc_fence_buffers(container.clone())?;

        let mut dev = Self {
            device,
            bar0,
            sm_version,
            compute_class,
            gpfifo_ring,
            gpfifo_put: 0,
            userd,
            channel,
            next_handle: 1,
            next_iova: USER_IOVA_BASE,
            container,
            buffers: std::collections::HashMap::new(),
            inflight: Vec::new(),
            caps,
            uses_semaphore_fence: sem_fence,
            fence_buf,
            fence_pb_buf,
            fence_value: 0,
            slm_buf,
            guard_page: None,
        };

        dev.apply_fecs_channel_init();
        Ok(dev)
    }

    /// Open from ember FDs in warm handoff mode.
    ///
    /// After `coralctl warm-fecs` + livepatch, FECS/GPCCS firmware is
    /// preserved in IMEM. This path skips GR BAR0 init (already done by
    /// nouveau) and uses a lighter PFIFO init that preserves PMC/engine state.
    pub fn open_warm(
        bdf: &str,
        fds: crate::vfio::ReceivedVfioFds,
        sm_version: u32,
        compute_class: u32,
    ) -> DriverResult<Self> {
        let device = VfioDevice::from_received(bdf, fds)?;
        Self::build_warm_device(device, bdf, sm_version, compute_class, false)
    }

    /// Open in warm handoff mode by directly opening `/dev/vfio/*`.
    ///
    /// Same as [`open_warm`] but without ember — opens the VFIO group and
    /// device fds directly. Requires the VFIO group to be accessible.
    ///
    /// On Kepler, uses deferred bus-master to quiesce PFIFO before DMA
    /// resumes — preventing `IO_PAGE_FAULT` cascades from nouveau's stale
    /// DMA configuration that gate GPCs.
    pub fn open_warm_direct(bdf: &str, sm_version: u32, compute_class: u32) -> DriverResult<Self> {
        let device = VfioDevice::open_no_busmaster(bdf)?;
        Self::build_warm_device(device, bdf, sm_version, compute_class, true)
    }

    /// Open in warm handoff mode via legacy VFIO group path.
    ///
    /// Combines the legacy VFIO open (safe for K80 — no iommufd FLR) with
    /// the warm handoff path (skips cold init — preserves nouveau POST state).
    ///
    /// Use after `k80_nouveau_post.sh` has POST'd the K80 via patched nouveau
    /// and swapped to vfio-pci.
    pub fn open_warm_legacy(bdf: &str, sm_version: u32, compute_class: u32) -> DriverResult<Self> {
        let device = VfioDevice::open_legacy(bdf)?;
        Self::build_warm_device(device, bdf, sm_version, compute_class, false)
    }

    fn build_warm_device(
        device: VfioDevice,
        bdf: &str,
        sm_version: u32,
        compute_class: u32,
        deferred_busmaster: bool,
    ) -> DriverResult<Self> {
        let container = device.dma_backend();
        let bar0 = device.map_bar(0)?;

        let (sm_version, compute_class) = Self::resolve_sm(&bar0, bdf, sm_version, compute_class)?;

        let profile = crate::nv::generation::profile_for_sm(sm_version);
        let seq = super::boot_sequence::boot_sequence_for(profile, sm_version);
        tracing::info!(
            sm_version,
            page_tables = ?profile.page_table_format,
            deferred_busmaster,
            needs_quiesce = seq.needs_pfifo_quiesce(),
            "warm handoff mode: PLLs preserved, reinitializing PGRAPH"
        );

        // Kepler warm handoff: quiesce PFIFO BEFORE enabling bus master.
        // After nouveau teardown, PFIFO retains stale DMA targets. If bus
        // master is on, PFIFO's DMA attempts cause IO_PAGE_FAULT cascades
        // that gate GPCs (0xbadf1100). Writing 0 to PFIFO_ENABLE (0x2504)
        // and PMC_SUBDEV_DISABLE (clearing PFIFO bit) stops stale DMA.
        if deferred_busmaster && seq.needs_pfifo_quiesce() {
            tracing::info!("warm: quiescing PFIFO before bus master enable");
            let _ = bar0.write_u32(0x2504, 0x0000_0000); // PFIFO_ENABLE = 0
            let _ = bar0.write_u32(0x2500, 0x0000_0000); // PFIFO_RUNLIST_DISABLE
            let _ = bar0.write_u32(0x3000, 0x0000_0000); // PFIFO_INTR_EN_0 = 0
            let _ = bar0.write_u32(0x3004, 0x0000_0000); // PFIFO_INTR_EN_1 = 0

            let gpc0_pre = bar0.read_u32(0x50_0000).unwrap_or(0xDEAD);
            tracing::info!(
                gpc0 = format_args!("{gpc0_pre:#010x}"),
                "GPC0 before bus master enable"
            );

            device.enable_bus_master()?;

            let gpc0_post = bar0.read_u32(0x50_0000).unwrap_or(0xDEAD);
            tracing::info!(
                gpc0 = format_args!("{gpc0_post:#010x}"),
                "GPC0 after bus master enable"
            );
        } else if deferred_busmaster {
            device.enable_bus_master()?;
        }

        // Map a guard page at IOVA 0x0 BEFORE starting firmware. FECS/PMU
        // firmware on K80 generates DMA to low IOVAs (e.g. 0x200) during boot.
        // Without a valid mapping, this triggers IO_PAGE_FAULT → IOMMU device
        // reset, destroying all GPU state mid-initialization.
        let _guard_page = DmaBuffer::new(container.clone(), 4096, GUARD_PAGE_IOVA)
            .map_err(|e| {
                tracing::warn!("guard page at IOVA 0x0 failed (non-fatal): {e}");
                e
            })
            .ok();

        // Kepler warm: PGRAPH registers are PRI-faulted after nouveau teardown.
        // Reset PGRAPH, apply GR MMIO init, and boot FECS firmware.
        if seq.needs_pfifo_quiesce() {
            let guard = super::hardware_guard::GuardedBar::new(&bar0, 32).map_err(|r| {
                crate::error::DriverError::HardwareGuardRefusal(r.to_string().into())
            })?;
            kepler_warm_pfifo_diagnostics(&bar0);
            super::init::kepler_warm_gr_init(&guard, bdf);
            kepler_warm_pfifo_post_check(&bar0);
        }

        let gpfifo_ring = DmaBuffer::new(container.clone(), gpfifo::RING_SIZE, GPFIFO_IOVA)?;
        let userd = DmaBuffer::new(container.clone(), 4096, USERD_IOVA)?;

        let slm_buf = DmaBuffer::new(container.clone(), SLM_SIZE, SLM_IOVA)
            .map_err(|e| {
                tracing::warn!("SLM pool allocation failed (non-fatal): {e}");
                e
            })
            .ok();

        #[expect(
            clippy::cast_possible_truncation,
            reason = "GPFIFO entries constant always fits u32"
        )]
        let channel = seq.create_channel_warm(
            container.clone(),
            &bar0,
            GPFIFO_IOVA,
            gpfifo::ENTRIES as u32,
            USERD_IOVA,
            0,
        )?;

        let caps = profile.to_capabilities();
        let sem_fence = seq.uses_semaphore_fence();
        let (fence_buf, fence_pb_buf) = seq.alloc_fence_buffers(container.clone())?;

        let mut dev = Self {
            device,
            bar0,
            sm_version,
            compute_class,
            gpfifo_ring,
            gpfifo_put: 0,
            userd,
            channel,
            next_handle: 1,
            next_iova: USER_IOVA_BASE,
            container,
            buffers: std::collections::HashMap::new(),
            inflight: Vec::new(),
            caps,
            uses_semaphore_fence: sem_fence,
            fence_buf,
            fence_pb_buf,
            fence_value: 0,
            slm_buf,
            guard_page: _guard_page,
        };

        if seq.warm_restarts_falcons() {
            dev.restart_warm_falcons()?;

            match Self::probe_fecs_alive(&dev.bar0) {
                Ok(true) => tracing::info!("warm handoff: FECS alive probe PASSED"),
                Ok(false) => tracing::warn!(
                    "warm handoff: FECS not running after restart — dispatch may fail"
                ),
                Err(e) => tracing::error!("warm handoff: FECS probe failed: {e}"),
            }
        } else {
            dev.setup_gr_context_warm()?;
        }

        Ok(dev)
    }
}

fn kepler_warm_pfifo_diagnostics(bar0: &crate::vfio::device::MappedBar) {
    let reg_2600 = bar0.read_u32(0x2600).unwrap_or(0xDEAD);
    let reg_2100 = bar0.read_u32(0x2100).unwrap_or(0xDEAD);
    let reg_2200 = bar0.read_u32(0x2200).unwrap_or(0xDEAD);
    let reg_2504 = bar0.read_u32(0x2504).unwrap_or(0xDEAD);
    let pmc = bar0.read_u32(0x200).unwrap_or(0xDEAD);
    let sched_ok = reg_2600 != 0xDEAD && reg_2600 & 0xBAD0_0000 != 0xBAD0_0000;
    tracing::info!(
        pmc = format_args!("{pmc:#010x}"),
        reg_2100 = format_args!("{reg_2100:#010x}"),
        reg_2200 = format_args!("{reg_2200:#010x}"),
        reg_2504 = format_args!("{reg_2504:#010x}"),
        reg_2600 = format_args!("{reg_2600:#010x}"),
        sched_ok,
        "PFIFO scheduler probe BEFORE kepler_warm_gr_init"
    );

    if !sched_ok {
        kepler_warm_pfifo_recovery(bar0);
    }
}

fn kepler_warm_pfifo_recovery(bar0: &crate::vfio::device::MappedBar) {
    // PMC PFIFO reset (toggle bit 8)
    let pmc_v = bar0.read_u32(0x200).unwrap_or(0);
    let _ = bar0.write_u32(0x200, pmc_v & !(1u32 << 8));
    let _ = bar0.read_u32(0x200);
    std::thread::sleep(std::time::Duration::from_millis(10));
    let _ = bar0.write_u32(0x200, pmc_v | (1u32 << 8));
    let _ = bar0.read_u32(0x200);
    std::thread::sleep(std::time::Duration::from_millis(20));

    let r2600 = bar0.read_u32(0x2600).unwrap_or(0xDEAD);
    if r2600 & 0xBAD0_0000 != 0xBAD0_0000 {
        tracing::info!(reg_2600 = format_args!("{r2600:#010x}"), "PFIFO: PMC toggle recovered scheduler");
        return;
    }

    // Full PMC cycle (all engines off then on)
    let _ = bar0.write_u32(0x200, 0);
    let _ = bar0.read_u32(0x200);
    std::thread::sleep(std::time::Duration::from_millis(50));
    let _ = bar0.write_u32(0x200, pmc_v);
    let _ = bar0.read_u32(0x200);
    std::thread::sleep(std::time::Duration::from_millis(50));

    // PRI ring re-enumerate after mass reset
    let _ = bar0.write_u32(0x12_004C, 0x0000_0002);
    std::thread::sleep(std::time::Duration::from_millis(20));
    let _ = bar0.write_u32(0x12_004C, 0x0000_0004);
    for _ in 0..200 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        let st = bar0.read_u32(0x12_0058).unwrap_or(0xDEAD);
        if st == 0 || (st != 0xDEAD && st & 0x8000_0000 == 0) {
            break;
        }
    }

    let r2600 = bar0.read_u32(0x2600).unwrap_or(0xDEAD);
    tracing::info!(
        reg_2600 = format_args!("{r2600:#010x}"),
        ok = r2600 & 0xBAD0_0000 != 0xBAD0_0000,
        "PFIFO: full PMC cycle + PRI re-enumerate"
    );
}

fn kepler_warm_pfifo_post_check(bar0: &crate::vfio::device::MappedBar) {
    let reg_2600 = bar0.read_u32(0x2600).unwrap_or(0xDEAD);
    let reg_2100 = bar0.read_u32(0x2100).unwrap_or(0xDEAD);
    let reg_2200 = bar0.read_u32(0x2200).unwrap_or(0xDEAD);
    tracing::info!(
        reg_2100 = format_args!("{reg_2100:#010x}"),
        reg_2200 = format_args!("{reg_2200:#010x}"),
        reg_2600 = format_args!("{reg_2600:#010x}"),
        sched_ok = reg_2600 != 0xDEAD && reg_2600 & 0xBAD0_0000 != 0xBAD0_0000,
        "PFIFO scheduler probe AFTER kepler_warm_gr_init"
    );
}
