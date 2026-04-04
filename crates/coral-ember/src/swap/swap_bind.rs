// SPDX-License-Identifier: AGPL-3.0-only
//! sysfs bind-time operations for [`super::handle_swap_device`] (VFIO and native DRM/compute drivers).

use crate::error::SwapError;
use crate::hold::HeldDevice;
use crate::sysfs;
use crate::vendor_lifecycle::{self, RebindStrategy};
use coral_driver::linux_paths;
use std::collections::HashMap;

use super::swap_preflight;

pub(super) fn bind_vfio(
    bdf: &str,
    held: &mut HashMap<String, HeldDevice>,
    lifecycle: &dyn vendor_lifecycle::VendorLifecycle,
) -> Result<String, SwapError> {
    let group_id = sysfs::read_iommu_group(bdf);

    sysfs::sysfs_write(
        &linux_paths::sysfs_pci_device_file(bdf, "driver_override"),
        "vfio-pci",
    )?;

    sysfs::bind_iommu_group_to_vfio(bdf, group_id);

    let _ = sysfs::sysfs_write(&linux_paths::sysfs_pci_driver_bind("vfio-pci"), bdf);

    let reset_path = linux_paths::sysfs_pci_device_file(bdf, "reset_method");
    if let Err(e) = sysfs::sysfs_write_direct(&reset_path, "") {
        tracing::warn!(bdf, error = %e, "failed to disable reset_method after vfio-pci bind");
    } else {
        tracing::info!(bdf, "reset_method disabled immediately after vfio-pci bind");
    }

    let settle = lifecycle.settle_secs("vfio-pci");
    std::thread::sleep(std::time::Duration::from_secs(settle));

    lifecycle.stabilize_after_bind(bdf, "vfio-pci");
    lifecycle.verify_health(bdf, "vfio-pci")?;

    match coral_driver::vfio::VfioDevice::open(bdf) {
        Ok(device) => {
            let req_eventfd = crate::arm_req_irq(&device, bdf);
            tracing::info!(
                bdf,
                backend = ?device.backend_kind(),
                device_fd = device.device_fd(),
                req_armed = req_eventfd.is_some(),
                "swap_device: VFIO fds reacquired"
            );
            held.insert(
                bdf.to_string(),
                HeldDevice {
                    bdf: bdf.to_string(),
                    device,
                    ring_meta: crate::hold::RingMeta::default(),
                    req_eventfd,
                },
            );
        }
        Err(e) => {
            return Err(SwapError::VfioReacquire {
                bdf: bdf.to_string(),
                reason: e.to_string(),
            });
        }
    }

    Ok("vfio".to_string())
}

fn pci_remove_rescan(bdf: &str, target_driver: Option<&str>) -> Result<(), SwapError> {
    Ok(sysfs::pci_remove_rescan_targeted(bdf, target_driver)?)
}

pub(super) fn is_drm_driver(target: &str) -> bool {
    matches!(target, "nouveau" | "nvidia" | "amdgpu" | "xe" | "i915")
        || target.starts_with("nvidia_oracle")
}

/// Ensure the kernel module for `target` is loaded before attempting sysfs bind.
/// If the module is already loaded or doesn't need loading (e.g. vfio-pci), this is a no-op.
fn ensure_module_loaded(target: &str) {
    let module = match target {
        "vfio" | "vfio-pci" => return,
        "akida-pcie" => "akida_pcie",
        other => other,
    };

    let sysfs_mod = format!("/sys/module/{}", module.replace('-', "_"));
    if std::path::Path::new(&sysfs_mod).exists() {
        tracing::debug!(module, "kernel module already loaded");
        return;
    }

    tracing::info!(module, "loading kernel module for target driver");
    match std::process::Command::new("modprobe").arg(module).output() {
        Ok(out) if out.status.success() => {
            tracing::info!(module, "kernel module loaded successfully");
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            tracing::warn!(module, %stderr, "modprobe returned non-zero (may still work via install hook)");
        }
        Err(e) => {
            tracing::warn!(module, error = %e, "modprobe not available — bind may fail if module not loaded");
        }
    }

    std::thread::sleep(std::time::Duration::from_millis(500));
}

