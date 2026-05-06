// SPDX-License-Identifier: AGPL-3.0-or-later
//! NVIDIA GPU generation profiles — single source of truth for per-generation knowledge.
//!
//! Every property that varies by GPU generation (QMD version, channel class,
//! compute class, local memory window, completion strategy, boot strategy,
//! launch method) is consolidated into [`GenerationProfile`]. All code that
//! previously branched on raw SM numbers now consults [`profile_for_sm`].
//!
//! Adding a new generation = one new `const GenerationProfile`, zero new
//! match arms scattered across the codebase.

use std::ops::RangeInclusive;

/// QMD (Queue Management Descriptor) version for this generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QmdVersion {
    /// 256-byte (64-word) — Pascal and earlier.
    V21,
    /// 256-byte (64-word) — Volta, Turing.
    V22,
    /// 256-byte (64-word) — Ampere (GA100/GA10x). NVK/CUDA confirmed v2.3.
    V23,
    /// 256-byte (64-word) — Ada, Hopper.
    V30,
    /// 384-byte (96-word) — Blackwell and later.
    V50,
}

/// Push buffer launch method for compute dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMethod {
    /// `SEND_SIGNALING_PCAS_B` (0x02BC) — Volta, Turing.
    Pcas,
    /// `SEND_SIGNALING_PCAS2_B` (0x02C0) — Ampere and later.
    Pcas2,
}

/// GPFIFO completion tracking strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionStrategy {
    /// Poll USERD GP_GET until it matches GP_PUT — pre-Blackwell.
    GpGetPoll,
    /// Semaphore release via compute engine — Blackwell+ (GP_GET removed from USERD).
    SemaphoreFence,
}

/// Sovereign boot strategy for VFIO / direct hardware init.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootStrategy {
    /// Kepler: no ACR/WPR, direct PIO falcon upload.
    NoAcr,
    /// Volta/Pascal/Turing/Ampere/Ada: SEC2 DMA → ACR chain → FECS release.
    AcrSec2,
    /// Blackwell+: kernel module `GPU_PROMOTE_CTX` for GR context buffers.
    KmodPromote,
    /// Generation exists but sovereign boot path is untested.
    Untested,
}

pub use crate::hardware::MemoryType;

/// Source of `@builtin(num_workgroups)` in compiled shaders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NctaidSource {
    /// S2R NCTAID_X/Y/Z system registers (pre-Blackwell).
    SystemRegister,
    /// LDC c\[7\]\[0/4/8\] driver constant buffer (Blackwell+, S2R NCTAID broken).
    DriverCbuf7,
}

/// GPU MMU page table format used by this generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageTableFormat {
    /// Fermi/Kepler 2-level (PD + PT) with 40-bit physical address.
    /// PDE: 8 bytes, `[2:0]=target, [4]=present`. PTE: 8 bytes, `[0]=present, [2:1]=target`.
    /// 128 TB VA space = 1 PD level + 1 PT level. Pages = 4 KiB small / 128 KiB big.
    V1TwoLevel,
    /// GP100+ 5-level (PD3→PD2→PD1→PD0→PT) with V2 PDE/PTE encoding.
    /// PDEs: `addr >> 4 | aperture | VOL`. PD0 dual-entry (16 bytes: small + large).
    /// 128 TB VA space. Pages = 4 KiB small / 64 KiB big.
    V2FiveLevel,
}

/// Instance block layout for the PFIFO channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceBlockFormat {
    /// Kepler/Maxwell: RAMFC + simple PDB at 0x200. No subcontext array.
    /// `PAGE_DIR_BASE` at RAMIN offset 0x200, `ADDR_LIMIT` at 0x208.
    Simple,
    /// Volta+: RAMFC + PDB + 64-subcontext array.
    /// `SC_PDB_VALID` at 0x298, `SC0_PDB` at 0x2A0, etc.
    Subcontexted,
}

/// Runlist entry format and register layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunlistFormat {
    /// GK104/GK110: 8-byte channel entries, global RUNLIST_BASE/SUBMIT at 0x2270/0x2274.
    /// Entry: `[31:12]=INST_PTR, [9:8]=INST_TARGET, [0]=ENABLE`.
    Gk104Global,
    /// GV100+: 16-byte TSG header + 16-byte channel entries, per-runlist
    /// BASE/SUBMIT at stride 0x10 from 0x2270.
    Gv100PerRunlist,
}

