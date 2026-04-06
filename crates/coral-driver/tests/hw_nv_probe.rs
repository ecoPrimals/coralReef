// SPDX-License-Identifier: AGPL-3.0-or-later
//! NVIDIA GPU probing — DRM render node discovery and device info.
//!
//! Tests probe the NVIDIA DRM render node (renderD129 on this system)
//! and verify that the nvidia-drm proprietary driver is detected.
//!
//! Run: `cargo test --test hw_nv_probe -- --ignored`

use coral_driver::drm::{DrmDevice, enumerate_render_nodes};

#[test]
#[ignore = "requires nvidia GPU with nvidia-drm module"]
fn nvidia_drm_render_node_discovered() {
    let nodes = enumerate_render_nodes();
    eprintln!("discovered render nodes:");
    for info in &nodes {
        eprintln!(
            "  {} — driver='{}', version={}.{}",
            info.path, info.driver, info.version_major, info.version_minor
        );
    }
    let nvidia_node = nodes.iter().find(|n| n.driver == "nvidia-drm");
    assert!(
        nvidia_node.is_some(),
        "expected to find an nvidia-drm render node"
    );
    eprintln!("nvidia-drm node: {}", nvidia_node.unwrap().path);
}

#[test]
#[ignore = "requires nvidia GPU with nvidia-drm module"]
fn nvidia_drm_device_opens_and_queries_driver() {
    let dev = DrmDevice::open_by_driver("nvidia-drm").expect("should find nvidia-drm render node");
    let name = dev.driver_name().expect("driver_name");
    assert_eq!(name, "nvidia-drm");
    let info = dev.device_info().expect("device_info");
    eprintln!("nvidia-drm device: {info:?}");
    assert_eq!(info.driver, "nvidia-drm");
}

#[test]
#[ignore = "requires multiple GPUs"]
fn multi_gpu_enumerates_multiple() {
    let nodes = enumerate_render_nodes();
    eprintln!(
        "found {} render nodes: {:?}",
        nodes.len(),
        nodes.iter().map(|n| n.driver.as_str()).collect::<Vec<_>>()
    );

    // VFIO-bound GPUs don't appear as DRM render nodes. Count them
    // from sysfs so the assertion reflects the true GPU population.
    let vfio_count = count_vfio_gpus();
    let total_gpus = nodes.len() + vfio_count;
    eprintln!(
        "total GPUs: {total_gpus} ({} DRM + {vfio_count} VFIO)",
        nodes.len()
    );

    assert!(
        total_gpus >= 2,
        "expected at least 2 GPUs (DRM + VFIO), found {total_gpus}"
    );
}

fn count_vfio_gpus() -> usize {
    let Ok(entries) = std::fs::read_dir("/sys/bus/pci/drivers/vfio-pci") else {
        return 0;
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let s = name.to_string_lossy();
            // PCI BDF format: DDDD:BB:DD.F
            s.contains(':') && s.contains('.')
        })
        .filter(|e| {
            // Only count VGA/3D class devices (GPUs), not audio companions
            let class_path = e.path().join("class");
            std::fs::read_to_string(class_path)
                .map(|c| c.trim().starts_with("0x03"))
                .unwrap_or(false)
        })
        .count()
}
