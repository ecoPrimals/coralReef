// SPDX-License-Identifier: AGPL-3.0-only
//! Vendor-agnostic PCI device discovery and power management.
//!
//! Parses PCI configuration space via sysfs to enumerate BARs, capabilities,
//! power states, and link info for any PCI device. This layer is completely
//! vendor-agnostic — it works on NVIDIA, AMD, Intel, or any PCI device.
//!
//! Key operations:
//! - Config space parsing (vendor, device, class, BARs, capabilities)
//! - Power Management capability discovery and state transitions
//! - PCIe link information
//! - D3cold power cycling (PCI remove/rescan)

use std::collections::HashSet;
use std::fmt;

// ── PCI Power States ────────────────────────────────────────────────────

/// PCI PM power state (from PCI Power Management spec).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PciPmState {
    /// Fully operational.
    D0,
    /// Low-power state (optional, rarely used by GPUs).
    D1,
    /// Lower-power state (optional, rarely used by GPUs).
    D2,
    /// PCIe sleep — BARs disabled, config space partially accessible.
    D3Hot,
    /// Full power off — requires PCI bus rescan to recover.
    D3Cold,
    /// State could not be determined.
    Unknown(u8),
}

impl PciPmState {
    fn from_pmcsr_bits(bits: u8) -> Self {
        match bits & 0x03 {
            0 => Self::D0,
            1 => Self::D1,
            2 => Self::D2,
            3 => Self::D3Hot,
            _ => Self::Unknown(bits),
        }
    }

    /// PMCSR bit encoding for this state.
    pub fn pmcsr_bits(self) -> u8 {
        match self {
            Self::D0 => 0,
            Self::D1 => 1,
            Self::D2 => 2,
            Self::D3Hot => 3,
            Self::D3Cold | Self::Unknown(_) => 3,
        }
    }
}

impl fmt::Display for PciPmState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::D0 => write!(f, "D0"),
            Self::D1 => write!(f, "D1"),
            Self::D2 => write!(f, "D2"),
            Self::D3Hot => write!(f, "D3hot"),
            Self::D3Cold => write!(f, "D3cold"),
            Self::Unknown(v) => write!(f, "Unknown({v:#x})"),
        }
    }
}

// ── GPU Vendor ──────────────────────────────────────────────────────────

/// GPU vendor identified from PCI vendor ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuVendor {
    /// NVIDIA Corporation (0x10DE).
    Nvidia,
    /// Advanced Micro Devices (0x1002).
    Amd,
    /// Intel Corporation (0x8086).
    Intel,
    /// Unknown or non-GPU vendor.
    Unknown(u16),
}

impl GpuVendor {
    /// Identify vendor from PCI vendor ID.
    pub fn from_vendor_id(id: u16) -> Self {
        match id {
            0x10DE => Self::Nvidia,
            0x1002 => Self::Amd,
            0x8086 => Self::Intel,
            other => Self::Unknown(other),
        }
    }
}

impl fmt::Display for GpuVendor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nvidia => write!(f, "NVIDIA"),
            Self::Amd => write!(f, "AMD"),
            Self::Intel => write!(f, "Intel"),
            Self::Unknown(id) => write!(f, "Unknown({id:#06x})"),
        }
    }
}

// ── PCI BAR ─────────────────────────────────────────────────────────────

/// A PCI Base Address Register (BAR).
#[derive(Debug, Clone)]
pub struct PciBar {
    /// BAR index (0–5).
    pub index: u8,
    /// Physical base address.
    pub base: u64,
    /// Region size in bytes.
    pub size: u64,
    /// MMIO (memory-mapped) vs I/O port.
    pub is_mmio: bool,
    /// 64-bit BAR (consumes two BAR slots).
    pub is_64bit: bool,
    /// Prefetchable memory.
    pub is_prefetchable: bool,
}

// ── PCI Capability ──────────────────────────────────────────────────────

/// A PCI capability from the capability chain.
#[derive(Debug, Clone)]
pub struct PciCapability {
    /// Capability ID (0x01 = PM, 0x05 = MSI, 0x10 = PCIe, 0x11 = MSI-X).
    pub id: u8,
    /// Config space offset of this capability.
    pub offset: u8,
    /// Human-readable name.
    pub name: &'static str,
}