/// Consolidated per-generation GPU knowledge.
///
/// Every property that varies by NVIDIA GPU generation is collected here.
/// Use [`profile_for_sm`] to look up the profile for a given SM version.
#[derive(Debug, Clone)]
pub struct GenerationProfile {
    /// Human-readable generation name.
    pub name: &'static str,
    /// SM architecture range this profile covers.
    pub sm_range: RangeInclusive<u32>,
    /// QMD layout version.
    pub qmd_version: QmdVersion,
    /// QMD size in u32 words (64 for V21-V30, 96 for V50).
    pub qmd_word_count: usize,
    /// RM channel class for GPFIFO allocation.
    pub channel_class: u32,
    /// RM compute engine class.
    pub compute_class: u32,
    /// Push buffer launch method (PCAS vs PCAS2).
    pub launch_method: LaunchMethod,
    /// Shader local memory window base address.
    pub local_mem_window: u64,
    /// GPFIFO completion tracking strategy.
    pub completion: CompletionStrategy,
    /// Sovereign VFIO boot strategy.
    pub boot_strategy: BootStrategy,
    /// GPU memory type.
    pub memory_type: MemoryType,
    /// Whether MUFU.RCP64H works in hardware (false on Blackwell).
    pub has_hardware_f64_rcp: bool,
    /// Source of `@builtin(num_workgroups)`.
    pub nctaid_source: NctaidSource,
    /// Whether USERD contains GP_GET (false on Blackwell).
    pub userd_gp_get: bool,
    /// Chip codename for firmware directory lookup.
    pub firmware_chip: &'static str,
    /// Whether this generation has full-rate FP64 (1:2 ratio with FP32).
    /// True for HPC variants (Kepler GK110/210, Pascal GP100, Volta GV100,
    /// Ampere GA100, Hopper GH100, Blackwell GB100).
    pub has_full_rate_fp64: bool,
    /// Recommended workgroup size (threads) for compute dispatch.
    pub recommended_workgroup_size: u32,
    /// Maximum concurrent CTAs (workgroups) per SM.
    pub max_cta_per_sm: u32,
    /// GPU MMU page table format.
    pub page_table_format: PageTableFormat,
    /// Instance block layout (simple vs subcontexted).
    pub instance_block_format: InstanceBlockFormat,
    /// Runlist entry format and register programming.
    pub runlist_format: RunlistFormat,
}

const LOCAL_MEM_WINDOW_LEGACY: u64 = 0xFF00_0000;
const LOCAL_MEM_WINDOW_VOLTA: u64 = 0xFF00_0000_0000_0000;

/// Kepler (GK110/GK210) — Tesla K40, Tesla K80.
pub const KEPLER: GenerationProfile = GenerationProfile {
    name: "Kepler",
    sm_range: 35..=37,
    qmd_version: QmdVersion::V21,
    qmd_word_count: 64,
    channel_class: 0xA06F, // KEPLER_CHANNEL_GPFIFO_A
    compute_class: 0xA1C0, // KEPLER_COMPUTE_B
    launch_method: LaunchMethod::Pcas,
    local_mem_window: LOCAL_MEM_WINDOW_LEGACY,
    completion: CompletionStrategy::GpGetPoll,
    boot_strategy: BootStrategy::NoAcr,
    memory_type: MemoryType::Gddr5,
    has_hardware_f64_rcp: true,
    nctaid_source: NctaidSource::SystemRegister,
    userd_gp_get: true,
    firmware_chip: "gk210",
    has_full_rate_fp64: true,
    recommended_workgroup_size: 128,
    max_cta_per_sm: 32,
    page_table_format: PageTableFormat::V1TwoLevel,
    instance_block_format: InstanceBlockFormat::Simple,
    runlist_format: RunlistFormat::Gk104Global,
};