pub(super) fn bind_native(
    bdf: &str,
    target: &str,
    lifecycle: &dyn vendor_lifecycle::VendorLifecycle,
) -> Result<String, SwapError> {
    if is_drm_driver(target) {
        swap_preflight::verify_drm_isolation(bdf)?;
    }

    let group_id = sysfs::read_iommu_group(bdf);
    if group_id != 0 {
        sysfs::release_iommu_group_from_vfio(bdf, group_id);
    }

    let strategy = lifecycle.rebind_strategy(target);
    tracing::info!(
        bdf,
        target,
        ?strategy,
        "swap_device: rebind strategy selected"
    );

    sysfs::sysfs_write(
        &linux_paths::sysfs_pci_device_file(bdf, "driver_override"),
        target,
    )?;
    std::thread::sleep(std::time::Duration::from_millis(200));

    ensure_module_loaded(target);

    match strategy {
        RebindStrategy::SimpleBind => {
            let _ = sysfs::sysfs_write(&linux_paths::sysfs_pci_driver_bind(target), bdf);
        }
        RebindStrategy::SimpleWithRescanFallback => {
            let bind_result = sysfs::sysfs_write(&linux_paths::sysfs_pci_driver_bind(target), bdf);

            if bind_result.is_err() {
                tracing::warn!(
                    bdf,
                    target,
                    "simple bind failed — falling back to PCI remove + rescan"
                );
                pci_remove_rescan(bdf, Some(target))?;
            }
        }
        RebindStrategy::PciRescan => {
            tracing::info!(
                bdf,
                target,
                "using PCI remove + rescan (skipping simple bind)"
            );
            pci_remove_rescan(bdf, Some(target))?;
        }
        RebindStrategy::PmResetAndBind => {
            tracing::info!(bdf, target, "PM power cycle before bind");
            match sysfs::pm_power_cycle(bdf) {
                Ok(()) => {
                    tracing::info!(bdf, "PM cycle OK, attempting bind");
                    let _ = sysfs::sysfs_write_direct(
                        &linux_paths::sysfs_pci_device_file(bdf, "reset_method"),
                        "",
                    );
                }
                Err(e) => {
                    tracing::warn!(bdf, error = %e, "PM power cycle failed, attempting bind anyway");
                }
            }

            let bind_result = sysfs::sysfs_write(&linux_paths::sysfs_pci_driver_bind(target), bdf);

            if bind_result.is_err() {
                tracing::warn!(
                    bdf,
                    target,
                    "simple bind after PM cycle failed — trying rescan fallback"
                );
                pci_remove_rescan(bdf, Some(target))?;
            }
        }
    }

    let wait_secs = lifecycle.settle_secs(target);
    for attempt in 0..wait_secs {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let drv = sysfs::read_current_driver(bdf);
        if drv.as_deref() == Some(target) && sysfs::find_drm_card(bdf).is_some() {
            tracing::info!(
                bdf,
                target,
                attempt,
                "swap_device: driver init complete (DRM up)"
            );
            break;
        }
        tracing::debug!(bdf, target, attempt, driver = ?drv, "swap_device: waiting for driver init");
    }

    lifecycle.stabilize_after_bind(bdf, target);

    let actual = sysfs::read_current_driver(bdf);
    if actual.as_deref() != Some(target) {
        tracing::warn!(
            bdf, target,
            actual = ?actual,
            "swap_device: target driver did not bind"
        );
    }

    lifecycle.verify_health(bdf, target)?;

    Ok(target.to_string())
}

#[cfg(test)]
mod tests {
    use super::is_drm_driver;

    #[test]
    fn is_drm_driver_matches_drm_targets() {
        assert!(is_drm_driver("nouveau"));
        assert!(is_drm_driver("nvidia"));
        assert!(is_drm_driver("amdgpu"));
        assert!(is_drm_driver("xe"));
        assert!(is_drm_driver("i915"));
    }

    #[test]
    fn is_drm_driver_rejects_non_drm() {
        assert!(!is_drm_driver("vfio-pci"));
        assert!(!is_drm_driver("akida-pcie"));
        assert!(!is_drm_driver("unbound"));
    }

    #[test]
    fn is_drm_driver_matches_nvidia_oracle_prefix() {
        assert!(is_drm_driver("nvidia_oracle"));
        assert!(is_drm_driver("nvidia_oracle_535"));
    }
}
