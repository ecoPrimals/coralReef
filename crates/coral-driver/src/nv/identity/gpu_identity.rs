// SPDX-License-Identifier: AGPL-3.0-or-later

use super::constants::{PCI_VENDOR_AMD, PCI_VENDOR_NVIDIA};

/// PCI identity of a GPU device.
#[derive(Debug, Clone)]
pub struct GpuIdentity {
    /// PCI vendor ID (see [`PCI_VENDOR_NVIDIA`], [`PCI_VENDOR_AMD`]).
    pub vendor_id: u16,
    /// PCI device ID (maps to specific GPU model).
    pub device_id: u16,
    /// Sysfs device path.
    pub sysfs_path: String,
}

impl GpuIdentity {
    /// Map a known NVIDIA PCI device ID to an SM architecture version.
    ///
    /// Returns `None` for unrecognized device IDs. This table covers
    /// common consumer and professional GPUs.
    #[must_use]
    pub const fn nvidia_sm(&self) -> Option<u32> {
        if self.vendor_id != PCI_VENDOR_NVIDIA {
            return None;
        }
        match self.device_id {
            // Kepler GK110/GK110B/GK210
            0x1003..=0x1005 => Some(35), // GK110: Tesla K40/K20X variants
            0x100A | 0x100C => Some(35), // GK110B: Tesla K40/K80 single-die
            0x102D => Some(37),          // GK210GL: Tesla K80
            // Volta
            0x1D81 | 0x1DB1 | 0x1DB4 | 0x1DB5 | 0x1DB6 | 0x1DB7 => Some(70),
            // Turing
            0x1E02..=0x1E07
            | 0x1E30..=0x1E3D
            | 0x1E82..=0x1E87
            | 0x1F02..=0x1F15
            | 0x1F82..=0x1F95
            | 0x2182..=0x2191
            | 0x1E89..=0x1E93 => Some(75),
            // Ampere GA100
            0x2080 | 0x20B0..=0x20BF | 0x20F1..=0x20F5 => Some(80),
            // Ampere GA102/GA104/GA106/GA107
            0x2200..=0x2210
            | 0x2216
            | 0x2230..=0x2237
            | 0x2414
            | 0x2460..=0x2489
            | 0x2501..=0x2531
            | 0x2560..=0x2572
            | 0x2580..=0x25AC => Some(86),
            // Ada Lovelace AD102/AD103/AD104/AD106/AD107 (RTX 4090–4060)
            0x2600..=0x28FF => Some(89),
            // Hopper GH100 (H100 SXM/PCIe, H200)
            0x2321..=0x233F => Some(90),
            // Blackwell GB202 (RTX 5090), GB203 (RTX 5080), GB205 (RTX 5070 Ti),
            // GB206 (RTX 5070/5060 Ti), GB207 (RTX 5060)
            // PCI ID ranges: 0x29xx (initial), 0x2Bxx/0x2Cxx (Pro), 0x2Dxx (refresh)
            0x2900..=0x2999 | 0x2B00..=0x2DFF => Some(120),
            _ => None,
        }
    }

    /// Map a known AMD PCI device ID to an architecture identifier.
    ///
    /// Returns `None` for unrecognized device IDs. Covers RDNA 1/2/3
    /// and some GCN5 (Vega) devices.
    #[must_use]
    pub const fn amd_arch(&self) -> Option<&'static str> {
        if self.vendor_id != PCI_VENDOR_AMD {
            return None;
        }
        match self.device_id {
            // Vega 10/20 (GCN 5 / GFX9)
            0x6860..=0x687F | 0x66A0..=0x66AF => Some("gfx9"),
            // Navi 10 (RDNA 1 / GFX10.1)
            0x7310..=0x731F | 0x7340..=0x734F => Some("rdna1"),
            // Navi 21/22/23/24 (RDNA 2 / GFX10.3)
            0x73A0..=0x73BF | 0x73C0..=0x73DF | 0x73E0..=0x73FF | 0x7420..=0x743F => Some("rdna2"),
            // Navi 31/32/33 (RDNA 3 / GFX11)
            0x7440..=0x745F | 0x7460..=0x747F | 0x7480..=0x749F | 0x15BF..=0x15CF => Some("rdna3"),
            _ => None,
        }
    }
}
