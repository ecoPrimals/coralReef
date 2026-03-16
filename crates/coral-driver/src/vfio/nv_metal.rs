// SPDX-License-Identifier: AGPL-3.0-only
#![allow(missing_docs)]
//! NVIDIA Volta `GpuMetal` implementation.
//!
//! Provides register maps, power domains, engine topology, and warm-up
//! sequences specific to the GV100 (Titan V) and other Volta-class GPUs.
//! This is the reference implementation — AMD Vega will follow the same
//! trait structure with its own register offsets.

use super::bar_cartography::DomainHint;
use super::device::MappedBar;
use super::gpu_vendor::*;
use super::pci_discovery::GpuVendor;

// ── NVIDIA Volta register constants ─────────────────────────────────────

#[allow(dead_code)]
mod volta_regs {
    pub const BOOT0: usize = 0x0000_0000;
    pub const PMC_ENABLE: usize = 0x0000_0200;
    pub const PFIFO_ENABLE: usize = 0x0000_2200;
    pub const PBDMA_MAP: usize = 0x0000_2004;
    pub const BAR0_WINDOW: usize = 0x0000_1700;
    pub const BAR2_BLOCK: usize = 0x0000_1714;
    pub const PRAMIN_BASE: usize = 0x0070_0000;

    pub const PMU_BASE: usize = 0x0010_A000;
    pub const GR_BASE: usize = 0x0040_0000;
    pub const CE_BASE: usize = 0x0010_4000;
    pub const PDISP_BASE: usize = 0x0061_0000;
    pub const NVDEC_BASE: usize = 0x0008_4000;
    pub const NVENC_BASE: usize = 0x001C_8000;

    pub const FBPA0_BASE: usize = 0x009A_0000;
    pub const FBPA_STRIDE: usize = 0x0000_4000;
    pub const LTC_BASE: usize = 0x0017_E000;
    pub const LTC_STRIDE: usize = 0x0000_2000;

    pub const PCLOCK_BASE: usize = 0x0013_7000;
    pub const CLK_BASE: usize = 0x0013_2000;

    pub const THERMAL_BASE: usize = 0x0002_0400;
    pub const FUSE_BASE: usize = 0x0002_1000;
    pub const STRAP_BASE: usize = 0x0010_1000;

    pub const PFB_BASE: usize = 0x0010_0000;
    pub const PTIMER_BASE: usize = 0x0000_9000;
}

// ── NvVoltaIdentity ─────────────────────────────────────────────────────

/// Decoded NVIDIA GPU identity from BOOT0 register.
#[derive(Debug, Clone)]
pub struct NvVoltaIdentity {
    pub boot0: u32,
    pub chip_impl: u8,
    pub chip_rev: u8,
    pub chip_name_str: String,
    pub arch_name: String,
}

impl NvVoltaIdentity {
    /// Decode identity from a BOOT0 register value.
    pub fn from_boot0(boot0: u32) -> Self {
        let arch_code = ((boot0 >> 20) & 0x1FF) as u16;
        let chip_impl = ((boot0 >> 20) & 0xFF) as u8;
        let chip_rev = (boot0 & 0xFF) as u8;

        let (chip_name_str, arch_name) = match arch_code {
            0x140 => ("GV100".into(), "Volta".into()),
            0x142 => ("GV10B".into(), "Volta".into()),
            0x160..=0x16F => (format!("TU{:02X}", arch_code & 0xFF), "Turing".into()),
            0x170..=0x17F => (format!("GA{:02X}", arch_code & 0xFF), "Ampere".into()),
            0x190..=0x19F => (format!("AD{:02X}", arch_code & 0xFF), "Ada".into()),
            _ => (
                format!("NV{arch_code:03X}"),
                format!("Unknown({arch_code:#x})"),
            ),
        };

        Self {
            boot0,
            chip_impl,
            chip_rev,
            chip_name_str,
            arch_name,
        }
    }
}

impl GpuIdentity for NvVoltaIdentity {
    fn vendor(&self) -> GpuVendor {
        GpuVendor::Nvidia
    }
    fn chip_name(&self) -> &str {
        &self.chip_name_str
    }
    fn architecture(&self) -> &str {
        &self.arch_name
    }
    fn implementation(&self) -> u8 {
        self.chip_impl
    }
    fn revision(&self) -> u8 {
        self.chip_rev
    }
    fn raw_id(&self) -> u32 {
        self.boot0
    }
}

