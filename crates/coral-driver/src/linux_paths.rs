// SPDX-License-Identifier: AGPL-3.0-only
//! Linux sysfs and procfs layout roots for portable deployments and tests.
//!
//! Environment:
//! - `CORALREEF_SYSFS_ROOT` — sysfs mount (default `/sys`).
//! - `CORALREEF_PROC_ROOT` — procfs mount (default `/proc`).
//! - `CORALREEF_DATA_DIR` — optional data directory for dumps and training assets
//!   (falls back to `HOTSPRING_DATA_DIR` for backward compatibility).

use std::sync::OnceLock;

fn sysfs_root_storage() -> &'static str {
    static ROOT: OnceLock<String> = OnceLock::new();
    ROOT.get_or_init(|| {
        std::env::var("CORALREEF_SYSFS_ROOT")
            .ok()
            .filter(|s| !s.is_empty())
            .map(|s| s.trim_end_matches('/').to_string())
            .unwrap_or_else(|| "/sys".to_string())
    })
    .as_str()
}

fn proc_root_storage() -> &'static str {
    static ROOT: OnceLock<String> = OnceLock::new();
    ROOT.get_or_init(|| {
        std::env::var("CORALREEF_PROC_ROOT")
            .ok()
            .filter(|s| !s.is_empty())
            .map(|s| s.trim_end_matches('/').to_string())
            .unwrap_or_else(|| "/proc".to_string())
    })
    .as_str()
}

/// Resolved sysfs mount path (`$CORALREEF_SYSFS_ROOT`, default `/sys`).
#[must_use]
pub fn sysfs_root() -> &'static str {
    sysfs_root_storage()
}

/// Resolved procfs mount path (`$CORALREEF_PROC_ROOT`, default `/proc`).
#[must_use]
pub fn proc_root() -> &'static str {
    proc_root_storage()
}

/// Optional data directory for VBIOS dumps and similar assets.
///
/// Reads `$CORALREEF_DATA_DIR`, then `$HOTSPRING_DATA_DIR` if unset (legacy).
#[must_use]
pub fn optional_data_dir() -> Option<String> {
    std::env::var("CORALREEF_DATA_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("HOTSPRING_DATA_DIR")
                .ok()
                .filter(|s| !s.is_empty())
        })
}

/// Join path segments under [`sysfs_root`].
#[must_use]
pub fn sysfs_join(parts: &[&str]) -> String {
    let mut s = String::with_capacity(96);
    s.push_str(sysfs_root());
    for p in parts {
        s.push('/');
        s.push_str(p.trim_matches('/'));
    }
    s
}

/// `/…/bus/pci/devices` under [`sysfs_root`].
#[must_use]
pub fn sysfs_pci_devices() -> String {
    sysfs_join(&["bus", "pci", "devices"])
}

/// `/…/bus/pci/devices/{bdf}` under [`sysfs_root`].
#[must_use]
pub fn sysfs_pci_device_path(bdf: &str) -> String {
    sysfs_join(&["bus", "pci", "devices", bdf])
}

/// `/…/bus/pci/devices/{bdf}/{tail}` (e.g. `tail` = `config`, `power/control`).
#[must_use]
pub fn sysfs_pci_device_file(bdf: &str, tail: &str) -> String {
    let base = sysfs_pci_device_path(bdf);
    if tail.is_empty() {
        base
    } else {
        format!("{base}/{}", tail.trim_start_matches('/'))
    }
}

/// `/…/bus/pci/drivers/{driver}/bind` under [`sysfs_root`].
#[must_use]
pub fn sysfs_pci_driver_bind(driver: &str) -> String {
    sysfs_join(&["bus", "pci", "drivers", driver, "bind"])
}

/// `/…/bus/pci/drivers/{driver}/unbind` under [`sysfs_root`].
#[must_use]
pub fn sysfs_pci_driver_unbind(driver: &str) -> String {
    sysfs_join(&["bus", "pci", "drivers", driver, "unbind"])
}

/// `/…/bus/pci/rescan` under [`sysfs_root`].
#[must_use]
pub fn sysfs_pci_bus_rescan() -> String {
    sysfs_join(&["bus", "pci", "rescan"])
}

/// `/…/bus/pci/drivers_autoprobe` under [`sysfs_root`].
///
/// Writing `0` disables automatic driver probing on bus rescan;
/// writing `1` re-enables it. Used during targeted PCI remove+rescan
/// to prevent the kernel from matching `vfio-pci.ids` before ember
/// can set `driver_override`.
#[must_use]
pub fn sysfs_pci_drivers_autoprobe() -> String {
    sysfs_join(&["bus", "pci", "drivers_autoprobe"])
}

/// `/…/bus/pci/drivers_probe` under [`sysfs_root`].
///
/// Writing a BDF triggers the kernel's driver matching for that device,
/// honoring `driver_override` if set. Used after disabling autoprobe
/// and setting the desired override.
#[must_use]
pub fn sysfs_pci_drivers_probe() -> String {
    sysfs_join(&["bus", "pci", "drivers_probe"])
}