/// Maxwell (GM200) — GTX 980 Ti, Titan X (Maxwell).
pub const MAXWELL: GenerationProfile = GenerationProfile {
    name: "Maxwell",
    sm_range: 50..=52,
    qmd_version: QmdVersion::V21,
    qmd_word_count: 64,
    channel_class: 0xB06F, // MAXWELL_CHANNEL_GPFIFO_A
    compute_class: 0xB0C0, // MAXWELL_COMPUTE_B
    launch_method: LaunchMethod::Pcas,
    local_mem_window: LOCAL_MEM_WINDOW_LEGACY,
    completion: CompletionStrategy::GpGetPoll,
    boot_strategy: BootStrategy::Untested,
    memory_type: MemoryType::Gddr5,
    has_hardware_f64_rcp: true,
    nctaid_source: NctaidSource::SystemRegister,
    userd_gp_get: true,
    firmware_chip: "gm200",
    has_full_rate_fp64: false,
    recommended_workgroup_size: 128,
    max_cta_per_sm: 32,
    page_table_format: PageTableFormat::V1TwoLevel,
    instance_block_format: InstanceBlockFormat::Simple,
    runlist_format: RunlistFormat::Gk104Global,
};

/// Pascal (GP100/GP102) — GTX 1080, Tesla P100.
///
/// `has_full_rate_fp64` is true because this profile covers GP100 (1:2).
/// Consumer GP10x (1:32) shares SM 6.x but uses different device IDs.
pub const PASCAL: GenerationProfile = GenerationProfile {
    name: "Pascal",
    sm_range: 60..=62,
    qmd_version: QmdVersion::V21,
    qmd_word_count: 64,
    channel_class: 0xC06F, // PASCAL_CHANNEL_GPFIFO_A
    compute_class: 0xC0C0, // PASCAL_COMPUTE_A
    launch_method: LaunchMethod::Pcas,
    local_mem_window: LOCAL_MEM_WINDOW_LEGACY,
    completion: CompletionStrategy::GpGetPoll,
    boot_strategy: BootStrategy::AcrSec2,
    memory_type: MemoryType::Hbm2,
    has_hardware_f64_rcp: true,
    nctaid_source: NctaidSource::SystemRegister,
    userd_gp_get: true,
    firmware_chip: "gp100",
    has_full_rate_fp64: true,
    recommended_workgroup_size: 256,
    max_cta_per_sm: 32,
    page_table_format: PageTableFormat::V2FiveLevel,
    instance_block_format: InstanceBlockFormat::Simple,
    runlist_format: RunlistFormat::Gk104Global,
};

/// Volta (GV100) — Titan V, Tesla V100.
pub const VOLTA: GenerationProfile = GenerationProfile {
    name: "Volta",
    sm_range: 70..=74,
    qmd_version: QmdVersion::V22,
    qmd_word_count: 64,
    channel_class: 0xC36F, // VOLTA_CHANNEL_GPFIFO_A
    compute_class: 0xC3C0, // VOLTA_COMPUTE_A
    launch_method: LaunchMethod::Pcas,
    local_mem_window: LOCAL_MEM_WINDOW_VOLTA,
    completion: CompletionStrategy::GpGetPoll,
    boot_strategy: BootStrategy::AcrSec2,
    memory_type: MemoryType::Hbm2,
    has_hardware_f64_rcp: true,
    nctaid_source: NctaidSource::SystemRegister,
    userd_gp_get: true,
    firmware_chip: "gv100",
    has_full_rate_fp64: true,
    recommended_workgroup_size: 128,
    max_cta_per_sm: 32,
    page_table_format: PageTableFormat::V2FiveLevel,
    instance_block_format: InstanceBlockFormat::Subcontexted,
    runlist_format: RunlistFormat::Gv100PerRunlist,
};

/// Turing (TU102/TU104/TU106) — RTX 2080, Tesla T4.
pub const TURING: GenerationProfile = GenerationProfile {
    name: "Turing",
    sm_range: 75..=79,
    qmd_version: QmdVersion::V22,
    qmd_word_count: 64,
    channel_class: 0xC36F, // VOLTA_CHANNEL_GPFIFO_A (shared with Volta)
    compute_class: 0xC5C0, // TURING_COMPUTE_A
    launch_method: LaunchMethod::Pcas,
    local_mem_window: LOCAL_MEM_WINDOW_VOLTA,
    completion: CompletionStrategy::GpGetPoll,
    boot_strategy: BootStrategy::AcrSec2,
    memory_type: MemoryType::Gddr6,
    has_hardware_f64_rcp: true,
    nctaid_source: NctaidSource::SystemRegister,
    userd_gp_get: true,
    firmware_chip: "tu102",
    has_full_rate_fp64: false,
    recommended_workgroup_size: 256,
    max_cta_per_sm: 16,
    page_table_format: PageTableFormat::V2FiveLevel,
    instance_block_format: InstanceBlockFormat::Subcontexted,
    runlist_format: RunlistFormat::Gv100PerRunlist,
};