impl PciCapability {
    fn name_for_id(id: u8) -> &'static str {
        match id {
            0x01 => "Power Management",
            0x02 => "AGP",
            0x03 => "VPD",
            0x04 => "Slot ID",
            0x05 => "MSI",
            0x06 => "CompactPCI Hot Swap",
            0x07 => "PCI-X",
            0x08 => "HyperTransport",
            0x09 => "Vendor Specific",
            0x0A => "Debug Port",
            0x0B => "CompactPCI CRC",
            0x0C => "PCI Hot-Plug",
            0x0D => "PCI Bridge Subsystem VID",
            0x0E => "AGP 8x",
            0x0F => "Secure Device",
            0x10 => "PCI Express",
            0x11 => "MSI-X",
            0x12 => "SATA",
            0x13 => "Advanced Features",
            0x14 => "Enhanced Allocation",
            0x15 => "Flattening Portal Bridge",
            _ => "Unknown",
        }
    }
}

// ── PCI Power Management Info ───────────────────────────────────────────

/// Power management capability details.
#[derive(Debug, Clone)]
pub struct PciPowerInfo {
    /// Config space offset of the PM capability (None if not present).
    pub pm_cap_offset: Option<u8>,
    /// Current power state.
    pub current_state: PciPmState,
    /// D1 power state supported.
    pub d1_support: bool,
    /// D2 power state supported.
    pub d2_support: bool,
    /// PME (Power Management Event) support mask.
    pub pme_support: u8,
    /// Raw PMCSR register value.
    pub pmcsr_raw: u16,
}

// ── PCIe Link Info ──────────────────────────────────────────────────────

/// PCIe link status and capabilities.
#[derive(Debug, Clone)]
pub struct PcieLinkInfo {
    /// Maximum link speed (e.g., "8.0 GT/s" for Gen3).
    pub max_speed: PcieLinkSpeed,
    /// Current negotiated speed.
    pub current_speed: PcieLinkSpeed,
    /// Maximum link width (e.g., x16).
    pub max_width: u8,
    /// Current negotiated width.
    pub current_width: u8,
}

/// PCIe link speed encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PcieLinkSpeed {
    /// 2.5 GT/s (Gen1).
    Gen1,
    /// 5.0 GT/s (Gen2).
    Gen2,
    /// 8.0 GT/s (Gen3).
    Gen3,
    /// 16.0 GT/s (Gen4).
    Gen4,
    /// 32.0 GT/s (Gen5).
    Gen5,
    /// Unknown speed encoding.
    Unknown(u8),
}

impl PcieLinkSpeed {
    fn from_encoding(val: u8) -> Self {
        match val & 0x0F {
            1 => Self::Gen1,
            2 => Self::Gen2,
            3 => Self::Gen3,
            4 => Self::Gen4,
            5 => Self::Gen5,
            other => Self::Unknown(other),
        }
    }
}

impl fmt::Display for PcieLinkSpeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gen1 => write!(f, "2.5 GT/s (Gen1)"),
            Self::Gen2 => write!(f, "5.0 GT/s (Gen2)"),
            Self::Gen3 => write!(f, "8.0 GT/s (Gen3)"),
            Self::Gen4 => write!(f, "16.0 GT/s (Gen4)"),
            Self::Gen5 => write!(f, "32.0 GT/s (Gen5)"),
            Self::Unknown(v) => write!(f, "Unknown({v:#x})"),
        }
    }
}

// ── PCI Device Info (top-level) ─────────────────────────────────────────

/// Complete PCI device information parsed from sysfs config space.
#[derive(Debug, Clone)]
pub struct PciDeviceInfo {
    /// PCI Bus:Device.Function address (e.g., "0000:4a:00.0").
    pub bdf: String,
    /// PCI vendor ID.
    pub vendor_id: u16,
    /// PCI device ID.
    pub device_id: u16,
    /// PCI class code (24-bit: class << 16 | subclass << 8 | prog_if).
    pub class_code: u32,
    /// Subsystem vendor and device IDs.
    pub subsystem: (u16, u16),
    /// Base Address Registers.
    pub bars: Vec<PciBar>,
    /// PCI capabilities from the capability chain.
    pub capabilities: Vec<PciCapability>,
    /// Power management info.
    pub power: PciPowerInfo,
    /// PCIe link info (if PCIe capability present).
    pub pcie_link: Option<PcieLinkInfo>,
    /// GPU vendor classification.
    pub vendor: GpuVendor,
}

