// SPDX-License-Identifier: AGPL-3.0-only
//! JSON-RPC method handlers — structured by domain.
//!
//! - [`device_ops`]: sync device lifecycle operations (list, get, swap, health, …)
//! - [`compute`]: async compute dispatch and MMU oracle capture
//! - [`quota`]: nvidia-smi telemetry and quota management

mod compute;
mod device_ops;
mod quota;

pub(crate) use compute::{compute_dispatch_async, oracle_capture_async};
pub(crate) use device_ops::dispatch;
pub(crate) use quota::{compute_info_async, quota_info_async, set_quota_async};

use super::protocol::DeviceInfo;

/// Validate that a BDF string matches the expected PCI address format.
///
/// Rejects path traversal attempts, null bytes, and malformed addresses
/// that could be interpolated into sysfs paths by device operations.
pub(crate) fn validate_bdf(bdf: &str) -> Result<&str, coral_glowplug::error::RpcError> {
    let is_valid = !bdf.is_empty()
        && bdf.len() <= 16
        && !bdf.contains('/')
        && !bdf.contains('\0')
        && !bdf.contains("..")
        && bdf
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.');
    if is_valid {
        Ok(bdf)
    } else {
        Err(coral_glowplug::error::RpcError::invalid_params(format!(
            "invalid BDF address: {bdf:?}"
        )))
    }
}

fn device_to_info(d: &coral_glowplug::device::DeviceSlot) -> DeviceInfo {
    DeviceInfo {
        bdf: d.bdf.to_string(),
        name: d.config.name.clone(),
        chip: d.chip_name.clone(),
        vendor_id: d.vendor_id,
        device_id: d.device_id,
        personality: d.personality.to_string(),
        role: d.config.role.clone(),
        power: d.health.power.to_string(),
        vram_alive: d.health.vram_alive,
        domains_alive: d.health.domains_alive,
        domains_faulted: d.health.domains_faulted,
        has_vfio_fd: d.has_vfio(),
        pci_link_width: d.health.pci_link_width,
        protected: d.config.is_protected(),
    }
}

#[cfg(test)]
pub(super) fn test_device_config(bdf: &str) -> coral_glowplug::config::DeviceConfig {
    coral_glowplug::config::DeviceConfig {
        bdf: bdf.into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_bdf_accepts_max_length_hex_address() {
        let s = "0000:ab:cd.ef";
        assert_eq!(validate_bdf(s).expect("valid BDF"), s);
    }
}