// ── NvVoltaMetal ────────────────────────────────────────────────────────

/// NVIDIA Volta `GpuMetal` implementation.
///
/// Covers GV100 (Titan V, Tesla V100) register layout, power domains,
/// engine topology, and warm-up sequence.
#[derive(Debug)]
pub struct NvVoltaMetal {
    identity: NvVoltaIdentity,
    power_domains: Vec<PowerDomain>,
    memory_regions: Vec<MetalMemoryRegion>,
    engines: Vec<EngineInfo>,
}

impl NvVoltaMetal {
    /// Create from a BOOT0 value read from BAR0 offset 0x0.
    pub fn from_boot0(boot0: u32) -> Self {
        let identity = NvVoltaIdentity::from_boot0(boot0);

        let power_domains = vec![
            PowerDomain {
                name: "GR".into(),
                enable_reg: Some(volta_regs::PMC_ENABLE),
                enable_bit: Some(1 << 12),
                clock_reg: None,
                state: DomainState::Unknown,
            },
            PowerDomain {
                name: "PFIFO".into(),
                enable_reg: Some(volta_regs::PMC_ENABLE),
                enable_bit: Some(1 << 8),
                clock_reg: None,
                state: DomainState::Unknown,
            },
            PowerDomain {
                name: "PBDMA".into(),
                enable_reg: Some(volta_regs::PMC_ENABLE),
                enable_bit: Some(1 << 13),
                clock_reg: None,
                state: DomainState::Unknown,
            },
            PowerDomain {
                name: "CE0".into(),
                enable_reg: Some(volta_regs::PMC_ENABLE),
                enable_bit: Some(1 << 6),
                clock_reg: None,
                state: DomainState::Unknown,
            },
            PowerDomain {
                name: "PMU".into(),
                enable_reg: Some(volta_regs::PMC_ENABLE),
                enable_bit: Some(1 << 24),
                clock_reg: None,
                state: DomainState::Unknown,
            },
            PowerDomain {
                name: "FB".into(),
                enable_reg: Some(volta_regs::PMC_ENABLE),
                enable_bit: Some(1 << 20),
                clock_reg: None,
                state: DomainState::Unknown,
            },
            PowerDomain {
                name: "LTC".into(),
                enable_reg: Some(volta_regs::PMC_ENABLE),
                enable_bit: Some(1 << 21),
                clock_reg: None,
                state: DomainState::Unknown,
            },
            PowerDomain {
                name: "DISP".into(),
                enable_reg: Some(volta_regs::PMC_ENABLE),
                enable_bit: Some(1 << 30),
                clock_reg: None,
                state: DomainState::Unknown,
            },
        ];

        let memory_regions = vec![
            MetalMemoryRegion {
                name: "VRAM (HBM2)".into(),
                kind: MemoryKind::Vram,
                control_base: Some(volta_regs::PFB_BASE),
                size: None,
                partitions: Some(4),
            },
            MetalMemoryRegion {
                name: "L2 Cache".into(),
                kind: MemoryKind::L2Cache,
                control_base: Some(volta_regs::LTC_BASE),
                size: None,
                partitions: Some(6),
            },
            MetalMemoryRegion {
                name: "PRAMIN Aperture".into(),
                kind: MemoryKind::Aperture,
                control_base: Some(volta_regs::PRAMIN_BASE),
                size: Some(1024 * 1024),
                partitions: None,
            },
        ];

        let engines = vec![
            EngineInfo {
                name: "GR (Graphics/Compute)".into(),
                kind: EngineKind::Compute,
                base_offset: volta_regs::GR_BASE,
                has_firmware: true,
                firmware_state: FirmwareState::NotLoaded,
            },
            EngineInfo {
                name: "CE0 (Copy Engine)".into(),
                kind: EngineKind::Copy,
                base_offset: volta_regs::CE_BASE,
                has_firmware: true,
                firmware_state: FirmwareState::NotLoaded,
            },
            EngineInfo {
                name: "PMU (Power Management)".into(),
                kind: EngineKind::Scheduler,
                base_offset: volta_regs::PMU_BASE,
                has_firmware: true,
                firmware_state: FirmwareState::NotLoaded,
            },
            EngineInfo {
                name: "PDISP (Display)".into(),
                kind: EngineKind::Display,
                base_offset: volta_regs::PDISP_BASE,
                has_firmware: false,
                firmware_state: FirmwareState::NotPresent,
            },
            EngineInfo {
                name: "NVDEC (Video Decode)".into(),
                kind: EngineKind::Video,
                base_offset: volta_regs::NVDEC_BASE,
                has_firmware: true,
                firmware_state: FirmwareState::NotLoaded,
            },
            EngineInfo {
                name: "NVENC (Video Encode)".into(),
                kind: EngineKind::Video,
                base_offset: volta_regs::NVENC_BASE,
                has_firmware: true,
                firmware_state: FirmwareState::NotLoaded,
            },
        ];

        Self {
            identity,
            power_domains,
            memory_regions,
            engines,
        }
    }
}