impl PciDeviceInfo {
    /// Parse PCI device info from sysfs config space.
    pub fn from_sysfs(bdf: &str) -> Result<Self, String> {
        let config_path = format!("/sys/bus/pci/devices/{bdf}/config");
        let config = std::fs::read(&config_path)
            .map_err(|e| format!("read {config_path}: {e}"))?;

        if config.len() < 64 {
            return Err(format!("PCI config too short: {} bytes", config.len()));
        }

        let r16 = |off: usize| u16::from_le_bytes([config[off], config[off + 1]]);
        let r32 = |off: usize| {
            u32::from_le_bytes([config[off], config[off + 1], config[off + 2], config[off + 3]])
        };

        let vendor_id = r16(0x00);
        let device_id = r16(0x02);
        let class_code = r32(0x08) >> 8;
        let subsystem = (r16(0x2C), r16(0x2E));

        // Parse BARs from sysfs resource file (more reliable than config space)
        let bars = Self::parse_bars_sysfs(bdf);

        // Walk capability chain.
        // vfio-pci may only expose 64 bytes of config space via sysfs,
        // so the capability pointer (0x34) may reference offsets beyond
        // what we have. We try the full config first, then fall back to
        // inferring common capabilities from the vendor ID.
        let status = r16(0x06);
        let mut capabilities = Vec::new();
        let mut pm_cap_offset = None;
        let mut pcie_cap_offset = None;

        let has_cap_list = status & 0x10 != 0;
        if has_cap_list && config.len() >= 0x40 {
            let mut cap_ptr = (config[0x34] & 0xFC) as usize;
            let mut visited = HashSet::new();
            while cap_ptr != 0 && !visited.contains(&cap_ptr) && cap_ptr + 2 <= config.len() {
                visited.insert(cap_ptr);
                let cap_id = config[cap_ptr];
                let name = PciCapability::name_for_id(cap_id);

                capabilities.push(PciCapability {
                    id: cap_id,
                    offset: cap_ptr as u8,
                    name,
                });

                if cap_id == 0x01 {
                    pm_cap_offset = Some(cap_ptr as u8);
                }
                if cap_id == 0x10 {
                    pcie_cap_offset = Some(cap_ptr as u8);
                }

                cap_ptr = (config[cap_ptr + 1] & 0xFC) as usize;
            }
        }

        // If vfio-pci truncated config space, populate from sysfs attributes
        if capabilities.is_empty() && has_cap_list {
            let dev_path = format!("/sys/bus/pci/devices/{bdf}");
            // PCIe link info from sysfs as evidence of PCIe capability
            if std::path::Path::new(&format!("{dev_path}/current_link_speed")).exists() {
                capabilities.push(PciCapability {
                    id: 0x10,
                    offset: 0,
                    name: "PCI Express (from sysfs)",
                });
            }
            // Power state from sysfs as evidence of PM capability
            if std::path::Path::new(&format!("{dev_path}/power_state")).exists() {
                capabilities.push(PciCapability {
                    id: 0x01,
                    offset: 0,
                    name: "Power Management (from sysfs)",
                });
            }
        }

        // Parse PM capability — try config space first, then sysfs fallback
        let power = if let Some(pm_off) = pm_cap_offset {
            let pm_off = pm_off as usize;
            if pm_off + 6 <= config.len() {
                let pmc = r16(pm_off + 2);
                let pmcsr = r16(pm_off + 4);
                PciPowerInfo {
                    pm_cap_offset: Some(pm_off as u8),
                    current_state: PciPmState::from_pmcsr_bits((pmcsr & 0x03) as u8),
                    d1_support: pmc & (1 << 9) != 0,
                    d2_support: pmc & (1 << 10) != 0,
                    pme_support: ((pmc >> 11) & 0x1F) as u8,
                    pmcsr_raw: pmcsr,
                }
            } else {
                PciPowerInfo {
                    pm_cap_offset: Some(pm_off as u8),
                    current_state: PciPmState::Unknown(0xFF),
                    d1_support: false,
                    d2_support: false,
                    pme_support: 0,
                    pmcsr_raw: 0,
                }
            }
        } else {
            // sysfs fallback: read power_state if config space was truncated
            let dev_path = format!("/sys/bus/pci/devices/{bdf}");
            let sysfs_state = std::fs::read_to_string(format!("{dev_path}/power_state"))
                .ok()
                .and_then(|s| match s.trim() {
                    "D0" => Some(PciPmState::D0),
                    "D1" => Some(PciPmState::D1),
                    "D2" => Some(PciPmState::D2),
                    "D3hot" => Some(PciPmState::D3Hot),
                    "D3cold" => Some(PciPmState::D3Cold),
                    _ => None,
                });
            PciPowerInfo {
                pm_cap_offset: None,
                current_state: sysfs_state.unwrap_or(PciPmState::Unknown(0xFF)),
                d1_support: false,
                d2_support: false,
                pme_support: 0,
                pmcsr_raw: 0,
            }
        };

        // Parse PCIe link info — config space or sysfs fallback
        let pcie_link = pcie_cap_offset
            .and_then(|off| {
                let off = off as usize;
                if off + 0x14 <= config.len() {
                    let link_cap = r32(off + 0x0C);
                    let link_sta = r16(off + 0x12);
                    Some(PcieLinkInfo {
                        max_speed: PcieLinkSpeed::from_encoding((link_cap & 0x0F) as u8),
                        current_speed: PcieLinkSpeed::from_encoding((link_sta & 0x0F) as u8),
                        max_width: ((link_cap >> 4) & 0x3F) as u8,
                        current_width: ((link_sta >> 4) & 0x3F) as u8,
                    })
                } else {
                    None
                }
            })
            .or_else(|| Self::parse_link_sysfs(bdf));

        Ok(Self {
            bdf: bdf.to_string(),
            vendor_id,
            device_id,
            class_code,
            subsystem,
            bars,
            capabilities,
            power,
            pcie_link,
            vendor: GpuVendor::from_vendor_id(vendor_id),
        })
    }

