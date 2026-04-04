// SPDX-License-Identifier: AGPL-3.0-only
//! JSON-RPC guard helpers (managed BDF allowlist, reset method ordering).

use std::collections::HashSet;
use std::io::Write;

use crate::error::{EmberIpcError, SysfsError};
use crate::sysfs;

use super::jsonrpc::write_jsonrpc_error;

/// Completes after [`require_managed_bdf`]: `Err(Ok(()))` means the JSON-RPC error was already sent.
pub(crate) fn finish_managed_bdf_early(
    r: Result<(), Result<(), std::io::Error>>,
) -> Result<(), EmberIpcError> {
    match r {
        Ok(()) => Ok(()),
        Err(Ok(())) => Ok(()),
        Err(Err(e)) => Err(EmberIpcError::from(e)),
    }
}

/// Reject a BDF that is not in the managed set from `glowplug.toml`.
pub(crate) fn require_managed_bdf(
    bdf: &str,
    managed: &HashSet<String>,
    stream: &mut impl Write,
    id: &serde_json::Value,
) -> Result<(), Result<(), std::io::Error>> {
    if managed.contains(bdf) {
        return Ok(());
    }
    tracing::warn!(bdf, "BDF not in managed allowlist — rejecting RPC");
    let msg = format!(
        "BDF {bdf} is not managed by ember (not listed in glowplug.toml). \
         Only configured devices are accepted."
    );
    write_jsonrpc_error(stream, id.clone(), -32001, &msg).map_err(Err)?;
    Err(Ok(()))
}

/// Try reset methods in priority order until one succeeds.
pub(crate) fn try_reset_methods(
    bdf: &str,
    methods: &[crate::vendor_lifecycle::ResetMethod],
) -> Result<(), SysfsError> {
    let mut last_err = String::new();
    for m in methods {
        let (label, result) = match m {
            crate::vendor_lifecycle::ResetMethod::VfioFlr => {
                last_err = "FLR not available via ember (use GlowPlug device.reset)".to_string();
                continue;
            }
            crate::vendor_lifecycle::ResetMethod::BridgeSbr => {
                ("bridge-sbr", sysfs::pci_bridge_reset(bdf))
            }
            crate::vendor_lifecycle::ResetMethod::SysfsSbr => ("sbr", sysfs::pci_device_reset(bdf)),
            crate::vendor_lifecycle::ResetMethod::RemoveRescan => {
                ("remove-rescan", sysfs::pci_remove_rescan(bdf))
            }
        };
        tracing::info!(bdf, method = label, "trying reset method");
        match result {
            Ok(()) => return Ok(()),
            Err(e) => {
                tracing::warn!(bdf, method = label, error = %e, "reset method failed, trying next");
                last_err = format!("{label}: {e}");
            }
        }
    }
    Err(SysfsError::PciReset {
        bdf: bdf.to_string(),
        reason: last_err,
    })
}