impl GpuMetal for NvVoltaMetal {
    fn identity(&self) -> &dyn GpuIdentity {
        &self.identity
    }

    fn power_domains(&self) -> &[PowerDomain] {
        &self.power_domains
    }

    fn memory_regions(&self) -> &[MetalMemoryRegion] {
        &self.memory_regions
    }

    fn engine_list(&self) -> &[EngineInfo] {
        &self.engines
    }

    fn register_domain(&self, name: &str) -> Option<(usize, usize)> {
        match name {
            "PMC" => Some((0x000000, 0x001000)),
            "PBUS" => Some((0x001000, 0x002000)),
            "PFIFO" => Some((0x002000, 0x004000)),
            "PTIMER" => Some((volta_regs::PTIMER_BASE, volta_regs::PTIMER_BASE + 0x1000)),
            "PBDMA" => Some((0x040000, 0x080000)),
            "PCCSR" => Some((0x800000, 0x810000)),
            "USERMODE" => Some((0x810000, 0x820000)),
            "PFB" => Some((volta_regs::PFB_BASE, volta_regs::PFB_BASE + 0x2000)),
            "FBPA" => Some((volta_regs::FBPA0_BASE, volta_regs::FBPA0_BASE + 0x10000)),
            "LTC" => Some((volta_regs::LTC_BASE, volta_regs::LTC_BASE + 0x10000)),
            "PMU" => Some((volta_regs::PMU_BASE, volta_regs::PMU_BASE + 0x1000)),
            "GR" => Some((volta_regs::GR_BASE, volta_regs::GR_BASE + 0x20000)),
            "CE" => Some((volta_regs::CE_BASE, volta_regs::CE_BASE + 0x1000)),
            "PCLOCK" => Some((volta_regs::PCLOCK_BASE, volta_regs::PCLOCK_BASE + 0x1000)),
            "CLK" => Some((volta_regs::CLK_BASE, volta_regs::CLK_BASE + 0x1000)),
            "THERMAL" => Some((volta_regs::THERMAL_BASE, volta_regs::THERMAL_BASE + 0x100)),
            "FUSE" => Some((volta_regs::FUSE_BASE, volta_regs::FUSE_BASE + 0x1000)),
            "PRAMIN" => Some((volta_regs::PRAMIN_BASE, volta_regs::PRAMIN_BASE + 0x100000)),
            "DISP" => Some((volta_regs::PDISP_BASE, volta_regs::PDISP_BASE + 0x10000)),
            _ => None,
        }
    }