    fn parse_link_sysfs(bdf: &str) -> Option<PcieLinkInfo> {
        let dev = format!("/sys/bus/pci/devices/{bdf}");
        let parse_speed = |s: &str| -> PcieLinkSpeed {
            if s.contains("32") || s.contains("Gen5") {
                PcieLinkSpeed::Gen5
            } else if s.contains("16") || s.contains("Gen4") {
                PcieLinkSpeed::Gen4
            } else if s.contains("8") || s.contains("Gen3") {
                PcieLinkSpeed::Gen3
            } else if s.contains("5") || s.contains("Gen2") {
                PcieLinkSpeed::Gen2
            } else if s.contains("2.5") || s.contains("Gen1") {
                PcieLinkSpeed::Gen1
            } else {
                PcieLinkSpeed::Unknown(0)
            }
        };
        let parse_width = |s: &str| -> u8 {
            s.trim().trim_start_matches('x').parse().unwrap_or(0)
        };
        let cur_speed = std::fs::read_to_string(format!("{dev}/current_link_speed")).ok()?;
        let max_speed = std::fs::read_to_string(format!("{dev}/max_link_speed")).unwrap_or_default();
        let cur_width = std::fs::read_to_string(format!("{dev}/current_link_width")).unwrap_or_default();
        let max_width = std::fs::read_to_string(format!("{dev}/max_link_width")).unwrap_or_default();
        Some(PcieLinkInfo {
            current_speed: parse_speed(&cur_speed),
            max_speed: parse_speed(&max_speed),
            current_width: parse_width(&cur_width),
            max_width: parse_width(&max_width),
        })
    }

    fn parse_bars_sysfs(bdf: &str) -> Vec<PciBar> {
        let resource_path = format!("/sys/bus/pci/devices/{bdf}/resource");
        let content = match std::fs::read_to_string(&resource_path) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };

        let mut bars = Vec::new();
        for (index, line) in content.lines().enumerate() {
            if index > 5 {
                break;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 3 {
                continue;
            }
            let start = u64::from_str_radix(parts[0].trim_start_matches("0x"), 16).unwrap_or(0);
            let end = u64::from_str_radix(parts[1].trim_start_matches("0x"), 16).unwrap_or(0);
            let flags = u64::from_str_radix(parts[2].trim_start_matches("0x"), 16).unwrap_or(0);

            if start == 0 && end == 0 {
                continue;
            }

            let size = if end > start { end - start + 1 } else { 0 };
            let is_mmio = flags & 0x01 == 0;
            let is_64bit = flags & 0x04 != 0;
            let is_prefetchable = flags & 0x08 != 0;

            bars.push(PciBar {
                index: index as u8,
                base: start,
                size,
                is_mmio,
                is_64bit,
                is_prefetchable,
            });
        }

        bars
    }

