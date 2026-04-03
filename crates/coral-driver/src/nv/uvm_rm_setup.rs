// SPDX-License-Identifier: AGPL-3.0-only
//! UVM and RM client bootstrap: `/dev/nvidia-uvm`, RM root, device/subdevice, GPU UUID.

use crate::error::DriverResult;

use super::uvm::{
    NvGpuDevice, NvUvmDevice, RmClient, ADA_COMPUTE_A, AMPERE_CHANNEL_GPFIFO_A,
    AMPERE_COMPUTE_A, AMPERE_COMPUTE_B, BLACKWELL_CHANNEL_GPFIFO_B, BLACKWELL_COMPUTE_A,
    BLACKWELL_COMPUTE_B, HOPPER_COMPUTE_A, VOLTA_CHANNEL_GPFIFO_A, VOLTA_COMPUTE_A,
};

/// GPU generation derived from SM version, used for class selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::nv) enum GpuGen {
    Volta,
    Turing,
    /// GA100 (A100, SM 8.0) — uses `AMPERE_COMPUTE_A`.
    AmpereA,
    /// `GA10x` (RTX 30xx, SM 8.6+) — uses `AMPERE_COMPUTE_B`.
    AmpereB,
    /// AD10x (RTX 40xx, SM 8.9) — uses `ADA_COMPUTE_A`.
    Ada,
    /// GH100 (H100, SM 9.0) — uses `HOPPER_COMPUTE_A`.
    Hopper,
    /// GB100/200 (B200, SM 10.0) — data center Blackwell, `BLACKWELL_COMPUTE_A`.
    BlackwellA,
    /// GB20x (RTX 50xx, SM 12.0) — consumer Blackwell, `BLACKWELL_COMPUTE_B`.
    BlackwellB,
}

impl GpuGen {
    pub(in crate::nv) const fn from_sm(sm: u32) -> Self {
        match sm {
            75 => Self::Turing,
            80 => Self::AmpereA,
            81..=88 => Self::AmpereB,
            89 => Self::Ada,
            90 => Self::Hopper,
            100 => Self::BlackwellA,
            120.. => Self::BlackwellB,
            _ => Self::Volta,
        }
    }

    pub(in crate::nv) const fn channel_class(self) -> u32 {
        match self {
            Self::BlackwellA | Self::BlackwellB => BLACKWELL_CHANNEL_GPFIFO_B,
            Self::AmpereA | Self::AmpereB | Self::Ada | Self::Hopper => AMPERE_CHANNEL_GPFIFO_A,
            Self::Volta | Self::Turing => VOLTA_CHANNEL_GPFIFO_A,
        }
    }

    pub(in crate::nv) const fn compute_class(self) -> u32 {
        match self {
            Self::BlackwellA => BLACKWELL_COMPUTE_A,
            Self::BlackwellB => BLACKWELL_COMPUTE_B,
            Self::Hopper => HOPPER_COMPUTE_A,
            Self::Ada => ADA_COMPUTE_A,
            Self::AmpereA => AMPERE_COMPUTE_A,
            Self::AmpereB => AMPERE_COMPUTE_B,
            Self::Volta | Self::Turing => VOLTA_COMPUTE_A,
        }
    }
}

/// RM + UVM handles through GPU registration (`UVM_REGISTER_GPU`).
pub(in crate::nv) struct UvmRmInit {
    pub(in crate::nv) client: RmClient,
    pub(in crate::nv) uvm: NvUvmDevice,
    pub(in crate::nv) gpu: NvGpuDevice,
    pub(in crate::nv) gpu_gen: GpuGen,
    pub(in crate::nv) h_device: u32,
    pub(in crate::nv) h_subdevice: u32,
    pub(in crate::nv) gpu_uuid: [u8; 16],
}

/// Open `/dev/nvidia-uvm`, create the RM root client, allocate device/subdevice, register the GPU with UVM.
pub(in crate::nv) fn init_uvm_rm_client(gpu_index: u32, sm: u32) -> DriverResult<UvmRmInit> {
    let gpu_gen = GpuGen::from_sm(sm);

    let mut client = RmClient::new()?;
    let uvm = NvUvmDevice::open()?;
    let gpu = NvGpuDevice::open(gpu_index)?;
    gpu.register_fd(client.ctl_fd())?;

    uvm.initialize()?;

    let h_device = client.alloc_device(gpu_index)?;
    let h_subdevice = client.alloc_subdevice(h_device)?;

    let gpu_uuid = client.register_gpu_with_uvm(h_subdevice, &uvm)?;

    Ok(UvmRmInit {
        client,
        uvm,
        gpu,
        gpu_gen,
        h_device,
        h_subdevice,
        gpu_uuid,
    })
}