/// Ampere A (GA100) — A100 datacenter.
pub const AMPERE_A: GenerationProfile = GenerationProfile {
    name: "Ampere A",
    sm_range: 80..=80,
    qmd_version: QmdVersion::V23,
    qmd_word_count: 64,
    channel_class: 0xC56F, // AMPERE_CHANNEL_GPFIFO_A
    compute_class: 0xC6C0, // AMPERE_COMPUTE_A
    launch_method: LaunchMethod::Pcas2,
    local_mem_window: LOCAL_MEM_WINDOW_VOLTA,
    completion: CompletionStrategy::GpGetPoll,
    boot_strategy: BootStrategy::AcrSec2,
    memory_type: MemoryType::Hbm2,
    has_hardware_f64_rcp: true,
    nctaid_source: NctaidSource::SystemRegister,
    userd_gp_get: true,
    firmware_chip: "ga100",
    has_full_rate_fp64: true,
    recommended_workgroup_size: 256,
    max_cta_per_sm: 16,
    page_table_format: PageTableFormat::V2FiveLevel,
    instance_block_format: InstanceBlockFormat::Subcontexted,
    runlist_format: RunlistFormat::Gv100PerRunlist,
};

/// Ampere B (GA102/GA104/GA106/GA107) — RTX 3090, RTX 3080, etc.
pub const AMPERE_B: GenerationProfile = GenerationProfile {
    name: "Ampere B",
    sm_range: 81..=88,
    qmd_version: QmdVersion::V23,
    qmd_word_count: 64,
    channel_class: 0xC56F, // AMPERE_CHANNEL_GPFIFO_A
    compute_class: 0xC7C0, // AMPERE_COMPUTE_B
    launch_method: LaunchMethod::Pcas2,
    local_mem_window: LOCAL_MEM_WINDOW_VOLTA,
    completion: CompletionStrategy::GpGetPoll,
    boot_strategy: BootStrategy::AcrSec2,
    memory_type: MemoryType::Gddr6x,
    has_hardware_f64_rcp: true,
    nctaid_source: NctaidSource::SystemRegister,
    userd_gp_get: true,
    firmware_chip: "ga102",
    has_full_rate_fp64: false,
    recommended_workgroup_size: 256,
    max_cta_per_sm: 16,
    page_table_format: PageTableFormat::V2FiveLevel,
    instance_block_format: InstanceBlockFormat::Subcontexted,
    runlist_format: RunlistFormat::Gv100PerRunlist,
};

/// Ada Lovelace (AD102/AD103/AD104) — RTX 4090, RTX 4080, etc.
pub const ADA: GenerationProfile = GenerationProfile {
    name: "Ada",
    sm_range: 89..=89,
    qmd_version: QmdVersion::V30,
    qmd_word_count: 64,
    channel_class: 0xC56F, // AMPERE_CHANNEL_GPFIFO_A (shared with Ampere)
    compute_class: 0xC9C0, // ADA_COMPUTE_A
    launch_method: LaunchMethod::Pcas2,
    local_mem_window: LOCAL_MEM_WINDOW_VOLTA,
    completion: CompletionStrategy::GpGetPoll,
    boot_strategy: BootStrategy::AcrSec2,
    memory_type: MemoryType::Gddr6x,
    has_hardware_f64_rcp: true,
    nctaid_source: NctaidSource::SystemRegister,
    userd_gp_get: true,
    firmware_chip: "ad102",
    has_full_rate_fp64: false,
    recommended_workgroup_size: 256,
    max_cta_per_sm: 16,
    page_table_format: PageTableFormat::V2FiveLevel,
    instance_block_format: InstanceBlockFormat::Subcontexted,
    runlist_format: RunlistFormat::Gv100PerRunlist,
};