    /// Print a human-readable summary of the device.
    pub fn print_summary(&self) {
        eprintln!("╠══ PCI DEVICE INFO ═════════════════════════════════════════╣");
        eprintln!("║ BDF:     {}", self.bdf);
        eprintln!(
            "║ ID:      {:04x}:{:04x} ({})",
            self.vendor_id, self.device_id, self.vendor
        );
        eprintln!("║ Class:   {:#08x}", self.class_code);
        eprintln!("║ Power:   {} (PMCSR={:#06x})", self.power.current_state, self.power.pmcsr_raw);
        for bar in &self.bars {
            eprintln!(
                "║ BAR{}:    {:#014x} ({} KB) {}{}{}",
                bar.index,
                bar.base,
                bar.size / 1024,
                if bar.is_mmio { "MMIO" } else { "IO" },
                if bar.is_64bit { " 64bit" } else { "" },
                if bar.is_prefetchable { " prefetch" } else { "" },
            );
        }
        for cap in &self.capabilities {
            eprintln!("║ Cap:     [{:#04x}] {} @ {:#04x}", cap.id, cap.name, cap.offset);
        }
        if let Some(ref link) = self.pcie_link {
            eprintln!(
                "║ PCIe:    x{} @ {} (max x{} @ {})",
                link.current_width, link.current_speed,
                link.max_width, link.max_speed,
            );
        }
    }
}

// ── PCI Power Management Operations ─────────────────────────────────────

/// Force a PCI device from D3hot back to D0 by writing to the PM capability.
///
/// When `vfio-pci` binds, it unconditionally transitions the GPU to D3hot.
/// BAR0 reads return 0xFFFFFFFF in D3hot, making VRAM inaccessible.
/// However, HBM2 training is NOT lost — the data is still in the memory
/// controller's registers. Writing D0 to the PCI PMCSR restores BAR0
/// access and VRAM is immediately alive again.
///
/// This is vendor-agnostic — works for any PCI device with PM capability.
pub fn force_pci_d0(bdf: &str) -> Result<(), String> {
    let config_path = format!("/sys/bus/pci/devices/{bdf}/config");
    let config = std::fs::read(&config_path)
        .map_err(|e| format!("read PCI config: {e}"))?;

    if config.len() < 0x40 {
        return Err("PCI config too short".into());
    }

    let status = u16::from_le_bytes([config[0x06], config[0x07]]);
    if status & 0x10 == 0 {
        return Err("No PCI capabilities list".into());
    }

    let mut cap_ptr = (config[0x34] & 0xFC) as usize;
    let mut pm_offset = None;
    let mut visited = HashSet::new();
    while cap_ptr != 0 && !visited.contains(&cap_ptr) && cap_ptr + 2 <= config.len() {
        visited.insert(cap_ptr);
        if config[cap_ptr] == 0x01 {
            pm_offset = Some(cap_ptr);
            break;
        }
        cap_ptr = (config[cap_ptr + 1] & 0xFC) as usize;
    }

    let pm_off = pm_offset.ok_or("PM capability not found")?;
    let pmcsr_off = pm_off + 4;

    if pmcsr_off + 2 > config.len() {
        return Err("PMCSR offset beyond config".into());
    }

    let pmcsr = u16::from_le_bytes([config[pmcsr_off], config[pmcsr_off + 1]]);
    let current_state = pmcsr & 0x03;

    if current_state == 0 {
        return Ok(());
    }

    let pm_states = ["D0", "D1", "D2", "D3hot"];
    eprintln!(
        "  PCI PM: {} → D0 (PMCSR {pmcsr:#06x} → {:#06x}) at config+{pmcsr_off:#04x}",
        pm_states[current_state as usize],
        pmcsr & !0x03
    );

    let new_pmcsr = (pmcsr & !0x03).to_le_bytes();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(&config_path)
        .map_err(|e| format!("open config for write: {e}"))?;

    use std::io::{Seek, Write};
    file.seek(std::io::SeekFrom::Start(pmcsr_off as u64))
        .map_err(|e| format!("seek: {e}"))?;
    file.write_all(&new_pmcsr)
        .map_err(|e| format!("write PMCSR: {e}"))?;

    // PCI spec requires 10ms after D3hot → D0 transition
    std::thread::sleep(std::time::Duration::from_millis(20));

    Ok(())
}

