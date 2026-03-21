// SPDX-License-Identifier: AGPL-3.0-only
//! Linux sysfs and procfs layout roots for portable deployments and tests.
//!
//! Environment:
//! - `CORALREEF_SYSFS_ROOT` — sysfs mount (default `/sys`).
//! - `CORALREEF_PROC_ROOT` — procfs mount (default `/proc`).

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
