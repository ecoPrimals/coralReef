// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals

//! Integration tests: discover any local GPU and exercise compile + sysfs identity.
//! Each test returns early (skip) when no suitable hardware is present — never fail for missing GPU.

use coral_gpu::GpuContext;

fn discover_local_gpu() -> Option<GpuContext> {
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("No local GPU: hardware discovery requires Linux");
        return None;
    }
    #[cfg(target_os = "linux")]
    match GpuContext::auto() {
        Ok(ctx) if ctx.has_device() => {
            eprintln!(
                "Discovered GPU: {} {}",
                ctx.target().vendor(),
                ctx.target().arch_name()
            );
            Some(ctx)
        }
        Ok(_) => {
            eprintln!("No local GPU available: compile-only context (no device attached)");
            None
        }
        Err(e) => {
            eprintln!("No local GPU available: {e}");
            None
        }
    }
}

#[test]
fn discover_any_gpu() {
    let Some(ctx) = discover_local_gpu() else {
        eprintln!("SKIPPED: no GPU available");
        return;
    };
    let target = ctx.target();
    eprintln!(
        "target vendor={} arch={}",
        target.vendor(),
        target.arch_name()
    );
    assert!(!target.vendor().is_empty());
    assert!(!target.arch_name().is_empty());
}

#[test]
fn enumerate_all_gpus() {
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("SKIPPED: GpuContext::enumerate_all is Linux-only");
        return;
    }
    #[cfg(target_os = "linux")]
    {
        let results = GpuContext::enumerate_all();
        eprintln!("enumerate_all: {} candidate(s)", results.len());
        for (i, res) in results.iter().enumerate() {
            match res {
                Ok(ctx) => {
                    eprintln!(
                        "  [{i}] OK: {} {}",
                        ctx.target().vendor(),
                        ctx.target().arch_name()
                    );
                }
                Err(e) => {
                    eprintln!("  [{i}] ERR: {e}");
                }
            }
        }
    }
}

#[test]
fn compile_for_local_gpu() {
    let Some(ctx) = discover_local_gpu() else {
        eprintln!("SKIPPED: no GPU available");
        return;
    };
    const WGSL: &str = r"
@compute @workgroup_size(1)
fn main() {}
";
    let kernel = match ctx.compile_wgsl(WGSL) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("SKIPPED: compile failed: {e}");
            return;
        }
    };
    assert!(
        !kernel.binary.is_empty(),
        "compiled kernel binary should be non-empty"
    );
    eprintln!(
        "compiled {} bytes for {}",
        kernel.binary.len(),
        ctx.target().arch_name()
    );
}

#[test]
fn gpu_identity_report() {
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("SKIPPED: sysfs GPU identity is Linux-only");
        return;
    }
    #[cfg(target_os = "linux")]
    {
        use coral_driver::drm::enumerate_render_nodes;
        use coral_driver::nv::identity::probe_gpu_identity;

        let nodes = enumerate_render_nodes();
        if nodes.is_empty() {
            eprintln!("SKIPPED: no DRM render nodes");
            return;
        }

        let mut probed = 0usize;
        for info in &nodes {
            let Some(id) = probe_gpu_identity(&info.path) else {
                eprintln!(
                    "{} (driver={}): sysfs identity unavailable",
                    info.path, info.driver
                );
                continue;
            };
            probed += 1;
            let nvidia_sm = id.nvidia_sm();
            let amd_arch = id.amd_arch();
            eprintln!(
                "{} driver={} vendor={:#06x} device={:#06x} sysfs={} nvidia_sm={nvidia_sm:?} amd_arch={amd_arch:?}",
                info.path, info.driver, id.vendor_id, id.device_id, id.sysfs_path
            );
        }

        if probed == 0 {
            eprintln!("SKIPPED: could not probe sysfs identity for any render node");
        }
    }
}
