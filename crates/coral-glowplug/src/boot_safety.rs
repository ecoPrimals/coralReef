// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pure boot-safety evaluation — inputs are collected via sysfs/`/proc` elsewhere.

use crate::pci_ids;

/// Snapshot of kernel and PCI driver state used for Titan V / VFIO safety checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootSafetyInputs {
    /// Contents of `/proc/cmdline`.
    pub cmdline: String,
    /// Module names from `/proc/modules` (first column), newest-first order preserved.
    pub modules: Vec<String>,
    /// `(bdf, driver_name)` for each managed compute device (e.g. from `driver` symlink).
    pub device_drivers: Vec<(String, String)>,
    /// `(bdf, driver_override)` trimmed text from sysfs `driver_override`.
    pub device_driver_overrides: Vec<(String, String)>,
    /// `true` when `/sys/module/nvidia` exists (proprietary stack loaded).
    pub nvidia_kmod_loaded: bool,
}

/// Actionable or informational boot-safety outcomes (logged by the daemon).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootWarning {
    /// Kernel cmdline is missing `vfio-pci.ids` (Titan V risk).
    MissingVfioPciIdsInCmdline,
    /// `nvidia.ko` is bound to a managed compute BDF.
    NvidiaBoundToManagedDevice {
        /// PCI BDF.
        bdf: String,
    },
    /// `nvidia` is loaded and at least one managed device lacks `driver_override=vfio-pci`.
    NvidiaLoadedWithoutFullVfioOverride,
    /// `vfio-pci.ids` present in cmdline and every managed device uses `vfio-pci`.
    BootSafetyOk,
    /// All managed devices on `vfio-pci` but cmdline lacks the explicit ids parameter.
    AllOnVfioRecommendCmdlineParam,
    /// Informational: proprietary `nvidia` module is present (display stack).
    NvidiaModuleLoadedInfo,
}

/// Evaluate boot safety from pre-collected inputs (no I/O).
#[must_use]
pub fn evaluate_boot_safety(inputs: &BootSafetyInputs) -> Vec<BootWarning> {
    let mut out = Vec::new();

    if !inputs.cmdline.contains("vfio-pci.ids") {
        out.push(BootWarning::MissingVfioPciIdsInCmdline);
    }

    if inputs.nvidia_kmod_loaded {
        for (bdf, driver) in &inputs.device_drivers {
            if driver == "nvidia" {
                out.push(BootWarning::NvidiaBoundToManagedDevice { bdf: bdf.clone() });
            }
        }

        let nvidia_probed_managed = inputs
            .device_driver_overrides
            .iter()
            .any(|(_, o)| o.trim() != "vfio-pci");

        if nvidia_probed_managed {
            out.push(BootWarning::NvidiaLoadedWithoutFullVfioOverride);
        }
    }

    let vfio_ids_in_cmdline = inputs.cmdline.contains(pci_ids::TITAN_V_VFIO_IDS_CMDLINE)
        || inputs
            .cmdline
            .contains(pci_ids::TITAN_V_VFIO_IDS_CMDLINE_ALT);

    let all_on_vfio = inputs.device_drivers.iter().all(|(_, d)| d == "vfio-pci");

    if vfio_ids_in_cmdline && all_on_vfio && !inputs.device_drivers.is_empty() {
        out.push(BootWarning::BootSafetyOk);
    } else if all_on_vfio && !inputs.device_drivers.is_empty() {
        out.push(BootWarning::AllOnVfioRecommendCmdlineParam);
    }

    if inputs.nvidia_kmod_loaded {
        out.push(BootWarning::NvidiaModuleLoadedInfo);
    }

    let _ = &inputs.modules;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_inputs() -> BootSafetyInputs {
        BootSafetyInputs {
            cmdline: format!("quiet {}", pci_ids::TITAN_V_VFIO_IDS_CMDLINE),
            modules: vec!["nvidia".into(), "vfio_pci".into()],
            device_drivers: vec![("0000:01:00.0".into(), "vfio-pci".into())],
            device_driver_overrides: vec![("0000:01:00.0".into(), "vfio-pci".into())],
            nvidia_kmod_loaded: false,
        }
    }

    #[test]
    fn evaluate_ok_when_cmdline_and_vfio() {
        let w = evaluate_boot_safety(&sample_inputs());
        assert!(w.contains(&BootWarning::BootSafetyOk));
        assert!(!w.contains(&BootWarning::MissingVfioPciIdsInCmdline));
    }

    #[test]
    fn evaluate_warns_missing_vfio_ids_token() {
        let mut i = sample_inputs();
        i.cmdline = "quiet splash".into();
        let w = evaluate_boot_safety(&i);
        assert!(w.contains(&BootWarning::MissingVfioPciIdsInCmdline));
    }

    #[test]
    fn evaluate_flags_nvidia_on_managed_bdf() {
        let mut i = sample_inputs();
        i.nvidia_kmod_loaded = true;
        i.device_drivers = vec![("0000:01:00.0".into(), "nvidia".into())];
        let w = evaluate_boot_safety(&i);
        assert!(w.iter().any(|x| matches!(
            x,
            BootWarning::NvidiaBoundToManagedDevice { bdf } if bdf == "0000:01:00.0"
        )));
    }

    #[test]
    fn evaluate_warns_override_when_nvidia_loaded() {
        let mut i = sample_inputs();
        i.nvidia_kmod_loaded = true;
        i.device_drivers = vec![("0000:01:00.0".into(), "vfio-pci".into())];
        i.device_driver_overrides = vec![("0000:01:00.0".into(), "".into())];
        let w = evaluate_boot_safety(&i);
        assert!(w.contains(&BootWarning::NvidiaLoadedWithoutFullVfioOverride));
    }

    #[test]
    fn evaluate_recommends_cmdline_when_vfio_but_no_ids_param() {
        let mut i = sample_inputs();
        i.cmdline = "quiet".into();
        i.device_drivers = vec![("0000:01:00.0".into(), "vfio-pci".into())];
        let w = evaluate_boot_safety(&i);
        assert!(w.contains(&BootWarning::AllOnVfioRecommendCmdlineParam));
    }

    #[test]
    fn evaluate_multiple_warnings_when_iommu_and_nvidia_and_override() {
        let i = BootSafetyInputs {
            cmdline: "quiet splash".into(),
            modules: vec!["nvidia".into(), "vfio_pci".into()],
            device_drivers: vec![("0000:01:00.0".into(), "nvidia".into())],
            device_driver_overrides: vec![("0000:01:00.0".into(), "".into())],
            nvidia_kmod_loaded: true,
        };
        let w = evaluate_boot_safety(&i);
        assert!(w.contains(&BootWarning::MissingVfioPciIdsInCmdline));
        assert!(w.iter().any(|x| matches!(
            x,
            BootWarning::NvidiaBoundToManagedDevice { bdf } if bdf == "0000:01:00.0"
        )));
        assert!(w.contains(&BootWarning::NvidiaLoadedWithoutFullVfioOverride));
        assert!(w.contains(&BootWarning::NvidiaModuleLoadedInfo));
    }

    #[test]
    fn evaluate_clean_boot_no_problem_warnings() {
        let w = evaluate_boot_safety(&sample_inputs());
        assert!(!w.contains(&BootWarning::MissingVfioPciIdsInCmdline));
        assert!(!w.contains(&BootWarning::NvidiaLoadedWithoutFullVfioOverride));
        assert!(
            !w.iter()
                .any(|x| matches!(x, BootWarning::NvidiaBoundToManagedDevice { .. }))
        );
        assert!(w.contains(&BootWarning::BootSafetyOk));
    }
}
