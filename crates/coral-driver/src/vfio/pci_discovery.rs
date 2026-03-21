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
use std::fmt::Write as FmtWrite;

use crate::linux_paths;

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

use crate::nv::identity::{PCI_VENDOR_AMD, PCI_VENDOR_INTEL, PCI_VENDOR_NVIDIA};

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
            PCI_VENDOR_NVIDIA => Self::Nvidia,
            PCI_VENDOR_AMD => Self::Amd,
            PCI_VENDOR_INTEL => Self::Intel,
            other => Self::Unknown(other),
        }
    }

    /// True if this vendor matches the given PCI vendor ID.
    #[must_use]
    pub fn matches_vendor_id(self, id: u16) -> bool {
        matches!(
            (self, id),
            (Self::Nvidia, PCI_VENDOR_NVIDIA)
                | (Self::Amd, PCI_VENDOR_AMD)
                | (Self::Intel, PCI_VENDOR_INTEL)
        )
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

/// PCI capability ID: Power Management.
pub const PCI_CAP_ID_PM: u8 = 0x01;
/// PCI capability ID: PCI Express.
pub const PCI_CAP_ID_PCIE: u8 = 0x10;
/// PCI status register: capability list present (bit 4).
pub const PCI_STATUS_CAP_LIST: u16 = 0x10;

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
            PCI_CAP_ID_PM => "Power Management",
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
            PCI_CAP_ID_PCIE => "PCI Express",
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

/// Parse a PCI Bus/Device/Function string (`DDDD:BB:DD.F`) from sysfs paths.
///
/// Returns `(domain, bus, device, function)` or `None` if the string is malformed.
#[must_use]
pub(crate) fn parse_pci_bdf(bdf: &str) -> Option<(u32, u8, u8, u8)> {
    let mut colon = bdf.split(':');
    let domain = u32::from_str_radix(colon.next()?, 16).ok()?;
    let bus = u8::from_str_radix(colon.next()?, 16).ok()?;
    let dev_func = colon.next()?;
    if colon.next().is_some() {
        return None;
    }
    let mut dot = dev_func.split('.');
    let dev = u8::from_str_radix(dot.next()?, 16).ok()?;
    let func = u8::from_str_radix(dot.next()?, 16).ok()?;
    if dot.next().is_some() {
        return None;
    }
    Some((domain, bus, dev, func))
}

/// PCI base class code (byte 2 of the 3-byte class tuple: class, subclass, prog-if).
#[must_use]
#[allow(dead_code, reason = "Reserved for upcoming sysfs topology wiring")]
pub(crate) fn pci_class_base(class_code_24: u32) -> u8 {
    ((class_code_24 >> 16) & 0xFF) as u8
}

/// Parse a hex ID from sysfs files such as `vendor`, `device` (`0x10de` or `10de`).
#[must_use]
pub(crate) fn parse_pci_sysfs_hex_id(contents: &str) -> Option<u16> {
    let s = contents.trim();
    let digits = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    u16::from_str_radix(digits, 16).ok()
}

/// Parse one line of `/sys/bus/pci/devices/.../resource` (start end flags).
#[must_use]
pub(crate) fn parse_pci_resource_line(line: &str, index: u8) -> Option<PciBar> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 {
        return None;
    }
    let start = u64::from_str_radix(parts[0].trim_start_matches("0x"), 16).unwrap_or(0);
    let end = u64::from_str_radix(parts[1].trim_start_matches("0x"), 16).unwrap_or(0);
    let flags = u64::from_str_radix(parts[2].trim_start_matches("0x"), 16).unwrap_or(0);

    if start == 0 && end == 0 {
        return None;
    }

    let size = if end > start { end - start + 1 } else { 0 };
    let is_mmio = flags & 0x01 == 0;
    let is_64bit = flags & 0x04 != 0;
    let is_prefetchable = flags & 0x08 != 0;

    Some(PciBar {
        index,
        base: start,
        size,
        is_mmio,
        is_64bit,
        is_prefetchable,
    })
}

/// Parse the full `resource` file (BAR0–BAR5) into [`PciBar`] entries.
#[must_use]
pub(crate) fn parse_pci_resource_file(content: &str) -> Vec<PciBar> {
    let mut bars = Vec::new();
    for (index, line) in content.lines().enumerate() {
        if index > 5 {
            break;
        }
        if let Some(bar) = parse_pci_resource_line(line, index as u8) {
            bars.push(bar);
        }
    }
    bars
}

