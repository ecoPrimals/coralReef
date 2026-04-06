// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pure PCI reset planning for `device.reset` JSON-RPC.

use crate::error::SwapError;
use crate::sysfs;
use crate::vendor_lifecycle::{ResetMethod, VendorLifecycle};

/// Explicit sysfs reset op for a single-shot `device.reset` request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetDirectOp {
    /// Device-level `reset` sysfs file.
    Sbr,
    /// Parent bridge `reset` file.
    BridgeSbr,
    /// PCI remove + bus rescan.
    RemoveRescan,
}

/// What to run for `device.reset` after validation.
#[derive(Debug, Clone)]
pub enum DeviceResetPlan {
    /// One fixed method (`sbr`, `bridge-sbr`, `remove-rescan`).
    Direct(ResetDirectOp),
    /// `auto`: try lifecycle-ordered methods until one succeeds.
    Auto {
        /// Priority-ordered methods from [`VendorLifecycle::available_reset_methods`].
        methods: Vec<ResetMethod>,
    },
}

/// Decide reset behavior from the RPC `method` string and detected lifecycle.
///
/// # Errors
///
/// Returns [`SwapError::InvalidResetMethod`] when `method` is not one of the supported names.
pub fn device_reset_plan(
    method: &str,
    lifecycle: &dyn VendorLifecycle,
) -> Result<DeviceResetPlan, SwapError> {
    match method {
        "sbr" => Ok(DeviceResetPlan::Direct(ResetDirectOp::Sbr)),
        "bridge-sbr" => Ok(DeviceResetPlan::Direct(ResetDirectOp::BridgeSbr)),
        "remove-rescan" => Ok(DeviceResetPlan::Direct(ResetDirectOp::RemoveRescan)),
        "auto" => Ok(DeviceResetPlan::Auto {
            methods: lifecycle.available_reset_methods(),
        }),
        other => Err(SwapError::InvalidResetMethod(format!(
            "{other} (use 'auto', 'sbr', 'bridge-sbr', 'remove-rescan')"
        ))),
    }
}

/// Perform sysfs resets according to `plan`.
///
/// # Errors
///
/// Propagates sysfs and ordering failures.
pub fn execute_device_reset_plan(bdf: &str, plan: &DeviceResetPlan) -> Result<(), SwapError> {
    match plan {
        DeviceResetPlan::Direct(ResetDirectOp::Sbr) => {
            sysfs::pci_device_reset(bdf).map_err(Into::into)
        }
        DeviceResetPlan::Direct(ResetDirectOp::BridgeSbr) => {
            sysfs::pci_bridge_reset(bdf).map_err(Into::into)
        }
        DeviceResetPlan::Direct(ResetDirectOp::RemoveRescan) => {
            sysfs::pci_remove_rescan(bdf).map_err(Into::into)
        }
        DeviceResetPlan::Auto { methods } => {
            super::helpers::try_reset_methods(bdf, methods).map_err(Into::into)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DeviceResetPlan, ResetDirectOp, device_reset_plan};
    use crate::vendor_lifecycle::{
        NvidiaKeplerLifecycle, NvidiaLifecycle, ResetMethod, VendorLifecycle,
    };

    #[test]
    fn device_reset_plan_direct_ops() {
        let lc = NvidiaLifecycle { device_id: 0 };
        assert!(matches!(
            device_reset_plan("sbr", &lc).unwrap(),
            DeviceResetPlan::Direct(ResetDirectOp::Sbr)
        ));
        assert!(matches!(
            device_reset_plan("bridge-sbr", &lc).unwrap(),
            DeviceResetPlan::Direct(ResetDirectOp::BridgeSbr)
        ));
        assert!(matches!(
            device_reset_plan("remove-rescan", &lc).unwrap(),
            DeviceResetPlan::Direct(ResetDirectOp::RemoveRescan)
        ));
    }

    #[test]
    fn device_reset_plan_auto_copies_lifecycle_methods() {
        let lc = NvidiaLifecycle { device_id: 0 };
        let plan = device_reset_plan("auto", &lc).expect("auto");
        match plan {
            DeviceResetPlan::Auto { methods } => {
                assert_eq!(methods, lc.available_reset_methods());
            }
            _ => panic!("expected Auto"),
        }
    }

    #[test]
    fn device_reset_plan_auto_kepler_methods() {
        let lc = NvidiaKeplerLifecycle { device_id: 0x102d };
        let plan = device_reset_plan("auto", &lc).expect("auto");
        match plan {
            DeviceResetPlan::Auto { methods } => {
                assert_eq!(methods, vec![ResetMethod::RemoveRescan]);
            }
            _ => panic!("expected Auto"),
        }
    }

    #[test]
    fn device_reset_plan_invalid_method_errors() {
        let lc = NvidiaLifecycle { device_id: 0 };
        let err = device_reset_plan("flr-only", &lc).expect_err("bad method");
        assert!(matches!(
            err,
            crate::error::SwapError::InvalidResetMethod(_)
        ));
    }
}