/// Hopper (GH100) — H100, H200 datacenter.
pub const HOPPER: GenerationProfile = GenerationProfile {
    name: "Hopper",
    sm_range: 90..=99,
    qmd_version: QmdVersion::V30,
    qmd_word_count: 64,
    channel_class: 0xC56F, // AMPERE_CHANNEL_GPFIFO_A (shared)
    compute_class: 0xCBC0, // HOPPER_COMPUTE_A
    launch_method: LaunchMethod::Pcas2,
    local_mem_window: LOCAL_MEM_WINDOW_VOLTA,
    completion: CompletionStrategy::GpGetPoll,
    boot_strategy: BootStrategy::Untested,
    memory_type: MemoryType::Hbm2,
    has_hardware_f64_rcp: true,
    nctaid_source: NctaidSource::SystemRegister,
    userd_gp_get: true,
    firmware_chip: "gh100",
    has_full_rate_fp64: true,
    recommended_workgroup_size: 256,
    max_cta_per_sm: 16,
    page_table_format: PageTableFormat::V2FiveLevel,
    instance_block_format: InstanceBlockFormat::Subcontexted,
    runlist_format: RunlistFormat::Gv100PerRunlist,
};

/// Blackwell A (GB100/GB102) — B100, B200 datacenter.
pub const BLACKWELL_A: GenerationProfile = GenerationProfile {
    name: "Blackwell A",
    sm_range: 100..=119,
    qmd_version: QmdVersion::V50,
    qmd_word_count: 96,
    channel_class: 0xC96F, // BLACKWELL_CHANNEL_GPFIFO_A
    compute_class: 0xCDC0, // BLACKWELL_COMPUTE_A
    launch_method: LaunchMethod::Pcas2,
    local_mem_window: LOCAL_MEM_WINDOW_VOLTA,
    completion: CompletionStrategy::SemaphoreFence,
    boot_strategy: BootStrategy::KmodPromote,
    memory_type: MemoryType::Hbm2,
    has_hardware_f64_rcp: false,
    nctaid_source: NctaidSource::DriverCbuf7,
    userd_gp_get: false,
    firmware_chip: "gb100",
    has_full_rate_fp64: true,
    recommended_workgroup_size: 256,
    max_cta_per_sm: 16,
    page_table_format: PageTableFormat::V2FiveLevel,
    instance_block_format: InstanceBlockFormat::Subcontexted,
    runlist_format: RunlistFormat::Gv100PerRunlist,
};

/// Blackwell B (GB202/GB203/GB205/GB206/GB207) — RTX 5090, RTX 5080, RTX 5060, etc.
pub const BLACKWELL_B: GenerationProfile = GenerationProfile {
    name: "Blackwell B",
    sm_range: 120..=u32::MAX,
    qmd_version: QmdVersion::V50,
    qmd_word_count: 96,
    channel_class: 0xC96F, // BLACKWELL_CHANNEL_GPFIFO_A (CUDA R580 trace: 0xC96F for all Blackwell)
    compute_class: 0xCEC0, // BLACKWELL_COMPUTE_B
    launch_method: LaunchMethod::Pcas2,
    local_mem_window: LOCAL_MEM_WINDOW_VOLTA,
    completion: CompletionStrategy::SemaphoreFence,
    boot_strategy: BootStrategy::KmodPromote,
    memory_type: MemoryType::Gddr7,
    has_hardware_f64_rcp: false,
    nctaid_source: NctaidSource::DriverCbuf7,
    userd_gp_get: false,
    firmware_chip: "gb202",
    has_full_rate_fp64: false,
    recommended_workgroup_size: 256,
    max_cta_per_sm: 16,
    page_table_format: PageTableFormat::V2FiveLevel,
    instance_block_format: InstanceBlockFormat::Subcontexted,
    runlist_format: RunlistFormat::Gv100PerRunlist,
};

/// All known generation profiles, ordered by SM range.
const ALL_PROFILES: &[&GenerationProfile] = &[
    &KEPLER,
    &MAXWELL,
    &PASCAL,
    &VOLTA,
    &TURING,
    &AMPERE_A,
    &AMPERE_B,
    &ADA,
    &HOPPER,
    &BLACKWELL_A,
    &BLACKWELL_B,
];

