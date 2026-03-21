// SPDX-License-Identifier: AGPL-3.0-only
//! Trait-based abstraction over sysfs and related kernel interfaces for testability.
//!
//! Production code uses [`RealSysfs`]; unit tests can inject `MockSysfs` via
//! [`crate::device::DeviceSlot::with_sysfs`].

use crate::device::PowerState;
use coral_driver::linux_paths;

/// Operations that mirror [`crate::sysfs`] free functions for dependency injection.
pub trait SysfsOps: Send + Sync + 'static {
    /// Read PCI vendor and device IDs from sysfs.
    fn read_pci_ids(&self, bdf: &str) -> (u16, u16);

    /// Read the IOMMU group number for a PCI device.
    fn read_iommu_group(&self, bdf: &str) -> u32;

    /// Read the current kernel driver bound to a PCI device.
    fn read_current_driver(&self, bdf: &str) -> Option<String>;

    /// Read a PCI power state from sysfs.
    fn read_power_state(&self, bdf: &str) -> PowerState;

    /// Read PCI link width from sysfs.
    fn read_link_width(&self, bdf: &str) -> Option<u8>;

    /// Find the DRM card device path for a PCI device.
    fn find_drm_card(&self, bdf: &str) -> Option<String>;

    /// Whether any non-self process holds an open fd to this device's DRM nodes.
    fn has_active_drm_consumers(&self, bdf: &str) -> bool;

    /// Write to a sysfs path (same semantics as [`crate::sysfs::sysfs_write`]).
    fn sysfs_write(&self, path: &str, data: &str) -> Result<(), String>;

    /// Ensure peers in the IOMMU group are bound to `vfio-pci`.
    fn bind_iommu_group_to_vfio(&self, primary_bdf: &str, group_id: u32);
}

/// Production implementation: delegates to [`crate::sysfs`] helpers.
#[derive(Clone, Copy, Debug, Default)]
pub struct RealSysfs;

impl SysfsOps for RealSysfs {
    fn read_pci_ids(&self, bdf: &str) -> (u16, u16) {
        crate::sysfs::read_pci_ids(bdf)
    }

    fn read_iommu_group(&self, bdf: &str) -> u32 {
        crate::sysfs::read_iommu_group(bdf)
    }

    fn read_current_driver(&self, bdf: &str) -> Option<String> {
        crate::sysfs::read_current_driver(bdf)
    }

    fn read_power_state(&self, bdf: &str) -> PowerState {
        crate::sysfs::read_power_state(bdf)
    }

    fn read_link_width(&self, bdf: &str) -> Option<u8> {
        crate::sysfs::read_link_width(bdf)
    }

    fn find_drm_card(&self, bdf: &str) -> Option<String> {
        crate::sysfs::find_drm_card(bdf)
    }

    fn has_active_drm_consumers(&self, bdf: &str) -> bool {
        crate::sysfs::has_active_drm_consumers(bdf)
    }

    fn sysfs_write(&self, path: &str, data: &str) -> Result<(), String> {
        crate::sysfs::sysfs_write(path, data).map_err(|e| e.to_string())
    }

    fn bind_iommu_group_to_vfio(&self, primary_bdf: &str, group_id: u32) {
        bind_iommu_group_to_vfio_with(self, primary_bdf, group_id);
    }
}

