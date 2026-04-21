// SPDX-License-Identifier: AGPL-3.0-or-later
//! Vendor-agnostic hardware capabilities.
//!
//! [`HardwareCapabilities`] is the universal capability surface that every
//! [`ComputeDevice`](crate::ComputeDevice) exposes. Vendor-specific profiles
//! (`nv::GenerationProfile`, `amd::AmdGenerationProfile`) build these
//! capabilities from their own domain knowledge, but consumers only see
//! the vendor-agnostic struct.
//!
//! Adding a new vendor = one new generation module + one `capabilities()`
//! implementation. Consumers never branch on vendor identity.

/// GPU vendor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Vendor {
    /// NVIDIA (nouveau, proprietary, VFIO, CUDA).
    Nvidia,
    /// AMD (amdgpu DRM).
    Amd,
    /// Intel (future).
    Intel,
    /// Unrecognized or software backend.
    Unknown,
}

impl std::fmt::Display for Vendor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Nvidia => f.write_str("NVIDIA"),
            Self::Amd => f.write_str("AMD"),
            Self::Intel => f.write_str("Intel"),
            Self::Unknown => f.write_str("Unknown"),
        }
    }
}

/// GPU memory technology.
///
/// Shared across vendors — determines training/init pipeline and bandwidth
/// characteristics. Defined at crate root so both NV and AMD generation
/// profiles can reference it without cross-vendor imports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryType {
    /// GDDR5 (Kepler, Maxwell, older AMD).
    Gddr5,
    /// HBM2 / HBM2e (datacenter: Volta V100, Ampere A100, Vega MI50).
    Hbm2,
    /// HBM3 / HBM3e (datacenter: Hopper H100, MI300).
    Hbm3,
    /// GDDR6 (Turing, RDNA1/2).
    Gddr6,
    /// GDDR6X (Ampere B, Ada consumer).
    Gddr6x,
    /// GDDR7 (Blackwell consumer).
    Gddr7,
}

impl std::fmt::Display for MemoryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gddr5 => f.write_str("GDDR5"),
            Self::Hbm2 => f.write_str("HBM2"),
            Self::Hbm3 => f.write_str("HBM3"),
            Self::Gddr6 => f.write_str("GDDR6"),
            Self::Gddr6x => f.write_str("GDDR6X"),
            Self::Gddr7 => f.write_str("GDDR7"),
        }
    }
}

/// Native wave/warp execution width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WaveSize {
    /// 32 threads per warp (NVIDIA, RDNA wave32).
    Wave32,
    /// 64 threads per wave (GCN / CDNA wave64).
    Wave64,
    /// Hardware supports both widths (some RDNA can run wave32 or wave64).
    Configurable,
}

/// How the GPU signals dispatch completion to the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompletionStyle {
    /// Poll a register / USERD field until it advances (NV GP_GET pre-Blackwell).
    RegisterPoll,
    /// Device writes a fence value that the host polls or waits on
    /// (NV semaphore fence, AMD DRM fence, CUDA stream sync).
    DeviceFence,
}

/// Vendor-agnostic hardware capabilities.
///
/// Built from vendor-specific generation profiles at device-open time.
/// Consumers query this instead of branching on vendor identity or raw
/// architecture numbers.
#[derive(Debug, Clone)]
pub struct HardwareCapabilities {
    /// GPU vendor.
    pub vendor: Vendor,
    /// Short device/chip name (e.g. "Blackwell B", "Vega 20").
    pub device_name: &'static str,
    /// Generation / architecture family (e.g. "Kepler", "RDNA2").
    pub generation_name: &'static str,
    /// Whether the ALU supports IEEE 754 binary64 natively.
    pub has_hardware_f64: bool,
    /// Whether `MUFU.RCP64H` (or equivalent) produces correct results.
    /// False on Blackwell where the instruction is broken.
    pub has_hardware_f64_rcp: bool,
    /// Whether FP64 runs at full rate (1:2 ratio with FP32).
    /// False on consumer GPUs where FP64 is throttled (1:32 or 1:64).
    pub has_full_rate_fp64: bool,
    /// Native warp/wave width.
    pub native_wave_size: WaveSize,
    /// Video memory technology.
    pub memory_type: MemoryType,
    /// How dispatch completion is signalled.
    pub completion_style: CompletionStyle,
    /// Maximum shared (local) memory per workgroup in bytes.
    /// 0 means unknown / not yet probed.
    pub max_shared_mem_bytes: u32,
}

impl HardwareCapabilities {
    /// Placeholder capabilities for backends that haven't implemented
    /// introspection yet.
    pub const UNKNOWN: Self = Self {
        vendor: Vendor::Unknown,
        device_name: "unknown",
        generation_name: "unknown",
        has_hardware_f64: false,
        has_hardware_f64_rcp: false,
        has_full_rate_fp64: false,
        native_wave_size: WaveSize::Wave32,
        memory_type: MemoryType::Gddr6,
        completion_style: CompletionStyle::DeviceFence,
        max_shared_mem_bytes: 0,
    };
}

impl std::fmt::Display for HardwareCapabilities {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} ({}, {}, f64={})",
            self.vendor,
            self.device_name,
            self.generation_name,
            self.memory_type,
            self.has_hardware_f64,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_display() {
        assert_eq!(Vendor::Nvidia.to_string(), "NVIDIA");
        assert_eq!(Vendor::Amd.to_string(), "AMD");
        assert_eq!(Vendor::Intel.to_string(), "Intel");
        assert_eq!(Vendor::Unknown.to_string(), "Unknown");
    }

    #[test]
    fn memory_type_display() {
        assert_eq!(MemoryType::Gddr5.to_string(), "GDDR5");
        assert_eq!(MemoryType::Hbm2.to_string(), "HBM2");
        assert_eq!(MemoryType::Hbm3.to_string(), "HBM3");
        assert_eq!(MemoryType::Gddr7.to_string(), "GDDR7");
    }

    #[test]
    fn unknown_capabilities_are_conservative() {
        let caps = HardwareCapabilities::UNKNOWN;
        assert_eq!(caps.vendor, Vendor::Unknown);
        assert!(!caps.has_hardware_f64);
        assert!(!caps.has_hardware_f64_rcp);
        assert!(!caps.has_full_rate_fp64);
    }

    #[test]
    fn capabilities_display() {
        let caps = HardwareCapabilities {
            vendor: Vendor::Nvidia,
            device_name: "Blackwell B",
            generation_name: "Blackwell",
            has_hardware_f64: true,
            has_hardware_f64_rcp: false,
            has_full_rate_fp64: false,
            native_wave_size: WaveSize::Wave32,
            memory_type: MemoryType::Gddr7,
            completion_style: CompletionStyle::DeviceFence,
            max_shared_mem_bytes: 49152,
        };
        let s = caps.to_string();
        assert!(s.contains("NVIDIA"));
        assert!(s.contains("Blackwell B"));
        assert!(s.contains("GDDR7"));
    }

    #[test]
    fn vendor_equality_and_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(Vendor::Nvidia);
        set.insert(Vendor::Amd);
        assert!(set.contains(&Vendor::Nvidia));
        assert!(!set.contains(&Vendor::Intel));
    }
}
