// SPDX-License-Identifier: AGPL-3.0-or-later
//! VFIO compute device constructors and SM identity resolution.

use crate::error::{DriverError, DriverResult};
use crate::gsp::RegisterAccess;
use crate::vfio::channel::VfioChannel;
use crate::vfio::device::VfioDevice;
use crate::vfio::dma::DmaBuffer;

use super::NvVfioComputeDevice;
use super::layout::{
    GPFIFO_IOVA, USER_IOVA_BASE, USERD_IOVA, apply_error_to_driver, bar0_reg, gpfifo,
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
        let container = device.dma_backend();
        let bar0 = device.map_bar(0)?;

        let (sm_version, compute_class) = Self::resolve_sm(&bar0, bdf, sm_version, compute_class)?;

        NvVfioComputeDevice::apply_gr_bar0_init(&bar0, sm_version);

        let gpfifo_ring = DmaBuffer::new(container.clone(), gpfifo::RING_SIZE, GPFIFO_IOVA)?;
        let userd = DmaBuffer::new(container.clone(), 4096, USERD_IOVA)?;

        #[expect(
            clippy::cast_possible_truncation,
            reason = "GPFIFO entries constant always fits u32"
        )]
        let channel = VfioChannel::create(
            container.clone(),
            &bar0,
            GPFIFO_IOVA,
            gpfifo::ENTRIES as u32,
            USERD_IOVA,
            0,
        )?;

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

        NvVfioComputeDevice::apply_gr_bar0_init(&bar0, sm_version);

        let gpfifo_ring = DmaBuffer::new(container.clone(), gpfifo::RING_SIZE, GPFIFO_IOVA)?;
        let userd = DmaBuffer::new(container.clone(), 4096, USERD_IOVA)?;

        #[expect(
            clippy::cast_possible_truncation,
            reason = "GPFIFO entries constant always fits u32"
        )]
        let channel = VfioChannel::create(
            container.clone(),
            &bar0,
            GPFIFO_IOVA,
            gpfifo::ENTRIES as u32,
            USERD_IOVA,
            0,
        )?;

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
        let container = device.dma_backend();
        let bar0 = device.map_bar(0)?;

        let (sm_version, compute_class) = Self::resolve_sm(&bar0, bdf, sm_version, compute_class)?;

        tracing::info!("warm handoff mode: skipping GR BAR0 init (nouveau already configured)");

        let gpfifo_ring = DmaBuffer::new(container.clone(), gpfifo::RING_SIZE, GPFIFO_IOVA)?;
        let userd = DmaBuffer::new(container.clone(), 4096, USERD_IOVA)?;

        #[expect(
            clippy::cast_possible_truncation,
            reason = "GPFIFO entries constant always fits u32"
        )]
        let channel = VfioChannel::create_warm(
            container.clone(),
            &bar0,
            GPFIFO_IOVA,
            gpfifo::ENTRIES as u32,
            USERD_IOVA,
            0,
        )?;

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
        };

        dev.restart_warm_falcons()?;

        Ok(dev)
    }
}