    fn domain_hints(&self) -> Vec<DomainHint> {
        vec![
            DomainHint {
                start: 0x000000,
                end: 0x001000,
                name: "PMC",
            },
            DomainHint {
                start: 0x001000,
                end: 0x002000,
                name: "PBUS",
            },
            DomainHint {
                start: 0x002000,
                end: 0x004000,
                name: "PFIFO",
            },
            DomainHint {
                start: 0x009000,
                end: 0x00A000,
                name: "PTIMER",
            },
            DomainHint {
                start: 0x020000,
                end: 0x021000,
                name: "PTOP",
            },
            DomainHint {
                start: 0x021000,
                end: 0x022000,
                name: "FUSE",
            },
            DomainHint {
                start: 0x022000,
                end: 0x023000,
                name: "PTOP/ENGINE",
            },
            DomainHint {
                start: 0x040000,
                end: 0x080000,
                name: "PBDMA",
            },
            DomainHint {
                start: 0x084000,
                end: 0x085000,
                name: "NVDEC",
            },
            DomainHint {
                start: 0x100000,
                end: 0x102000,
                name: "PFB",
            },
            DomainHint {
                start: 0x104000,
                end: 0x105000,
                name: "CE",
            },
            DomainHint {
                start: 0x10A000,
                end: 0x10B000,
                name: "PMU",
            },
            DomainHint {
                start: 0x122000,
                end: 0x123000,
                name: "PRI_MASTER",
            },
            DomainHint {
                start: 0x132000,
                end: 0x133000,
                name: "CLK",
            },
            DomainHint {
                start: 0x137000,
                end: 0x138000,
                name: "PCLOCK",
            },
            DomainHint {
                start: 0x17E000,
                end: 0x190000,
                name: "LTC",
            },
            DomainHint {
                start: 0x1C8000,
                end: 0x1C9000,
                name: "NVENC",
            },
            DomainHint {
                start: 0x1FA000,
                end: 0x1FB000,
                name: "PMEM",
            },
            DomainHint {
                start: 0x400000,
                end: 0x420000,
                name: "GR",
            },
            DomainHint {
                start: 0x610000,
                end: 0x620000,
                name: "DISP",
            },
            DomainHint {
                start: 0x700000,
                end: 0x800000,
                name: "PRAMIN",
            },
            DomainHint {
                start: 0x800000,
                end: 0x810000,
                name: "PCCSR",
            },
            DomainHint {
                start: 0x810000,
                end: 0x820000,
                name: "USERMODE",
            },
            DomainHint {
                start: 0x9A0000,
                end: 0x9B0000,
                name: "FBPA",
            },
        ]
    }

    fn warmup_sequence(&self) -> Vec<WarmupStep> {
        vec![
            WarmupStep {
                description: "PMC_ENABLE: un-gate all engine clock domains".into(),
                writes: vec![RegisterWrite {
                    offset: volta_regs::PMC_ENABLE,
                    value: 0xFFFF_FFFF,
                    mask: None,
                }],
                delay_ms: 50,
                verify: vec![RegisterVerify {
                    offset: volta_regs::PMC_ENABLE,
                    expected: 1 << 8,
                    mask: 1 << 8,
                }],
            },
            WarmupStep {
                description: "PFIFO reset cycle: toggle PMC bit 8".into(),
                writes: vec![RegisterWrite {
                    offset: volta_regs::PMC_ENABLE,
                    value: 0,
                    mask: Some(!(1u32 << 8)),
                }],
                delay_ms: 20,
                verify: vec![],
            },
            WarmupStep {
                description: "PFIFO re-enable: set PMC bit 8 (preserve all other domains)".into(),
                writes: vec![RegisterWrite {
                    offset: volta_regs::PMC_ENABLE,
                    value: 1 << 8,
                    mask: Some(0xFFFF_FFFF),
                }],
                delay_ms: 50,
                verify: vec![RegisterVerify {
                    offset: volta_regs::PBDMA_MAP,
                    expected: 0,
                    mask: 0,
                }],
            },
        ]
    }

    fn boot0_offset(&self) -> usize {
        volta_regs::BOOT0
    }

    fn pmc_enable_offset(&self) -> usize {
        volta_regs::PMC_ENABLE
    }

    fn pbdma_map_offset(&self) -> Option<usize> {
        Some(volta_regs::PBDMA_MAP)
    }

    fn pramin_base_offset(&self) -> Option<usize> {
        Some(volta_regs::PRAMIN_BASE)
    }

    fn bar2_block_offset(&self) -> Option<usize> {
        Some(volta_regs::BAR2_BLOCK)
    }
}

/// Live hardware probe results from BAR0 reads.
#[derive(Debug, Clone)]
pub struct NvVoltaProbe {
    /// Current PMC_ENABLE register value.
    pub pmc_enable: u32,
    /// Per-domain active/gated state probed from PMC_ENABLE.
    pub domain_states: Vec<(String, bool)>,
    /// FALCON microcontroller states: (name, base, ctrl, halted).
    pub falcon_states: Vec<(String, usize, u32, bool)>,
    /// Temperature in celsius (if readable).
    pub temperature_c: Option<u32>,
    /// Fuse configuration registers.
    pub fuse_config: Vec<(String, u32)>,
    /// Number of active GPC partitions (from fuses).
    pub active_gpcs: u32,
    /// Number of active TPC partitions (from fuses).
    pub active_tpcs: u32,
    /// Number of active FBP partitions (from fuses).
    pub active_fbps: u32,
    /// FBPA partition liveness.
    pub fbpa_alive: Vec<(u32, bool)>,
    /// LTC partition liveness.
    pub ltc_alive: Vec<(u32, bool)>,
}

