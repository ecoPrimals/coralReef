// SPDX-License-Identifier: AGPL-3.0-only
//! Shared test helpers for IPC module tests.

use tokio::sync::watch;

pub fn test_shutdown_channel() -> (watch::Sender<()>, watch::Receiver<()>) {
    watch::channel(())
}

/// Generate valid SPIR-V for a minimal compute shader via naga (WGSL → SPIR-V).
pub fn valid_spirv_minimal_compute() -> Vec<u32> {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let module = naga::front::wgsl::parse_str(wgsl).expect("WGSL should parse");
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::default(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .expect("module should validate");
    naga::back::spv::write_vec(&module, &info, &naga::back::spv::Options::default(), None)
        .expect("SPIR-V write should succeed")
}
