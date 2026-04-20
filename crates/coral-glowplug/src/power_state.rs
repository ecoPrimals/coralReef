// SPDX-License-Identifier: AGPL-3.0-or-later
//! PCI power state (`D0` / `D3hot` / `D3cold`) shared by sysfs helpers and the Linux device stack.

/// ACPI PCI power state derived from sysfs `power_state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerState {
    /// Fully on.
    D0,
    /// D3hot — device in low-power state, configuration space accessible.
    D3Hot,
    /// D3cold — power may be removed.
    D3Cold,
    /// Unknown or unreadable.
    Unknown,
}

impl std::fmt::Display for PowerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::D0 => write!(f, "D0"),
            Self::D3Hot => write!(f, "D3hot"),
            Self::D3Cold => write!(f, "D3cold"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}
