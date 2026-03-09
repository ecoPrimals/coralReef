// SPDX-License-Identifier: AGPL-3.0-only
//! NVIDIA DRM device tests — probe, compile, and dispatch readiness.
//!
//! The nvidia-drm render node currently supports device probing and
//! identification. Buffer management and compute dispatch require
//! NVIDIA UVM integration (tracked for future evolution).
//!
//! Run: `cargo test --test hw_nv_buffers --features nvidia-drm -- --ignored`

#[cfg(feature = "nvidia-drm")]
mod tests {
    use coral_driver::nv::NvDrmDevice;
    use coral_driver::{ComputeDevice, DispatchDims, ShaderInfo};

    fn open_nv() -> NvDrmDevice {
        NvDrmDevice::open().expect("NvDrmDevice::open() failed — is nvidia-drm loaded?")
    }

    #[test]
    #[ignore = "requires nvidia-drm hardware"]
    fn device_opens_successfully() {
        let dev = open_nv();
        assert!(dev.path().contains("renderD"));
        let name = dev.driver_name().expect("driver_name");
        assert_eq!(name, "nvidia-drm");
    }

    #[test]
    #[ignore = "requires nvidia-drm hardware"]
    fn alloc_returns_pending_uvm_error() {
        let mut dev = open_nv();
        let result = dev.alloc(4096, coral_driver::MemoryDomain::Gtt);
        assert!(result.is_err(), "alloc should fail pending UVM");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("UVM"), "error should mention UVM: {err}");
    }

    #[test]
    #[ignore = "requires nvidia-drm hardware"]
    fn dispatch_returns_pending_uvm_error() {
        let mut dev = open_nv();
        let result = dev.dispatch(
            &[0xBF, 0x81, 0x00, 0x00],
            &[],
            DispatchDims::linear(1),
            &ShaderInfo::default(),
        );
        assert!(result.is_err(), "dispatch should fail pending UVM");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("UVM"), "error should mention UVM: {err}");
    }

    #[test]
    #[ignore = "requires nvidia-drm hardware"]
    fn sync_succeeds() {
        let mut dev = open_nv();
        dev.sync().expect("sync should succeed (no-op)");
    }

    /// Verify that SM86 shader compilation succeeds independently of
    /// the driver dispatch path. The compiled SASS is identical whether
    /// the target dispatches via nouveau or nvidia-drm.
    #[test]
    #[ignore = "requires nvidia-drm hardware"]
    fn sm86_compilation_independent_of_driver() {
        let wgsl = r"
@group(0) @binding(0)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main() {
    out[0] = 42u;
}
";
        let opts = coral_reef::CompileOptions {
            target: coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm86),
            opt_level: 2,
            ..Default::default()
        };
        let compiled =
            coral_reef::compile_wgsl_full(wgsl, &opts).expect("SM86 compilation should succeed");
        assert!(!compiled.binary.is_empty());
        eprintln!(
            "SM86 compiled: {} bytes, {} GPRs, {} instrs",
            compiled.binary.len(),
            compiled.info.gpr_count,
            compiled.info.instr_count
        );
    }
}