/// Look up the generation profile for a given SM version.
///
/// This is the ONE lookup — all code that previously branched on raw SM
/// numbers should call this instead.
///
/// Falls back to Volta for unrecognized SM versions (matching the existing
/// default behavior across the codebase).
#[must_use]
pub fn profile_for_sm(sm: u32) -> &'static GenerationProfile {
    for profile in ALL_PROFILES {
        if profile.sm_range.contains(&sm) {
            return profile;
        }
    }
    &VOLTA
}

/// Check whether a profile represents a Kepler-class GPU.
#[must_use]
pub const fn is_kepler(profile: &GenerationProfile) -> bool {
    matches!(profile.boot_strategy, BootStrategy::NoAcr)
}

/// Check whether a profile uses semaphore-based GPFIFO completion.
#[must_use]
pub const fn uses_semaphore_fence(profile: &GenerationProfile) -> bool {
    matches!(profile.completion, CompletionStrategy::SemaphoreFence)
}

impl GenerationProfile {
    /// Build vendor-agnostic [`HardwareCapabilities`] from this NVIDIA profile.
    #[must_use]
    pub fn to_capabilities(&self) -> crate::HardwareCapabilities {
        use crate::hardware::{CompletionStyle, Vendor, WaveSize};
        crate::HardwareCapabilities {
            vendor: Vendor::Nvidia,
            device_name: self.name,
            generation_name: self.name,
            has_hardware_f64: true,
            has_hardware_f64_rcp: self.has_hardware_f64_rcp,
            has_full_rate_fp64: self.has_full_rate_fp64,
            native_wave_size: WaveSize::Wave32,
            memory_type: self.memory_type,
            completion_style: match self.completion {
                CompletionStrategy::GpGetPoll => CompletionStyle::RegisterPoll,
                CompletionStrategy::SemaphoreFence => CompletionStyle::DeviceFence,
            },
            max_shared_mem_bytes: 49152,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kepler_k80_profile() {
        let p = profile_for_sm(37);
        assert_eq!(p.name, "Kepler");
        assert_eq!(p.compute_class, 0xA1C0);
        assert_eq!(p.qmd_version, QmdVersion::V21);
        assert_eq!(p.boot_strategy, BootStrategy::NoAcr);
        assert_eq!(p.memory_type, MemoryType::Gddr5);
        assert!(p.has_hardware_f64_rcp);
        assert!(p.userd_gp_get);
    }

    #[test]
    fn volta_titanv_profile() {
        let p = profile_for_sm(70);
        assert_eq!(p.name, "Volta");
        assert_eq!(p.compute_class, 0xC3C0);
        assert_eq!(p.qmd_version, QmdVersion::V22);
        assert_eq!(p.boot_strategy, BootStrategy::AcrSec2);
        assert_eq!(p.local_mem_window, 0xFF00_0000_0000_0000);
        assert!(p.has_hardware_f64_rcp);
    }

    #[test]
    fn blackwell_5060_profile() {
        let p = profile_for_sm(120);
        assert_eq!(p.name, "Blackwell B");
        assert_eq!(p.compute_class, 0xCEC0);
        assert_eq!(p.channel_class, 0xCA6F);
        assert_eq!(p.qmd_version, QmdVersion::V50);
        assert_eq!(p.qmd_word_count, 96);
        assert_eq!(p.completion, CompletionStrategy::SemaphoreFence);
        assert_eq!(p.nctaid_source, NctaidSource::DriverCbuf7);
        assert!(!p.has_hardware_f64_rcp);
        assert!(!p.userd_gp_get);
    }

    #[test]
    fn turing_profile() {
        let p = profile_for_sm(75);
        assert_eq!(p.name, "Turing");
        assert_eq!(p.compute_class, 0xC5C0);
        assert_eq!(p.launch_method, LaunchMethod::Pcas);
    }

    #[test]
    fn ampere_split() {
        let a = profile_for_sm(80);
        assert_eq!(a.name, "Ampere A");
        assert_eq!(a.compute_class, 0xC6C0);

        let b = profile_for_sm(86);
        assert_eq!(b.name, "Ampere B");
        assert_eq!(b.compute_class, 0xC7C0);

        assert_eq!(a.launch_method, LaunchMethod::Pcas2);
        assert_eq!(b.launch_method, LaunchMethod::Pcas2);
    }

    #[test]
    fn ada_profile() {
        let p = profile_for_sm(89);
        assert_eq!(p.name, "Ada");
        assert_eq!(p.compute_class, 0xC9C0);
    }

    #[test]
    fn hopper_profile() {
        let p = profile_for_sm(90);
        assert_eq!(p.name, "Hopper");
        assert_eq!(p.compute_class, 0xCBC0);
    }

    #[test]
    fn blackwell_datacenter_profile() {
        let p = profile_for_sm(100);
        assert_eq!(p.name, "Blackwell A");
        assert_eq!(p.compute_class, 0xCDC0);
        assert_eq!(p.channel_class, 0xC96F);
    }

    #[test]
    fn unknown_sm_falls_back_to_volta() {
        let p = profile_for_sm(999);
        assert_eq!(p.name, "Blackwell B");
    }

    #[test]
    fn all_profiles_cover_known_generations() {
        let known_sms = [35, 37, 50, 60, 70, 75, 80, 86, 89, 90, 100, 120];
        for sm in known_sms {
            let p = profile_for_sm(sm);
            assert!(
                p.sm_range.contains(&sm),
                "SM {sm} should be in range {:?} ({})",
                p.sm_range,
                p.name
            );
        }
    }

    /// Profile compute classes are the authoritative source; the legacy
    /// identity table (`sm_to_compute_class`) is coarser-grained and
    /// incorrect for SM 80+ (Ada/Hopper/Blackwell have wrong class IDs).
    /// Once the identity table delegates through `profile_for_sm`, all
    /// SM values will match. For now, only verify Kepler–Turing.
    #[test]
    fn compute_class_matches_identity_table_where_aligned() {
        use crate::nv::identity::sm_to_compute_class;
        let aligned_sms = [35, 50, 60, 70, 75, 80];
        for sm in aligned_sms {
            let profile_class = profile_for_sm(sm).compute_class;
            let identity_class = sm_to_compute_class(sm);
            assert_eq!(
                profile_class, identity_class,
                "SM {sm}: profile={profile_class:#06X} vs identity={identity_class:#06X}"
            );
        }
    }

    #[test]
    fn firmware_chip_matches_identity() {
        use crate::nv::identity::chip_name;
        let sms = [35, 50, 60, 70, 75, 80, 86, 89, 90, 100, 120];
        for sm in sms {
            let profile_chip = profile_for_sm(sm).firmware_chip;
            let identity_chip = chip_name(sm);
            assert_eq!(
                profile_chip, identity_chip,
                "SM {sm}: profile={profile_chip} vs identity={identity_chip}"
            );
        }
    }

    #[test]
    fn kepler_uses_v1_two_level_pt() {
        let p = profile_for_sm(37);
        assert_eq!(p.page_table_format, PageTableFormat::V1TwoLevel);
        assert_eq!(p.instance_block_format, InstanceBlockFormat::Simple);
        assert_eq!(p.runlist_format, RunlistFormat::Gk104Global);
    }

    #[test]
    fn volta_uses_v2_five_level_pt() {
        let p = profile_for_sm(70);
        assert_eq!(p.page_table_format, PageTableFormat::V2FiveLevel);
        assert_eq!(p.instance_block_format, InstanceBlockFormat::Subcontexted);
        assert_eq!(p.runlist_format, RunlistFormat::Gv100PerRunlist);
    }

    #[test]
    fn pascal_uses_v2_pt_simple_instance() {
        let p = profile_for_sm(60);
        assert_eq!(p.page_table_format, PageTableFormat::V2FiveLevel);
        assert_eq!(p.instance_block_format, InstanceBlockFormat::Simple);
        assert_eq!(p.runlist_format, RunlistFormat::Gk104Global);
    }

    #[test]
    fn is_kepler_helper() {
        assert!(is_kepler(profile_for_sm(35)));
        assert!(is_kepler(profile_for_sm(37)));
        assert!(!is_kepler(profile_for_sm(70)));
        assert!(!is_kepler(profile_for_sm(120)));
    }

    #[test]
    fn uses_semaphore_fence_helper() {
        assert!(!uses_semaphore_fence(profile_for_sm(70)));
        assert!(!uses_semaphore_fence(profile_for_sm(89)));
        assert!(uses_semaphore_fence(profile_for_sm(100)));
        assert!(uses_semaphore_fence(profile_for_sm(120)));
    }
}
