// SPDX-License-Identifier: AGPL-3.0-only
//! JSON-RPC guard helpers (managed BDF allowlist, reset method ordering).

use std::collections::HashSet;
use std::io::Write;

use crate::sysfs;

use super::jsonrpc::write_jsonrpc_error;

/// Reject a BDF that is not in the managed set from `glowplug.toml`.
pub(crate) fn require_managed_bdf(
    bdf: &str,
    managed: &HashSet<String>,
    stream: &mut impl Write,
    id: serde_json::Value,
) -> Result<(), Result<(), std::io::Error>> {
    if managed.contains(bdf) {
        return Ok(());
    }
    tracing::warn!(bdf, "BDF not in managed allowlist — rejecting RPC");
    let msg = format!(
        "BDF {bdf} is not managed by ember (not listed in glowplug.toml). \
         Only configured devices are accepted."
    );
    write_jsonrpc_error(stream, id, -32001, &msg).map_err(Err)?;
    Err(Ok(()))
}

/// Try reset methods in priority order until one succeeds.
pub(crate) fn try_reset_methods(
    bdf: &str,
    methods: &[crate::vendor_lifecycle::ResetMethod],
) -> Result<(), String> {
    let mut last_err = String::new();
    for m in methods {
        let label = match m {
            crate::vendor_lifecycle::ResetMethod::BridgeSbr => "bridge-sbr",
            crate::vendor_lifecycle::ResetMethod::SysfsSbr => "sbr",
            crate::vendor_lifecycle::ResetMethod::RemoveRescan => "remove-rescan",
            crate::vendor_lifecycle::ResetMethod::VfioFlr => {
                last_err = "FLR not available via ember (use GlowPlug device.reset)".to_string();
                continue;
            }
        };
        tracing::info!(bdf, method = label, "trying reset method");
        let result = match m {
            crate::vendor_lifecycle::ResetMethod::BridgeSbr => sysfs::pci_bridge_reset(bdf),
            crate::vendor_lifecycle::ResetMethod::SysfsSbr => sysfs::pci_device_reset(bdf),
            crate::vendor_lifecycle::ResetMethod::RemoveRescan => sysfs::pci_remove_rescan(bdf),
            crate::vendor_lifecycle::ResetMethod::VfioFlr => unreachable!(),
        };
        match result {
            Ok(()) => return Ok(()),
            Err(e) => {
                tracing::warn!(bdf, method = label, error = %e, "reset method failed, trying next");
                last_err = format!("{label}: {e}");
            }
        }
    }
    Err(last_err)
}