/// Map sysfs `current_link_speed` / `max_link_speed` text to a [`PcieLinkSpeed`].
#[must_use]
pub(crate) fn parse_sysfs_pcie_speed(s: &str) -> PcieLinkSpeed {
    if s.contains("32") || s.contains("Gen5") {
        PcieLinkSpeed::Gen5
    } else if s.contains("16") || s.contains("Gen4") {
        PcieLinkSpeed::Gen4
    } else if s.contains('8') || s.contains("Gen3") {
        PcieLinkSpeed::Gen3
    } else if s.contains('5') || s.contains("Gen2") {
        PcieLinkSpeed::Gen2
    } else if s.contains("2.5") || s.contains("Gen1") {
        PcieLinkSpeed::Gen1
    } else {
        PcieLinkSpeed::Unknown(0)
    }
}

/// Parse `x16` / `16` style width strings from sysfs.
#[must_use]
pub(crate) fn parse_sysfs_pcie_width(s: &str) -> u8 {
    s.trim().trim_start_matches('x').parse().unwrap_or(0)
}

/// Parse `power_state` sysfs contents (`D0`, `D3hot`, ...).
#[must_use]
pub(crate) fn parse_sysfs_power_state(s: &str) -> Option<PciPmState> {
    match s.trim() {
        "D0" => Some(PciPmState::D0),
        "D1" => Some(PciPmState::D1),
        "D2" => Some(PciPmState::D2),
        "D3hot" => Some(PciPmState::D3Hot),
        "D3cold" => Some(PciPmState::D3Cold),
        _ => None,
    }
}

fn read_sysfs_pci_hex_id(bdf: &str, name: &str) -> Option<u16> {
    let path = linux_paths::sysfs_pci_device_file(bdf, name);
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| parse_pci_sysfs_hex_id(&s))
}

fn pci_config_read_u16(config: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([config[off], config[off + 1]])
}

fn pci_config_read_u32(config: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([
        config[off],
        config[off + 1],
        config[off + 2],
        config[off + 3],
    ])
}

/// Walk the PCI capability list in config space.
///
/// Returns discovered capabilities plus the first PM and PCIe capability offsets.
fn walk_pci_capability_chain(config: &[u8]) -> (Vec<PciCapability>, Option<u8>, Option<u8>) {
    let mut capabilities = Vec::new();
    let mut pm_cap_offset = None;
    let mut pcie_cap_offset = None;

    let status = pci_config_read_u16(config, 0x06);
    let has_cap_list = status & PCI_STATUS_CAP_LIST != 0;
    if !has_cap_list || config.len() < 0x40 {
        return (capabilities, pm_cap_offset, pcie_cap_offset);
    }

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

        if cap_id == PCI_CAP_ID_PM {
            pm_cap_offset = Some(cap_ptr as u8);
        }
        if cap_id == PCI_CAP_ID_PCIE {
            pcie_cap_offset = Some(cap_ptr as u8);
        }

        cap_ptr = (config[cap_ptr + 1] & 0xFC) as usize;
    }

    (capabilities, pm_cap_offset, pcie_cap_offset)
}

fn pci_power_info_without_pm_capability() -> PciPowerInfo {
    PciPowerInfo {
        pm_cap_offset: None,
        current_state: PciPmState::Unknown(0xFF),
        d1_support: false,
        d2_support: false,
        pme_support: 0,
        pmcsr_raw: 0,
    }
}

fn parse_pci_power_info_from_config(config: &[u8], pm_cap_offset: u8) -> PciPowerInfo {
    let pm_off = pm_cap_offset as usize;
    if pm_off + 6 <= config.len() {
        let pmc = pci_config_read_u16(config, pm_off + 2);
        let pmcsr = pci_config_read_u16(config, pm_off + 4);
        PciPowerInfo {
            pm_cap_offset: Some(pm_cap_offset),
            current_state: PciPmState::from_pmcsr_bits((pmcsr & 0x03) as u8),
            d1_support: pmc & (1 << 9) != 0,
            d2_support: pmc & (1 << 10) != 0,
            pme_support: ((pmc >> 11) & 0x1F) as u8,
            pmcsr_raw: pmcsr,
        }
    } else {
        PciPowerInfo {
            pm_cap_offset: Some(pm_cap_offset),
            current_state: PciPmState::Unknown(0xFF),
            d1_support: false,
            d2_support: false,
            pme_support: 0,
            pmcsr_raw: 0,
        }
    }
}