/// `/…/module/{name}` under [`sysfs_root`] (e.g. `nvidia` for the proprietary stack).
#[must_use]
pub fn sysfs_module_path(name: &str) -> String {
    sysfs_join(&["module", name])
}

/// `/…/class/drm/{node}/device` under [`sysfs_root`] (e.g. `renderD128`).
#[must_use]
pub fn sysfs_class_drm_device(node_name: &str) -> String {
    sysfs_join(&["class", "drm", node_name, "device"])
}

/// `/…/kernel/iommu_groups/{group_id}/devices` under [`sysfs_root`].
#[must_use]
pub fn sysfs_kernel_iommu_group_devices(group_id: u32) -> String {
    let gid = group_id.to_string();
    sysfs_join(&["kernel", "iommu_groups", &gid, "devices"])
}

/// `{proc_root()}/{pid}/fd`.
#[must_use]
pub fn proc_pid_fd_dir(pid: u32) -> String {
    format!("{}/{pid}/fd", proc_root())
}

/// `{proc_root()}/self/fd/{fd}` (Linux open-fd directory entries).
#[must_use]
pub fn proc_self_fd(fd: i32) -> String {
    format!("{}/self/fd/{fd}", proc_root())
}

/// `{proc_root()}/cmdline` (kernel boot command line).
#[must_use]
pub fn proc_cmdline() -> String {
    format!("{}/cmdline", proc_root())
}