/// Transition a PCI device to a specific power state.
///
/// Writes the target state to PMCSR bits [1:0]. Observe PCI spec recovery
/// delays: D3hot→D0 requires 10ms, D2→D0 requires 200µs, etc.
pub fn set_pci_power_state(bdf: &str, target: PciPmState) -> Result<PciPmState, String> {
    let config_path = format!("/sys/bus/pci/devices/{bdf}/config");
    let config = std::fs::read(&config_path)
        .map_err(|e| format!("read PCI config: {e}"))?;

    if config.len() < 0x40 {
        return Err("PCI config too short".into());
    }

    let status = u16::from_le_bytes([config[0x06], config[0x07]]);
    if status & 0x10 == 0 {
        return Err("No PCI capabilities list".into());
    }

    let mut cap_ptr = (config[0x34] & 0xFC) as usize;
    let mut pm_offset = None;
    let mut visited = HashSet::new();
    while cap_ptr != 0 && !visited.contains(&cap_ptr) && cap_ptr + 2 <= config.len() {
        visited.insert(cap_ptr);
        if config[cap_ptr] == 0x01 {
            pm_offset = Some(cap_ptr);
            break;
        }
        cap_ptr = (config[cap_ptr + 1] & 0xFC) as usize;
    }

    let pm_off = pm_offset.ok_or("PM capability not found")?;
    let pmcsr_off = pm_off + 4;
    if pmcsr_off + 2 > config.len() {
        return Err("PMCSR beyond config".into());
    }

    let old_pmcsr = u16::from_le_bytes([config[pmcsr_off], config[pmcsr_off + 1]]);
    let old_state = PciPmState::from_pmcsr_bits((old_pmcsr & 0x03) as u8);

    let new_bits = target.pmcsr_bits() as u16;
    let new_pmcsr = (old_pmcsr & !0x03) | new_bits;

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(&config_path)
        .map_err(|e| format!("open config: {e}"))?;

    use std::io::{Seek, Write};
    file.seek(std::io::SeekFrom::Start(pmcsr_off as u64))
        .map_err(|e| format!("seek: {e}"))?;
    file.write_all(&new_pmcsr.to_le_bytes())
        .map_err(|e| format!("write: {e}"))?;

    // Recovery delays per PCI PM spec
    let delay_ms = match (old_state, target) {
        (PciPmState::D3Hot, PciPmState::D0) => 20,
        (PciPmState::D2, PciPmState::D0) => 1,
        _ => 5,
    };
    std::thread::sleep(std::time::Duration::from_millis(delay_ms));

    Ok(old_state)
}

/// Trigger a PCI D3cold → D0 power cycle via sysfs.
///
/// Forces a full power-off/power-on cycle, which causes the boot ROM to
/// re-execute devinit (including HBM2 training). The device must NOT be
/// bound to any driver. Vendor-agnostic.
pub fn pci_power_cycle(bdf: &str) -> Result<bool, String> {
    let dev_path = format!("/sys/bus/pci/devices/{bdf}");

    let driver_link = format!("{dev_path}/driver");
    if std::fs::read_link(&driver_link).is_ok() {
        return Err("Device has a driver bound — unbind first".into());
    }

    let _ = std::fs::write(format!("{dev_path}/d3cold_allowed"), "1");
    let _ = std::fs::write(format!("{dev_path}/power/control"), "auto");

    std::fs::write(format!("{dev_path}/remove"), "1")
        .map_err(|e| format!("remove failed: {e}"))?;

    std::thread::sleep(std::time::Duration::from_secs(2));

    std::fs::write("/sys/bus/pci/rescan", "1")
        .map_err(|e| format!("rescan failed: {e}"))?;

    std::thread::sleep(std::time::Duration::from_secs(3));

    if !std::path::Path::new(&dev_path).exists() {
        return Err("Device not found after PCI rescan".into());
    }

    let _ = std::fs::write(format!("{dev_path}/d3cold_allowed"), "0");
    let _ = std::fs::write(format!("{dev_path}/power/control"), "on");

    Ok(true)
}

/// Snapshot a range of PCI config space registers.
///
/// Returns `(offset, value)` pairs for each 32-bit register in the range.
pub fn snapshot_config_space(bdf: &str, start: usize, end: usize) -> Result<Vec<(usize, u32)>, String> {
    let config_path = format!("/sys/bus/pci/devices/{bdf}/config");
    let config = std::fs::read(&config_path)
        .map_err(|e| format!("read config: {e}"))?;

    let mut regs = Vec::new();
    let end = end.min(config.len());
    for off in (start..end).step_by(4) {
        if off + 4 <= config.len() {
            let val = u32::from_le_bytes([config[off], config[off + 1], config[off + 2], config[off + 3]]);
            regs.push((off, val));
        }
    }
    Ok(regs)
}