fn parse_pcie_link_from_config(config: &[u8], pcie_cap_offset: u8) -> Option<PcieLinkInfo> {
    let off = pcie_cap_offset as usize;
    if off + 0x14 > config.len() {
        return None;
    }
    let link_cap = pci_config_read_u32(config, off + 0x0C);
    let link_sta = pci_config_read_u16(config, off + 0x12);
    Some(PcieLinkInfo {
        max_speed: PcieLinkSpeed::from_encoding((link_cap & 0x0F) as u8),
        current_speed: PcieLinkSpeed::from_encoding((link_sta & 0x0F) as u8),
        max_width: ((link_cap >> 4) & 0x3F) as u8,
        current_width: ((link_sta >> 4) & 0x3F) as u8,
    })
}

/// Locate the PM capability offset in config space (first PM cap in the chain).
fn find_pm_capability_offset(config: &[u8]) -> Result<usize, String> {
    if config.len() < 0x40 {
        return Err("PCI config too short".into());
    }

    let status = pci_config_read_u16(config, 0x06);
    if status & PCI_STATUS_CAP_LIST == 0 {
        return Err("No PCI capabilities list".into());
    }

    let mut cap_ptr = (config[0x34] & 0xFC) as usize;
    let mut visited = HashSet::new();
    while cap_ptr != 0 && !visited.contains(&cap_ptr) && cap_ptr + 2 <= config.len() {
        visited.insert(cap_ptr);
        if config[cap_ptr] == PCI_CAP_ID_PM {
            return Ok(cap_ptr);
        }
        cap_ptr = (config[cap_ptr + 1] & 0xFC) as usize;
    }

    Err("PM capability not found".into())
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
    /// Parse PCI device info from raw config space bytes (for testing without sysfs).
    ///
    /// `config` must be at least 64 bytes. `bars` and `pcie_link` are passed through
    /// (from sysfs in production; use empty/None for unit tests).
    ///
    /// # Errors
    ///
    /// Returns an error if config is too short.
    pub fn from_config_bytes(
        bdf: &str,
        config: &[u8],
        bars: Vec<PciBar>,
        pcie_link: Option<PcieLinkInfo>,
    ) -> Result<Self, String> {
        if config.len() < 64 {
            return Err(format!("PCI config too short: {} bytes", config.len()));
        }

        let vendor_id = pci_config_read_u16(config, 0x00);
        let device_id = pci_config_read_u16(config, 0x02);
        let class_code = pci_config_read_u32(config, 0x08) >> 8;
        let subsystem = (
            pci_config_read_u16(config, 0x2C),
            pci_config_read_u16(config, 0x2E),
        );

        let (capabilities, pm_cap_offset, pcie_cap_offset) = walk_pci_capability_chain(config);

        let power = if let Some(pm_off) = pm_cap_offset {
            parse_pci_power_info_from_config(config, pm_off)
        } else {
            pci_power_info_without_pm_capability()
        };

        let pcie_link_parsed =
            pcie_cap_offset.and_then(|off| parse_pcie_link_from_config(config, off));
        let pcie_link_final = pcie_link_parsed.or(pcie_link);

        Ok(Self {
            bdf: bdf.to_string(),
            vendor_id,
            device_id,
            class_code,
            subsystem,
            bars,
            capabilities,
            power,
            pcie_link: pcie_link_final,
            vendor: GpuVendor::from_vendor_id(vendor_id),
        })
    }

    /// Parse PCI device info from sysfs config space.
    pub fn from_sysfs(bdf: &str) -> Result<Self, String> {
        parse_pci_bdf(bdf).ok_or_else(|| format!("invalid PCI BDF: {bdf}"))?;

        let config_path = linux_paths::sysfs_pci_device_file(bdf, "config");
        let config = std::fs::read(&config_path).map_err(|e| format!("read {config_path}: {e}"))?;

        if config.len() < 64 {
            return Err(format!("PCI config too short: {} bytes", config.len()));
        }

        let vendor_id = pci_config_read_u16(&config, 0x00);
        let device_id = pci_config_read_u16(&config, 0x02);
        if let Some(sys_vendor) = read_sysfs_pci_hex_id(bdf, "vendor")
            && sys_vendor != vendor_id
        {
            tracing::warn!(
                bdf,
                config_vendor = vendor_id,
                sysfs_vendor = sys_vendor,
                "PCI vendor ID mismatch (config vs sysfs)"
            );
        }
        let class_code = pci_config_read_u32(&config, 0x08) >> 8;
        let subsystem = (
            pci_config_read_u16(&config, 0x2C),
            pci_config_read_u16(&config, 0x2E),
        );

        // Parse BARs from sysfs resource file (more reliable than config space)
        let bars = Self::parse_bars_sysfs(bdf);

        // Walk capability chain.
        // vfio-pci may only expose 64 bytes of config space via sysfs,
        // so the capability pointer (0x34) may reference offsets beyond
        // what we have. We try the full config first, then fall back to
        // inferring common capabilities from the vendor ID.
        let status = pci_config_read_u16(&config, 0x06);
        let has_cap_list = status & PCI_STATUS_CAP_LIST != 0;
        let (mut capabilities, pm_cap_offset, pcie_cap_offset) = walk_pci_capability_chain(&config);

        // If vfio-pci truncated config space, populate from sysfs attributes
        if capabilities.is_empty() && has_cap_list {
            let dev_path = linux_paths::sysfs_pci_device_path(bdf);
            // PCIe link info from sysfs as evidence of PCIe capability
            if std::path::Path::new(&format!("{dev_path}/current_link_speed")).exists() {
                capabilities.push(PciCapability {
                    id: PCI_CAP_ID_PCIE,
                    offset: 0,
                    name: "PCI Express (from sysfs)",
                });
            }
            // Power state from sysfs as evidence of PM capability
            if std::path::Path::new(&format!("{dev_path}/power_state")).exists() {
                capabilities.push(PciCapability {
                    id: PCI_CAP_ID_PM,
                    offset: 0,
                    name: "Power Management (from sysfs)",
                });
            }
        }

        // Parse PM capability — try config space first, then sysfs fallback
        let power = if let Some(pm_off) = pm_cap_offset {
            parse_pci_power_info_from_config(&config, pm_off)
        } else {
            // sysfs fallback: read power_state if config space was truncated
            let dev_path = linux_paths::sysfs_pci_device_path(bdf);
            let sysfs_state = std::fs::read_to_string(format!("{dev_path}/power_state"))
                .ok()
                .and_then(|s| parse_sysfs_power_state(&s));
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
            .and_then(|off| parse_pcie_link_from_config(&config, off))
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
        let dev = linux_paths::sysfs_pci_device_path(bdf);
        let cur_speed = std::fs::read_to_string(format!("{dev}/current_link_speed")).ok()?;
        let max_speed =
            std::fs::read_to_string(format!("{dev}/max_link_speed")).unwrap_or_default();
        let cur_width =
            std::fs::read_to_string(format!("{dev}/current_link_width")).unwrap_or_default();
        let max_width =
            std::fs::read_to_string(format!("{dev}/max_link_width")).unwrap_or_default();
        Some(PcieLinkInfo {
            current_speed: parse_sysfs_pcie_speed(&cur_speed),
            max_speed: parse_sysfs_pcie_speed(&max_speed),
            current_width: parse_sysfs_pcie_width(&cur_width),
            max_width: parse_sysfs_pcie_width(&max_width),
        })
    }

    fn parse_bars_sysfs(bdf: &str) -> Vec<PciBar> {
        let resource_path = linux_paths::sysfs_pci_device_file(bdf, "resource");
        let content = match std::fs::read_to_string(&resource_path) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        parse_pci_resource_file(&content)
    }

    /// Print a human-readable summary of the device.
    pub fn print_summary(&self) {
        let mut s = String::new();
        writeln!(
            &mut s,
            "╠══ PCI DEVICE INFO ═════════════════════════════════════════╣"
        )
        .expect("writing to String is infallible");
        writeln!(&mut s, "║ BDF:     {}", self.bdf).expect("writing to String is infallible");
        writeln!(
            &mut s,
            "║ ID:      {:04x}:{:04x} ({})",
            self.vendor_id, self.device_id, self.vendor
        )
        .expect("writing to String is infallible");
        writeln!(
            &mut s,
            "║ Class:   {:#08x} (base {:#04x})",
            self.class_code,
            pci_class_base(self.class_code)
        )
        .expect("writing to String is infallible");
        writeln!(
            &mut s,
            "║ Power:   {} (PMCSR={:#06x})",
            self.power.current_state, self.power.pmcsr_raw
        )
        .expect("writing to String is infallible");
        for bar in &self.bars {
            writeln!(
                &mut s,
                "║ BAR{}:    {:#014x} ({} KB) {}{}{}",
                bar.index,
                bar.base,
                bar.size / 1024,
                if bar.is_mmio { "MMIO" } else { "IO" },
                if bar.is_64bit { " 64bit" } else { "" },
                if bar.is_prefetchable { " prefetch" } else { "" },
            )
            .expect("writing to String is infallible");
        }
        for cap in &self.capabilities {
            writeln!(
                &mut s,
                "║ Cap:     [{:#04x}] {} @ {:#04x}",
                cap.id, cap.name, cap.offset
            )
            .expect("writing to String is infallible");
        }
        if let Some(ref link) = self.pcie_link {
            writeln!(
                &mut s,
                "║ PCIe:    x{} @ {} (max x{} @ {})",
                link.current_width, link.current_speed, link.max_width, link.max_speed,
            )
            .expect("writing to String is infallible");
        }
        tracing::info!(summary = %s, "PCI device info");
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
    parse_pci_bdf(bdf).ok_or_else(|| format!("invalid PCI BDF: {bdf}"))?;
    let config_path = linux_paths::sysfs_pci_device_file(bdf, "config");
    let config = std::fs::read(&config_path).map_err(|e| format!("read PCI config: {e}"))?;

    let pm_off = find_pm_capability_offset(&config)?;
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
    let new_pmcsr_masked = pmcsr & !0x03;
    tracing::info!(
        from_state = pm_states[current_state as usize],
        pmcsr = format!("{pmcsr:#06x}"),
        new_pmcsr = format!("{new_pmcsr_masked:#06x}"),
        pmcsr_off = format!("{pmcsr_off:#04x}"),
        "PCI PM transition to D0"
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

    // Pin runtime PM to "on" so the kernel doesn't put the device back to D3hot
    let power_control = linux_paths::sysfs_pci_device_file(bdf, "power/control");
    if let Err(e) = std::fs::write(&power_control, "on") {
        tracing::warn!(error = %e, path = %power_control, "could not pin power/control=on");
    }

    Ok(())
}

/// Transition a PCI device to a specific power state.
///
/// Writes the target state to PMCSR bits \[1:0\]. Observe PCI spec recovery
/// delays: D3hot→D0 requires 10ms, D2→D0 requires 200µs, etc.
pub fn set_pci_power_state(bdf: &str, target: PciPmState) -> Result<PciPmState, String> {
    parse_pci_bdf(bdf).ok_or_else(|| format!("invalid PCI BDF: {bdf}"))?;
    let config_path = linux_paths::sysfs_pci_device_file(bdf, "config");
    let config = std::fs::read(&config_path).map_err(|e| format!("read PCI config: {e}"))?;

    let pm_off = find_pm_capability_offset(&config)?;
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
    parse_pci_bdf(bdf).ok_or_else(|| format!("invalid PCI BDF: {bdf}"))?;
    let dev_path = linux_paths::sysfs_pci_device_path(bdf);

    let driver_link = format!("{dev_path}/driver");
    if std::fs::read_link(&driver_link).is_ok() {
        return Err("Device has a driver bound — unbind first".into());
    }

    let _ = std::fs::write(format!("{dev_path}/d3cold_allowed"), "1");
    let _ = std::fs::write(format!("{dev_path}/power/control"), "auto");

    std::fs::write(format!("{dev_path}/remove"), "1").map_err(|e| format!("remove failed: {e}"))?;

    std::thread::sleep(std::time::Duration::from_secs(2));

    std::fs::write(linux_paths::sysfs_pci_bus_rescan(), "1")
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
pub fn snapshot_config_space(
    bdf: &str,
    start: usize,
    end: usize,
) -> Result<Vec<(usize, u32)>, String> {
    parse_pci_bdf(bdf).ok_or_else(|| format!("invalid PCI BDF: {bdf}"))?;
    let config_path = linux_paths::sysfs_pci_device_file(bdf, "config");
    let config = std::fs::read(&config_path).map_err(|e| format!("read config: {e}"))?;

    let mut regs = Vec::new();
    let end = end.min(config.len());
    for off in (start..end).step_by(4) {
        if off + 4 <= config.len() {
            let val = u32::from_le_bytes([
                config[off],
                config[off + 1],
                config[off + 2],
                config[off + 3],
            ]);
            regs.push((off, val));
        }
    }
    Ok(regs)
}

#[cfg(test)]
#[path = "pci_discovery_tests.rs"]
mod tests;