/// Shared implementation used by [`SysfsOps::bind_iommu_group_to_vfio`] and
/// [`crate::sysfs::bind_iommu_group_to_vfio`].
pub(crate) fn bind_iommu_group_to_vfio_with<S: SysfsOps>(
    ops: &S,
    primary_bdf: &str,
    group_id: u32,
) {
    let group_path = linux_paths::sysfs_kernel_iommu_group_devices(group_id);
    let Ok(entries) = std::fs::read_dir(&group_path) else {
        return;
    };

    for entry in entries.flatten() {
        let peer_bdf = entry.file_name().to_string_lossy().to_string();
        if peer_bdf == primary_bdf {
            continue;
        }

        let driver = ops.read_current_driver(&peer_bdf);
        if driver.as_deref() == Some("vfio-pci") {
            continue;
        }

        tracing::info!(
            peer = %peer_bdf,
            driver = driver.as_deref().unwrap_or("none"),
            group = group_id,
            "binding IOMMU group peer to vfio-pci"
        );

        if driver.is_some() {
            let _ = ops.sysfs_write(
                &linux_paths::sysfs_pci_device_file(&peer_bdf, "driver/unbind"),
                &peer_bdf,
            );
            std::thread::sleep(std::time::Duration::from_millis(200));
        }

        let _ = ops.sysfs_write(
            &linux_paths::sysfs_pci_device_file(&peer_bdf, "driver_override"),
            "vfio-pci",
        );
        let _ = ops.sysfs_write(&linux_paths::sysfs_pci_driver_bind("vfio-pci"), &peer_bdf);
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

#[cfg(test)]
mod mock {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    /// Configurable sysfs double for unit tests (no `/sys` access required).
    #[derive(Debug)]
    pub struct MockSysfs {
        /// Per-BDF PCI vendor/device IDs.
        pub pci_ids: HashMap<String, (u16, u16)>,
        /// Per-BDF IOMMU group id.
        pub iommu_group: HashMap<String, u32>,
        /// Per-BDF bound driver name (`None` = no driver symlink).
        pub current_driver: HashMap<String, Option<String>>,
        /// Per-BDF power state.
        pub power_state: HashMap<String, PowerState>,
        /// Per-BDF link width.
        pub link_width: HashMap<String, Option<u8>>,
        /// Per-BDF DRM card path.
        pub drm_card: HashMap<String, Option<String>>,
        /// Per-BDF whether DRM consumers are reported active.
        pub drm_consumers: HashMap<String, bool>,
        /// Recorded sysfs writes `(path, data)` for assertions.
        pub writes: Mutex<Vec<(String, String)>>,
        /// Count [`SysfsOps::bind_iommu_group_to_vfio`] invocations.
        pub bind_iommu_calls: AtomicU32,
    }

    impl Default for MockSysfs {
        fn default() -> Self {
            Self {
                pci_ids: HashMap::new(),
                iommu_group: HashMap::new(),
                current_driver: HashMap::new(),
                power_state: HashMap::new(),
                link_width: HashMap::new(),
                drm_card: HashMap::new(),
                drm_consumers: HashMap::new(),
                writes: Mutex::new(Vec::new()),
                bind_iommu_calls: AtomicU32::new(0),
            }
        }
    }

    impl MockSysfs {
        /// Insert defaults for a BDF so lookups succeed without filling every map.
        pub fn seed_bdf(&mut self, bdf: &str) {
            self.pci_ids
                .entry(bdf.to_string())
                .or_insert((0x10de, 0x1d81));
            self.iommu_group.entry(bdf.to_string()).or_insert(1);
            self.current_driver
                .entry(bdf.to_string())
                .or_insert(Some("vfio-pci".to_string()));
            self.power_state
                .entry(bdf.to_string())
                .or_insert(PowerState::Unknown);
            self.link_width.entry(bdf.to_string()).or_insert(None);
            self.drm_card.entry(bdf.to_string()).or_insert(None);
            self.drm_consumers.entry(bdf.to_string()).or_insert(false);
        }
    }

    impl SysfsOps for MockSysfs {
        fn read_pci_ids(&self, bdf: &str) -> (u16, u16) {
            self.pci_ids.get(bdf).copied().unwrap_or((0, 0))
        }

        fn read_iommu_group(&self, bdf: &str) -> u32 {
            self.iommu_group.get(bdf).copied().unwrap_or(0)
        }

        fn read_current_driver(&self, bdf: &str) -> Option<String> {
            self.current_driver.get(bdf).cloned().unwrap_or(None)
        }

        fn read_power_state(&self, bdf: &str) -> PowerState {
            self.power_state
                .get(bdf)
                .copied()
                .unwrap_or(PowerState::Unknown)
        }

        fn read_link_width(&self, bdf: &str) -> Option<u8> {
            self.link_width.get(bdf).copied().flatten()
        }

        fn find_drm_card(&self, bdf: &str) -> Option<String> {
            self.drm_card.get(bdf).and_then(Clone::clone)
        }

        fn has_active_drm_consumers(&self, bdf: &str) -> bool {
            self.drm_consumers.get(bdf).copied().unwrap_or(false)
        }

        fn sysfs_write(&self, path: &str, data: &str) -> Result<(), String> {
            self.writes
                .lock()
                .map_err(|e| format!("mock writes mutex poisoned: {e}"))?
                .push((path.to_string(), data.to_string()));
            Ok(())
        }

        fn bind_iommu_group_to_vfio(&self, _primary_bdf: &str, _group_id: u32) {
            self.bind_iommu_calls.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
pub use mock::MockSysfs;

#[cfg(test)]
mod sysfs_ops_tests {
    use std::sync::atomic::Ordering;

    use crate::device::PowerState;

    use super::MockSysfs;
    use super::SysfsOps;
    use super::bind_iommu_group_to_vfio_with;

    #[test]
    fn mock_bind_iommu_group_to_vfio_increments_counter() {
        let m = MockSysfs::default();
        m.bind_iommu_group_to_vfio("0000:01:00.0", 42);
        assert_eq!(m.bind_iommu_calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn bind_iommu_group_to_vfio_with_mock_no_sysfs_group_is_noop() {
        let m = MockSysfs::default();
        bind_iommu_group_to_vfio_with(&m, "0000:01:00.0", 999_999);
        assert_eq!(m.bind_iommu_calls.load(Ordering::Relaxed), 0);
        assert!(m.writes.lock().expect("writes").is_empty());
    }

    #[test]
    fn mock_sysfs_write_records_paths() {
        let m = MockSysfs::default();
        m.sysfs_write("/tmp/coral-mock-test", "x").expect("write");
        let w = m.writes.lock().expect("writes");
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].0, "/tmp/coral-mock-test");
        assert_eq!(w[0].1, "x");
    }

    #[test]
    fn mock_bogus_bdf_returns_pci_id_defaults() {
        let m = MockSysfs::default();
        assert_eq!(m.read_pci_ids("not-a-bdf"), (0, 0));
        assert_eq!(m.read_iommu_group("bogus"), 0);
        assert_eq!(m.read_current_driver("bogus"), None);
        assert_eq!(m.read_power_state("bogus"), PowerState::Unknown);
        assert_eq!(m.read_link_width("bogus"), None);
        assert_eq!(m.find_drm_card("bogus"), None);
        assert!(!m.has_active_drm_consumers("bogus"));
    }
}