/// Discover the VFIO cdev name for a PCI device (kernel 6.2+).
///
/// Reads the first entry under `/sys/bus/pci/devices/{bdf}/vfio-dev/`, which
/// the kernel populates when vfio-pci binds via the cdev path (e.g. `"vfio0"`).
/// Returns `None` if the directory doesn't exist or is empty (older kernels or
/// device not bound to vfio-pci).
#[must_use]
pub fn sysfs_vfio_cdev_name(bdf: &str) -> Option<String> {
    let dir = sysfs_pci_device_file(bdf, "vfio-dev");
    std::fs::read_dir(dir)
        .ok()?
        .next()?
        .ok()?
        .file_name()
        .into_string()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static OPTIONAL_DATA_DIR_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn sysfs_join_builds_path_under_root() {
        let root = sysfs_root();
        let path = sysfs_join(&["bus", "pci", "devices"]);
        assert!(
            path.starts_with(root),
            "path should start with sysfs root {root:?}, got {path:?}"
        );
        assert!(path.ends_with("/bus/pci/devices"));
    }

    #[test]
    fn sysfs_join_trims_slashes_in_segments() {
        let path = sysfs_join(&["/class/drm/", "renderD128"]);
        assert!(path.contains("/class/drm/renderD128"));
    }

    #[test]
    fn sysfs_pci_devices_matches_join() {
        assert_eq!(sysfs_pci_devices(), sysfs_join(&["bus", "pci", "devices"]));
    }

    #[test]
    fn sysfs_pci_device_path_various_bdfs() {
        for bdf in ["0000:03:00.0", "0000:0a:00.1", "0000:41:00.0"] {
            let p = sysfs_pci_device_path(bdf);
            assert!(
                p.ends_with(&format!("/bus/pci/devices/{bdf}")),
                "unexpected path for {bdf}: {p}"
            );
        }
    }

    #[test]
    fn sysfs_pci_device_file_appends_tail() {
        let bdf = "0000:01:00.0";
        let config = sysfs_pci_device_file(bdf, "config");
        assert!(config.ends_with("/config"));
        assert!(config.contains(&format!("devices/{bdf}")));

        let power = sysfs_pci_device_file(bdf, "power/control");
        assert!(power.ends_with("power/control"));

        let leading = sysfs_pci_device_file(bdf, "/uevent");
        assert!(leading.ends_with("/uevent"));
    }

    #[test]
    fn sysfs_pci_device_file_empty_tail_is_base_only() {
        let bdf = "0000:02:00.0";
        assert_eq!(sysfs_pci_device_file(bdf, ""), sysfs_pci_device_path(bdf));
    }

    #[test]
    fn sysfs_pci_driver_bind_unbind() {
        let bind = sysfs_pci_driver_bind("vfio-pci");
        assert!(bind.ends_with("/vfio-pci/bind"));
        let unbind = sysfs_pci_driver_unbind("vfio-pci");
        assert!(unbind.ends_with("/vfio-pci/unbind"));
    }

    #[test]
    fn sysfs_pci_bus_rescan_path() {
        let p = sysfs_pci_bus_rescan();
        assert!(p.ends_with("/bus/pci/rescan"));
    }

    #[test]
    fn sysfs_pci_drivers_autoprobe_path() {
        let p = sysfs_pci_drivers_autoprobe();
        assert!(p.ends_with("/bus/pci/drivers_autoprobe"));
    }

    #[test]
    fn sysfs_pci_drivers_probe_path() {
        let p = sysfs_pci_drivers_probe();
        assert!(p.ends_with("/bus/pci/drivers_probe"));
    }

    #[test]
    fn sysfs_module_path_nvidia() {
        let p = sysfs_module_path("nvidia");
        assert!(p.ends_with("/module/nvidia"));
    }

    #[test]
    fn sysfs_class_drm_device_card0() {
        let p = sysfs_class_drm_device("card0");
        assert!(p.ends_with("/class/drm/card0/device"));
    }

    #[test]
    fn sysfs_kernel_iommu_group_42() {
        let p = sysfs_kernel_iommu_group_devices(42);
        assert!(p.ends_with("/kernel/iommu_groups/42/devices"));
    }

    #[test]
    fn proc_pid_fd_dir_and_self_fd() {
        let fd_dir = proc_pid_fd_dir(1234);
        assert!(fd_dir.ends_with("/1234/fd"));
        assert!(fd_dir.starts_with(proc_root()));

        let self_fd = proc_self_fd(7);
        assert!(self_fd.ends_with("/self/fd/7"));
        assert!(self_fd.starts_with(proc_root()));
    }

    #[test]
    fn proc_cmdline_path() {
        let c = proc_cmdline();
        assert!(c.ends_with("/cmdline"));
        assert!(c.starts_with(proc_root()));
    }

    #[test]
    fn optional_data_dir_prefers_coralreef_env() {
        let _guard = OPTIONAL_DATA_DIR_TEST_LOCK.lock().expect("lock");
        // SAFETY: `OPTIONAL_DATA_DIR_TEST_LOCK` serializes tests that touch these
        // process environment variables; no concurrent readers elsewhere in tests.
        unsafe {
            std::env::remove_var("CORALREEF_DATA_DIR");
            std::env::remove_var("HOTSPRING_DATA_DIR");
        }
        assert!(optional_data_dir().is_none());

        // SAFETY: Same mutex and test-only env contract as above.
        unsafe {
            std::env::set_var("CORALREEF_DATA_DIR", "/var/coral");
        }
        assert_eq!(optional_data_dir().as_deref(), Some("/var/coral"));

        // SAFETY: Same mutex and test-only env contract as above.
        unsafe {
            std::env::set_var("CORALREEF_DATA_DIR", "");
            std::env::set_var("HOTSPRING_DATA_DIR", "/var/hot");
        }
        assert_eq!(optional_data_dir().as_deref(), Some("/var/hot"));

        // SAFETY: Same mutex and test-only env contract as above.
        unsafe {
            std::env::remove_var("CORALREEF_DATA_DIR");
            std::env::remove_var("HOTSPRING_DATA_DIR");
        }
    }

    #[test]
    fn optional_data_dir_empty_hot_spring_ignored() {
        let _guard = OPTIONAL_DATA_DIR_TEST_LOCK.lock().expect("lock");
        // SAFETY: test-only env mutation under a mutex; no concurrent reads
        // of these env vars in this process outside guarded tests.
        unsafe {
            std::env::remove_var("CORALREEF_DATA_DIR");
            std::env::set_var("HOTSPRING_DATA_DIR", "");
        }
        assert!(optional_data_dir().is_none());
        // SAFETY: same guard; restoring env to clean state.
        unsafe {
            std::env::remove_var("HOTSPRING_DATA_DIR");
        }
    }

    #[test]
    fn sysfs_join_empty_segment_trims_to_root_only_extra_slash() {
        let path = sysfs_join(&["", "bus", "pci"]);
        assert!(path.contains("/bus/pci"));
        assert!(path.starts_with(sysfs_root()));
    }

    #[test]
    fn sysfs_join_single_segment() {
        let path = sysfs_join(&["kernel"]);
        assert_eq!(path, format!("{}/kernel", sysfs_root()));
    }

    #[test]
    fn sysfs_pci_device_file_tail_only_slash_strips_to_relative_segment() {
        let bdf = "0000:05:00.0";
        let p = sysfs_pci_device_file(bdf, "/config");
        assert!(p.ends_with("/config"));
    }

    #[test]
    fn optional_data_dir_whitespace_only_not_treated_as_unset() {
        let _guard = OPTIONAL_DATA_DIR_TEST_LOCK.lock().expect("lock");
        // SAFETY: `OPTIONAL_DATA_DIR_TEST_LOCK` serializes tests that touch these
        // process environment variables; no concurrent readers elsewhere in tests.
        unsafe {
            std::env::set_var("CORALREEF_DATA_DIR", "   ");
            std::env::remove_var("HOTSPRING_DATA_DIR");
        }
        assert_eq!(optional_data_dir().as_deref(), Some("   "));
        // SAFETY: Same mutex and test-only env contract as above.
        unsafe {
            std::env::remove_var("CORALREEF_DATA_DIR");
        }
    }
}
