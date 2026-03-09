// SPDX-License-Identifier: AGPL-3.0-only
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
    eprintln!("nvidia-drm device: {:?}", info);
    assert_eq!(info.driver, "nvidia-drm");
}

#[test]
#[ignore = "requires amdgpu + nvidia GPUs"]
fn multi_gpu_enumerates_both() {
    let nodes = enumerate_render_nodes();
    let has_amd = nodes.iter().any(|n| n.driver == "amdgpu");
    let has_nv = nodes.iter().any(|n| n.driver == "nvidia-drm");
    assert!(has_amd, "expected amdgpu render node");
    assert!(has_nv, "expected nvidia-drm render node");
    eprintln!(
        "found {} render nodes: {:?}",
        nodes.len(),
        nodes.iter().map(|n| n.driver.as_str()).collect::<Vec<_>>()
    );
}