impl NvVoltaProbe {
    /// Print human-readable summary.
    pub fn print_summary(&self) {
        eprintln!("╠══ LIVE HARDWARE PROBE ═════════════════════════════════════╣");
        eprintln!("║ PMC_ENABLE = {:#010x}", self.pmc_enable);
        for (name, active) in &self.domain_states {
            eprintln!(
                "║   {name:<8} → {}",
                if *active { "ACTIVE" } else { "gated" }
            );
        }
        if let Some(t) = self.temperature_c {
            eprintln!("║ Temperature: ~{}°C", t);
        }
        eprintln!(
            "║ Active: {} GPCs, {} TPCs, {} FBPs",
            self.active_gpcs, self.active_tpcs, self.active_fbps
        );
        for (idx, alive) in &self.fbpa_alive {
            eprintln!("║   FBPA{idx}: {}", if *alive { "alive" } else { "dead" });
        }
        for (idx, alive) in &self.ltc_alive {
            eprintln!("║   LTC{idx}: {}", if *alive { "alive" } else { "dead" });
        }
        for (name, base, ctrl, halted) in &self.falcon_states {
            eprintln!(
                "║   {name:6} @ {base:#08x}: CTRL={ctrl:#010x} {}",
                if *halted { "HALTED" } else { "running?" },
            );
        }
    }

    /// Export as JSON.
    pub fn to_json_value(&self) -> serde_json::Value {
        use serde_json::json;
        json!({
            "pmc_enable": format!("{:#010x}", self.pmc_enable),
            "domains": self.domain_states.iter().map(|(n, a)| json!({
                "name": n, "active": a,
            })).collect::<Vec<_>>(),
            "temperature_c": self.temperature_c,
            "active_gpcs": self.active_gpcs,
            "active_tpcs": self.active_tpcs,
            "active_fbps": self.active_fbps,
            "fbpa": self.fbpa_alive.iter().map(|(i, a)| json!({
                "index": i, "alive": a,
            })).collect::<Vec<_>>(),
            "ltc": self.ltc_alive.iter().map(|(i, a)| json!({
                "index": i, "alive": a,
            })).collect::<Vec<_>>(),
            "falcons": self.falcon_states.iter().map(|(n, b, c, h)| json!({
                "name": n, "base": format!("{b:#x}"), "ctrl": format!("{c:#010x}"), "halted": h,
            })).collect::<Vec<_>>(),
            "fuses": self.fuse_config.iter().map(|(n, v)| json!({
                "name": n, "value": format!("{v:#010x}"),
            })).collect::<Vec<_>>(),
        })
    }
}

