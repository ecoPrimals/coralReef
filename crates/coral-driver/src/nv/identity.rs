// SPDX-License-Identifier: AGPL-3.0-only
//! GPU identity probing via sysfs — no ioctl dependencies.
//!
//! Reads PCI vendor/device IDs from `/sys/class/drm/` to identify the
//! GPU model and map it to an SM architecture version.

/// PCI identity of a GPU device.
#[derive(Debug, Clone)]
pub struct GpuIdentity {
    /// PCI vendor ID (0x10DE for NVIDIA).
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
        if self.vendor_id != 0x10DE {
            return None;
        }
        match self.device_id {
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
            | 0x2580..=0x25AC
            | 0x2684..=0x26B1
            | 0x2700..=0x2730
            | 0x2780..=0x2799
            | 0x2820..=0x2860
            | 0x2880..=0x2899 => Some(86),
            // Ada Lovelace AD102/AD103/AD104/AD106/AD107
            0x2600..=0x2683 => Some(89),
            _ => None,
        }
    }
}

/// Probe sysfs for the GPU chipset on a nouveau render node.
///
/// Looks for `/sys/class/drm/renderDN/device/` to identify the PCI device.
/// Returns the PCI vendor:device ID pair if readable.
#[must_use]
pub fn probe_gpu_identity(render_node_path: &str) -> Option<GpuIdentity> {
    let node_name = render_node_path.rsplit('/').next()?;
    let sysfs_device = format!("/sys/class/drm/{node_name}/device");

    let vendor = std::fs::read_to_string(format!("{sysfs_device}/vendor")).ok()?;
    let device = std::fs::read_to_string(format!("{sysfs_device}/device")).ok()?;

    let vendor_id = u16::from_str_radix(vendor.trim().trim_start_matches("0x"), 16).ok()?;
    let device_id = u16::from_str_radix(device.trim().trim_start_matches("0x"), 16).ok()?;

    Some(GpuIdentity {
        vendor_id,
        device_id,
        sysfs_path: sysfs_device,
    })
}

/// Check for NVIDIA firmware files required by nouveau for compute on Volta+.
///
/// Returns a list of (path, exists) for the firmware files that nouveau
/// typically needs.
#[must_use]
pub fn check_nouveau_firmware(chip: &str) -> Vec<(String, bool)> {
    let base = format!("/lib/firmware/nvidia/{chip}");
    let firmware_files = [
        "acr/bl.bin",
        "acr/ucode_unload.bin",
        "gr/fecs_bl.bin",
        "gr/fecs_inst.bin",
        "gr/fecs_data.bin",
        "gr/gpccs_bl.bin",
        "gr/gpccs_inst.bin",
        "gr/gpccs_data.bin",
        "gr/sw_ctx.bin",
        "gr/sw_nonctx.bin",
        "gr/sw_bundle_init.bin",
        "gr/sw_method_init.bin",
        "nvdec/scrubber.bin",
        "sec2/desc.bin",
        "sec2/image.bin",
        "sec2/sig.bin",
    ];

    firmware_files
        .iter()
        .map(|f| {
            let path = format!("{base}/{f}");
            let exists = std::path::Path::new(&path).exists();
            (path, exists)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_identity_nvidia_sm_mapping() {
        let titan_v = GpuIdentity {
            vendor_id: 0x10DE,
            device_id: 0x1D81,
            sysfs_path: String::new(),
        };
        assert_eq!(titan_v.nvidia_sm(), Some(70));

        let non_nvidia = GpuIdentity {
            vendor_id: 0x1002,
            device_id: 0x73BF,
            sysfs_path: String::new(),
        };
        assert_eq!(non_nvidia.nvidia_sm(), None);
    }

    #[test]
    fn nvidia_sm_turing_sm75() {
        // Turing: RTX 2080 (1E82), RTX 2060 (1F03), GTX 1660 Ti (2182)
        let rtx_2080 = GpuIdentity {
            vendor_id: 0x10DE,
            device_id: 0x1E82,
            sysfs_path: String::new(),
        };
        assert_eq!(rtx_2080.nvidia_sm(), Some(75));

        let rtx_2060 = GpuIdentity {
            vendor_id: 0x10DE,
            device_id: 0x1F03,
            sysfs_path: String::new(),
        };
        assert_eq!(rtx_2060.nvidia_sm(), Some(75));

        let gtx_1660_ti = GpuIdentity {
            vendor_id: 0x10DE,
            device_id: 0x2182,
            sysfs_path: String::new(),
        };
        assert_eq!(gtx_1660_ti.nvidia_sm(), Some(75));
    }

    #[test]
    fn nvidia_sm_ampere_ga100_vs_ga102() {
        // Ampere GA100 (A100): SM80
        let a100 = GpuIdentity {
            vendor_id: 0x10DE,
            device_id: 0x20B0,
            sysfs_path: String::new(),
        };
        assert_eq!(a100.nvidia_sm(), Some(80));

        // Ampere GA102 (RTX 3090/3080): SM86
        let rtx_3090 = GpuIdentity {
            vendor_id: 0x10DE,
            device_id: 0x2204,
            sysfs_path: String::new(),
        };
        assert_eq!(rtx_3090.nvidia_sm(), Some(86));

        let rtx_3080 = GpuIdentity {
            vendor_id: 0x10DE,
            device_id: 0x2206,
            sysfs_path: String::new(),
        };
        assert_eq!(rtx_3080.nvidia_sm(), Some(86));
    }

    #[test]
    fn nvidia_sm_ada_lovelace_sm89() {
        // Ada Lovelace AD102/AD103/AD104 (0x2600..=0x2683 maps to SM89)
        let ada_ad102 = GpuIdentity {
            vendor_id: 0x10DE,
            device_id: 0x2680,
            sysfs_path: String::new(),
        };
        assert_eq!(ada_ad102.nvidia_sm(), Some(89));

        let ada_ad103 = GpuIdentity {
            vendor_id: 0x10DE,
            device_id: 0x2682,
            sysfs_path: String::new(),
        };
        assert_eq!(ada_ad103.nvidia_sm(), Some(89));

        let ada_ad104 = GpuIdentity {
            vendor_id: 0x10DE,
            device_id: 0x2683,
            sysfs_path: String::new(),
        };
        assert_eq!(ada_ad104.nvidia_sm(), Some(89));
    }

    #[test]
    fn nvidia_sm_unknown_device_id_returns_none() {
        let unknown = GpuIdentity {
            vendor_id: 0x10DE,
            device_id: 0x9999,
            sysfs_path: String::new(),
        };
        assert_eq!(unknown.nvidia_sm(), None);

        let fake_vendor = GpuIdentity {
            vendor_id: 0x1234,
            device_id: 0x1D81,
            sysfs_path: String::new(),
        };
        assert_eq!(fake_vendor.nvidia_sm(), None);
    }

    #[test]
    fn probe_gpu_identity_nonexistent_path_returns_none() {
        // Path parses to node "renderD99999"; /sys/class/drm/renderD99999/device won't exist
        let result = probe_gpu_identity("/tmp/fake/renderD99999");
        assert!(result.is_none());
    }

    #[test]
    fn firmware_check_returns_entries() {
        let entries = check_nouveau_firmware("gv100");
        assert!(!entries.is_empty());
        for (path, _exists) in &entries {
            assert!(path.contains("gv100"));
        }
    }
}
