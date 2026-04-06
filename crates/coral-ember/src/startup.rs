// SPDX-License-Identifier: AGPL-3.0-or-later
//! Daemon startup: config load, hold planning, and VFIO acquisition.

use std::collections::HashMap;
use std::path::Path;

use crate::drm_isolation;
use crate::error::LoadEmberConfigError;
use crate::hold::{self, HeldDevice};
use crate::parse_glowplug_config;
use crate::sysfs::{self, RealSysfs, SysfsPort};
use crate::vendor_lifecycle;
use crate::{EmberConfig, EmberDeviceConfig, find_config};

/// One ordered step for [`apply_hold_actions`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoldAction {
    /// Display or shared GPU: set `driver_override` (no VFIO hold).
    IsolateDisplayGpu {
        /// PCI BDF.
        bdf: String,
    },
    /// Compute GPU: bind IOMMU group, pin power, open VFIO (ember hold).
    OpenVfioForCompute {
        /// PCI BDF.
        bdf: String,
        /// IOMMU group id from sysfs at plan time.
        iommu_group: u32,
    },
}

/// Resolve path, read file, parse TOML, and reject an empty device table.
///
/// # Errors
///
/// Returns [`LoadEmberConfigError`] when the path cannot be resolved, I/O fails, TOML is invalid,
/// or `device` is empty.
pub fn load_ember_config(path: Option<&Path>) -> Result<EmberConfig, LoadEmberConfigError> {
    let config_path = if let Some(p) = path {
        p.to_path_buf()
    } else if let Some(p) = find_config().map(std::path::PathBuf::from) {
        p
    } else {
        return Err(LoadEmberConfigError::NotFound);
    };

    let config_str = std::fs::read_to_string(&config_path)?;
    let config: EmberConfig = parse_glowplug_config(&config_str)?;
    if config.device.is_empty() {
        return Err(LoadEmberConfigError::EmptyDeviceList);
    }
    Ok(config)
}

/// Classify configured devices and produce an ordered hold plan (pure data + sysfs reads for IOMMU group ids).
///
/// Display/shared entries are listed first so [`apply_hold_actions`] can set `driver_override`
/// before DRM isolation and compute VFIO opens.
pub fn plan_device_hold(config: &EmberConfig, sysfs: &dyn SysfsPort) -> Vec<HoldAction> {
    let display_devices: Vec<&EmberDeviceConfig> =
        config.device.iter().filter(|d| d.is_protected()).collect();
    let compute_devices: Vec<&EmberDeviceConfig> =
        config.device.iter().filter(|d| !d.is_protected()).collect();

    let mut actions = Vec::new();
    for d in display_devices {
        actions.push(HoldAction::IsolateDisplayGpu { bdf: d.bdf.clone() });
    }
    for d in compute_devices {
        let iommu_group = sysfs::read_iommu_group_with(sysfs, &d.bdf);
        actions.push(HoldAction::OpenVfioForCompute {
            bdf: d.bdf.clone(),
            iommu_group,
        });
    }
    actions
}

/// Execute a [`plan_device_hold`] result: display isolation, DRM checks, lifecycle prep, and VFIO open.
///
/// # Errors
///
/// Returns an error string when no compute device could be held (same policy as the historical
/// monolithic startup).
pub fn apply_hold_actions(
    config: &EmberConfig,
    actions: &[HoldAction],
    sysfs: &dyn SysfsPort,
) -> Result<HashMap<String, HeldDevice>, String> {
    for action in actions {
        if let HoldAction::IsolateDisplayGpu { bdf } = action {
            tracing::info!(
                bdf = %bdf,
                "display GPU — skipping VFIO hold, setting driver_override"
            );
            sysfs::set_driver_override_with(sysfs, bdf, "nvidia");
        }
    }

    drm_isolation::ensure_drm_isolation(&config.device);

    for action in actions {
        let HoldAction::OpenVfioForCompute { bdf, .. } = action else {
            continue;
        };
        let lifecycle = vendor_lifecycle::detect_lifecycle(bdf);
        let current = sysfs::read_current_driver_with(sysfs, bdf);
        if let Some(ref drv) = current {
            if let Err(e) = lifecycle.prepare_for_unbind(bdf, drv) {
                tracing::warn!(
                    bdf = %bdf, error = %e,
                    "startup: prepare_for_unbind failed (non-fatal)"
                );
            }
        } else {
            sysfs::pin_power_with(sysfs, bdf);
        }
    }

    let mut held_init: HashMap<String, HeldDevice> = HashMap::new();

    for action in actions {
        let HoldAction::OpenVfioForCompute {
            bdf,
            iommu_group: group_id,
        } = action
        else {
            continue;
        };
        tracing::info!(bdf = %bdf, "opening VFIO device for ember hold");

        if *group_id != 0 {
            sysfs::bind_iommu_group_to_vfio_with(sysfs, bdf, *group_id);
        }

        sysfs::pin_power_with(sysfs, bdf);

        match coral_driver::vfio::VfioDevice::open(bdf) {
            Ok(device) => {
                let req_eventfd = crate::arm_req_irq(&device, bdf);
                tracing::info!(
                    bdf = %bdf,
                    backend = ?device.backend_kind(),
                    device_fd = device.device_fd(),
                    num_fds = device.sendable_fds().len(),
                    req_armed = req_eventfd.is_some(),
                    "VFIO device held by ember"
                );
                held_init.insert(
                    bdf.clone(),
                    HeldDevice {
                        bdf: bdf.clone(),
                        device,
                        ring_meta: hold::RingMeta::default(),
                        req_eventfd,
                    },
                );
            }
            Err(e) => {
                tracing::error!(
                    bdf = %bdf,
                    error = %e,
                    "failed to open VFIO device — ember will not hold this device"
                );
            }
        }
    }

    if held_init.is_empty() {
        return Err("no devices held — ember cannot provide fd keepalive".to_string());
    }

    Ok(held_init)
}

