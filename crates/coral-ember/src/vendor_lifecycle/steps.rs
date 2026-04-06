// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pure lifecycle step lists and sysfs execution (injectable [`crate::sysfs::SysfsPort`]).

use coral_driver::linux_paths;

use crate::error::SwapError;
use crate::sysfs::{self, SysfsPort};

/// One sysfs-side effect used during `prepare_for_unbind` (ordering matters).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleStep {
    /// Pin device and bridge power rails (`power/control`, `d3cold_allowed`).
    PinPower,
    /// Pin upstream bridge power (walks PCI parents).
    PinBridgePower,
    /// Clear PCI `reset_method` (write empty / newline semantics).
    ClearResetMethod,
}

/// Run `steps` in order via [`SysfsPort`] (for tests and production).
///
/// # Errors
///
/// Propagates sysfs write failures.
pub fn execute_lifecycle_steps(
    bdf: &str,
    steps: &[LifecycleStep],
    sysfs: &dyn SysfsPort,
) -> Result<(), SwapError> {
    for step in steps {
        match *step {
            LifecycleStep::PinPower => {
                sysfs::pin_power_with(sysfs, bdf);
            }
            LifecycleStep::PinBridgePower => {
                sysfs::pin_bridge_power_with(sysfs, bdf);
            }
            LifecycleStep::ClearResetMethod => {
                let path = linux_paths::sysfs_pci_device_file(bdf, "reset_method");
                let _ = sysfs::sysfs_write_direct_with(sysfs, &path, "");
            }
        }
    }
    Ok(())
}

/// Steps for NVIDIA Kepler [`super::NvidiaKeplerLifecycle`] before unbind.
#[must_use]
pub fn nvidia_kepler_lifecycle_prepare_steps() -> Vec<LifecycleStep> {
    vec![
        LifecycleStep::PinPower,
        LifecycleStep::PinBridgePower,
        LifecycleStep::ClearResetMethod,
    ]
}

/// Steps for [`super::GenericLifecycle`] before unbind (mirrors prior behavior).
#[must_use]
pub fn generic_lifecycle_prepare_steps(current_driver: &str) -> Vec<LifecycleStep> {
    let mut steps = vec![LifecycleStep::PinPower];
    if current_driver == "vfio-pci" {
        steps.push(LifecycleStep::ClearResetMethod);
    }
    steps
}

#[cfg(test)]
mod tests {
    use super::{
        LifecycleStep, generic_lifecycle_prepare_steps, nvidia_kepler_lifecycle_prepare_steps,
    };

    #[test]
    fn generic_prepare_non_vfio_is_pin_power_only() {
        assert_eq!(
            generic_lifecycle_prepare_steps("nouveau"),
            vec![LifecycleStep::PinPower]
        );
    }

    #[test]
    fn generic_prepare_vfio_adds_clear_reset_method() {
        assert_eq!(
            generic_lifecycle_prepare_steps("vfio-pci"),
            vec![LifecycleStep::PinPower, LifecycleStep::ClearResetMethod]
        );
    }

    #[test]
    fn nvidia_kepler_prepare_three_steps() {
        let s = nvidia_kepler_lifecycle_prepare_steps();
        assert_eq!(s.len(), 3);
        assert_eq!(
            s,
            vec![
                LifecycleStep::PinPower,
                LifecycleStep::PinBridgePower,
                LifecycleStep::ClearResetMethod,
            ]
        );
    }
}
