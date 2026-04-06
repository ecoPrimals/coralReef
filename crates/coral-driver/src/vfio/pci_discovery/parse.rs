// SPDX-License-Identifier: AGPL-3.0-or-later
//! Sysfs and textual PCI helpers (BDF strings, `resource` lines, power/link sysfs).

use super::types::{PciBar, PciPmState, PcieLinkSpeed};

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