/// Convenience: plan then apply using [`RealSysfs`].
pub fn plan_and_apply_hold(config: &EmberConfig) -> Result<HashMap<String, HeldDevice>, String> {
    let sysfs = RealSysfs;
    let actions = plan_device_hold(config, &sysfs);
    apply_hold_actions(config, &actions, &sysfs)
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::Path;

    use super::{HoldAction, load_ember_config, plan_device_hold};
    use crate::error::LoadEmberConfigError;
    use crate::sysfs::MockSysfs;
    use crate::{EmberConfig, parse_glowplug_config};

    #[test]
    fn load_ember_config_valid_toml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("glowplug.toml");
        let mut f = std::fs::File::create(&path).expect("create");
        write!(
            f,
            r#"
[[device]]
bdf = "0000:01:00.0"
"#
        )
        .expect("write");
        let cfg = load_ember_config(Some(path.as_path())).expect("load");
        assert_eq!(cfg.device.len(), 1);
        assert_eq!(cfg.device[0].bdf, "0000:01:00.0");
    }

    #[test]
    fn load_ember_config_invalid_toml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bad.toml");
        std::fs::write(&path, "{{{").expect("write");
        let err = load_ember_config(Some(path.as_path()))
            .err()
            .expect("parse");
        assert!(matches!(err, LoadEmberConfigError::Parse(_)));
    }

    #[test]
    fn load_ember_config_missing_file() {
        let path = Path::new("/nonexistent/ember/glowplug.toml");
        let err = load_ember_config(Some(path)).err().expect("io");
        assert!(matches!(err, LoadEmberConfigError::Read(_)));
    }

    #[test]
    fn load_ember_config_empty_device_list() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("empty.toml");
        std::fs::write(&path, "device = []\n").expect("write");
        let err = load_ember_config(Some(path.as_path()))
            .err()
            .expect("empty");
        assert!(matches!(err, LoadEmberConfigError::EmptyDeviceList));
    }

    #[test]
    fn plan_device_hold_compute_only() {
        let cfg: EmberConfig = parse_glowplug_config(
            r#"
[[device]]
bdf = "0000:02:00.0"
"#,
        )
        .expect("parse");
        let mock = MockSysfs::new().expect("mock sysfs");
        #[cfg(unix)]
        mock.seed_iommu_group("0000:02:00.0", 11)
            .expect("seed iommu");
        let actions = plan_device_hold(&cfg, &mock);
        assert_eq!(actions.len(), 1);
        #[cfg(unix)]
        let expected_group = 11;
        #[cfg(not(unix))]
        let expected_group = 0;
        assert!(matches!(
            &actions[0],
            HoldAction::OpenVfioForCompute { bdf, iommu_group }
                if bdf == "0000:02:00.0" && *iommu_group == expected_group
        ));
    }

    #[test]
    fn plan_device_hold_display_only() {
        let cfg: EmberConfig = parse_glowplug_config(
            r#"
[[device]]
bdf = "0000:01:00.0"
role = "display"
"#,
        )
        .expect("parse");
        let mock = MockSysfs::new().expect("mock sysfs");
        let actions = plan_device_hold(&cfg, &mock);
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            &actions[0],
            HoldAction::IsolateDisplayGpu { bdf } if bdf == "0000:01:00.0"
        ));
    }

    #[test]
    fn plan_device_hold_display_then_compute() {
        let cfg: EmberConfig = parse_glowplug_config(
            r#"
[[device]]
bdf = "0000:01:00.0"
role = "display"

[[device]]
bdf = "0000:02:00.0"
"#,
        )
        .expect("parse");
        let mock = MockSysfs::new().expect("mock sysfs");
        #[cfg(unix)]
        mock.seed_iommu_group("0000:02:00.0", 7)
            .expect("seed iommu");
        let actions = plan_device_hold(&cfg, &mock);
        assert_eq!(actions.len(), 2);
        assert!(matches!(
            &actions[0],
            HoldAction::IsolateDisplayGpu { bdf } if bdf == "0000:01:00.0"
        ));
        #[cfg(unix)]
        let expected_group = 7;
        #[cfg(not(unix))]
        let expected_group = 0;
        assert!(matches!(
            &actions[1],
            HoldAction::OpenVfioForCompute { bdf, iommu_group }
                if bdf == "0000:02:00.0" && *iommu_group == expected_group
        ));
    }
}