impl NvVoltaMetal {
    /// Probe live hardware state from BAR0 reads.
    ///
    /// Unlike `from_boot0` which only decodes identity, this reads actual
    /// register values to determine power domain states, FALCON status,
    /// temperature, fuse configuration, and partition liveness.
    pub fn probe_live(&self, bar0: &MappedBar) -> NvVoltaProbe {
        let r = |off: usize| bar0.read_u32(off).unwrap_or(0xDEAD_DEAD);
        let is_err = |v: u32| {
            v == 0xFFFF_FFFF || v == 0xDEAD_DEAD || (v >> 16) == 0xBADF || (v >> 16) == 0xBAD0
        };

        let pmc_enable = r(volta_regs::PMC_ENABLE);

        let domain_states: Vec<(String, bool)> = self
            .power_domains
            .iter()
            .map(|d| {
                let active = d.enable_bit.is_some_and(|bit| pmc_enable & bit != 0);
                (d.name.clone(), active)
            })
            .collect();

        // FALCON states
        let falcon_defs: &[(&str, usize)] = &[
            ("PMU", volta_regs::PMU_BASE),
            ("GR", 0x409000),
            ("CE0", volta_regs::CE_BASE),
            ("NVDEC", volta_regs::NVDEC_BASE),
            ("NVENC", volta_regs::NVENC_BASE),
        ];
        let falcon_states: Vec<(String, usize, u32, bool)> = falcon_defs
            .iter()
            .filter_map(|&(name, base)| {
                let ctrl = r(base + 0x100);
                if is_err(ctrl) {
                    return None;
                }
                let halted = ctrl & 0x10 != 0;
                Some((name.to_string(), base, ctrl, halted))
            })
            .collect();

        // Temperature: Volta uses NV_THERM_I2C_SENSOR_DATA at 0x20460
        // Format: bits [23:8] are temperature in 8.8 fixed point
        let temp_raw = r(volta_regs::THERMAL_BASE + 0x60);
        let temperature_c = if !is_err(temp_raw) && temp_raw != 0 {
            Some((temp_raw >> 8) & 0x1FF)
        } else {
            // Fallback: try 0x20008 (NV_THERM_TSENSE_U2_A_0_TEMPERATURE)
            let t2 = r(0x20008);
            if !is_err(t2) && t2 != 0 {
                Some(t2 & 0x3FF)
            } else {
                None
            }
        };

        // Fuse configuration
        let fuse_defs: &[(&str, usize)] = &[
            ("OPT_GPU_DISABLE", 0x21C04),
            ("OPT_GPC_DISABLE", 0x21C08),
            ("OPT_TPC_DISABLE", 0x21C0C),
            ("OPT_FBP_DISABLE", 0x21C14),
            ("OPT_PES_DISABLE", 0x21C18),
        ];
        let fuse_config: Vec<(String, u32)> = fuse_defs
            .iter()
            .filter_map(|&(name, off)| {
                let val = r(off);
                if is_err(val) {
                    return None;
                }
                Some((name.to_string(), val))
            })
            .collect();

        // Derive active counts from fuses
        let gpc_disable = fuse_config
            .iter()
            .find(|(n, _)| n == "OPT_GPC_DISABLE")
            .map(|(_, v)| *v)
            .unwrap_or(0);
        let tpc_disable = fuse_config
            .iter()
            .find(|(n, _)| n == "OPT_TPC_DISABLE")
            .map(|(_, v)| *v)
            .unwrap_or(0);
        let fbp_disable = fuse_config
            .iter()
            .find(|(n, _)| n == "OPT_FBP_DISABLE")
            .map(|(_, v)| *v)
            .unwrap_or(0);
        let active_gpcs = 6 - gpc_disable.count_ones();
        let active_tpcs = 84 - tpc_disable.count_ones();
        let active_fbps = 4 - (fbp_disable & 0xF).count_ones();

        // FBPA partition liveness
        let fbpa_alive: Vec<(u32, bool)> = (0..4)
            .map(|i| {
                let base = volta_regs::FBPA0_BASE + (i as usize) * volta_regs::FBPA_STRIDE;
                let v = r(base);
                (i, !is_err(v))
            })
            .collect();

        // LTC partition liveness
        let ltc_alive: Vec<(u32, bool)> = (0..6)
            .map(|i| {
                let base = volta_regs::LTC_BASE + (i as usize) * volta_regs::LTC_STRIDE;
                let v = r(base);
                (i, !is_err(v))
            })
            .collect();

        NvVoltaProbe {
            pmc_enable,
            domain_states,
            falcon_states,
            temperature_c,
            fuse_config,
            active_gpcs,
            active_tpcs,
            active_fbps,
            fbpa_alive,
            ltc_alive,
        }
    }
}

/// Detect which `GpuMetal` implementation to use from a BOOT0 value.
///
/// Currently only supports NVIDIA Volta. Future: add Turing, Ampere,
/// AMD Vega, etc.
pub fn detect_gpu_metal(vendor: GpuVendor, boot0: u32) -> Option<Box<dyn GpuMetal>> {
    match vendor {
        GpuVendor::Nvidia => {
            let arch_code = ((boot0 >> 20) & 0x1FF) as u16;
            match arch_code {
                0x140..=0x14F => Some(Box::new(NvVoltaMetal::from_boot0(boot0))),
                // Future: Turing, Ampere, Ada...
                _ => Some(Box::new(NvVoltaMetal::from_boot0(boot0))),
            }
        }
        GpuVendor::Amd => {
            // Stub — will be implemented when MI50 arrives
            None
        }
        _ => None,
    }
}
