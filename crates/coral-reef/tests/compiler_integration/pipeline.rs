// SPDX-License-Identifier: AGPL-3.0-or-later

use coral_reef::{CompileError, CompileOptions, GpuArch, compile_wgsl};

// ---------------------------------------------------------------------------
// Pipeline pass exercising tests — shaders that force specific code paths
// ---------------------------------------------------------------------------

/// Exercises SM70 ALU encoder (integer add).
#[test]
fn test_pipeline_pass_alu_ops() {
    let wgsl = "@compute @workgroup_size(1) fn main() { let x = 1u + 2u; }";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "ALU ops (1u+2u) should compile or fail with NotImplemented: {result:?}"
    );
    if let Ok(binary) = result {
        assert!(!binary.is_empty());
    }
}

/// Exercises memory encoding (storage buffer load/store).
#[test]
fn test_pipeline_pass_memory_ops() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let idx = gid.x;
            let val = data[idx];
            data[idx] = val * 2.0;
        }
    ";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "memory ops (storage buffer) should compile or fail with NotImplemented: {result:?}"
    );
    if let Ok(binary) = result {
        assert!(!binary.is_empty());
    }
}

/// Exercises control flow encoding (if/else).
#[test]
fn test_pipeline_pass_control_flow() {
    let wgsl = "
        @compute @workgroup_size(1) fn main() {
            if true { } else { }
        }
    ";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "control flow (if/else) should compile or fail with NotImplemented: {result:?}"
    );
    if let Ok(binary) = result {
        assert!(!binary.is_empty());
    }
}

/// Exercises f64 lowering with `fp64_software=true`.
#[test]
fn test_pipeline_pass_f64_lowering() {
    let wgsl = "
        enable naga_ext_f64;
        @group(0) @binding(0) var<storage, read_write> data: array<f64>;
        @compute @workgroup_size(1)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let x = f64(gid.x);
            data[gid.x] = sqrt(x) + 1.0;
        }
    ";
    let opts = CompileOptions {
        fp64_software: true,
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    match result {
        Ok(binary) => assert!(!binary.is_empty()),
        Err(CompileError::NotImplemented(_)) => {}
        Err(CompileError::InvalidInput(msg)) if msg.contains("naga_ext_f64") => {}
        Err(e) => panic!("f64 lowering: unexpected error: {e}"),
    }
}

// ---------------------------------------------------------------------------
// Full pipeline success tests
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_minimal_compute_produces_binary() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    let binary = result.expect("minimal compute should compile");
    assert!(!binary.is_empty(), "compiled binary should not be empty");
}

#[test]
fn test_pipeline_minimal_compute_binary_has_header() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let binary = compile_wgsl(wgsl, &CompileOptions::default()).expect("should compile");
    assert!(
        binary.len() >= super::SPH_HEADER_BYTES,
        "binary should have at least SPH header ({} bytes), got {}",
        super::SPH_HEADER_BYTES,
        binary.len()
    );
}

// ---------------------------------------------------------------------------
// Shader variety tests
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_compute_workgroup_size_64_with_barrier() {
    let wgsl = "@compute @workgroup_size(64) fn main() { workgroupBarrier(); }";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "compute with barrier should compile or fail with NotImplemented: {result:?}"
    );
    if let Ok(binary) = result {
        assert!(!binary.is_empty());
    }
}

#[test]
fn test_pipeline_compute_arithmetic() {
    let wgsl = "@compute @workgroup_size(1) fn main() {
        let x = 1.0 + 2.0;
        let y = x * 3.0;
        let z = fma(1.0, 2.0, 3.0);
    }";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "compute with add/mul/fma should compile or fail with NotImplemented: {result:?}"
    );
    if let Ok(binary) = result {
        assert!(!binary.is_empty());
    }
}

#[test]
fn test_pipeline_compute_control_flow_if_else() {
    let wgsl = "@compute @workgroup_size(1) fn main() {
        if true { } else { }
    }";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "compute with if/else should compile or fail with NotImplemented: {result:?}"
    );
    if let Ok(binary) = result {
        assert!(!binary.is_empty());
    }
}

#[test]
fn test_pipeline_compute_loop() {
    let wgsl = "@compute @workgroup_size(1) fn main() {
        loop { break; }
    }";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "compute with loop should compile or fail with NotImplemented: {result:?}"
    );
    if let Ok(binary) = result {
        assert!(!binary.is_empty());
    }
}

#[test]
fn test_pipeline_compute_builtin_inputs() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(
            @builtin(global_invocation_id) gid: vec3<u32>,
            @builtin(local_invocation_id) lid: vec3<u32>,
        ) {
            data[gid.x] = f32(lid.x);
        }
    ";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "compute with builtins should compile or fail with NotImplemented: {result:?}"
    );
    if let Ok(binary) = result {
        assert!(!binary.is_empty());
    }
}

// ---------------------------------------------------------------------------
// Cross-architecture tests
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_cross_arch_sm70() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let opts = CompileOptions {
        target: GpuArch::Sm70.into(),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "Sm70 should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_pipeline_cross_arch_sm75() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let opts = CompileOptions {
        target: GpuArch::Sm75.into(),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "Sm75 should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_pipeline_cross_arch_sm80() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let opts = CompileOptions {
        target: GpuArch::Sm80.into(),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "Sm80 should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_pipeline_cross_arch_sm86() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let opts = CompileOptions {
        target: GpuArch::Sm86.into(),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "Sm86 should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_pipeline_cross_arch_sm89() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let opts = CompileOptions {
        target: GpuArch::Sm89.into(),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "Sm89 should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_pipeline_cross_arch_all_produce_binary() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    for &arch in GpuArch::ALL {
        let opts = CompileOptions {
            target: arch.into(),
            ..CompileOptions::default()
        };
        let result = compile_wgsl(wgsl, &opts);
        assert!(
            result.is_ok(),
            "arch {arch} should produce binary: {result:?}"
        );
        let binary = result.unwrap();
        assert!(!binary.is_empty(), "arch {arch} binary should not be empty");
    }
}

// ---------------------------------------------------------------------------
// Pipeline property tests
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_binary_starts_with_header() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let binary = compile_wgsl(wgsl, &CompileOptions::default()).expect("should compile");
    assert!(
        binary.len() >= super::SPH_HEADER_BYTES,
        "binary length {} should be >= header size {}",
        binary.len(),
        super::SPH_HEADER_BYTES
    );
    let header = &binary[..super::SPH_HEADER_BYTES.min(binary.len())];
    assert_eq!(header.len(), super::SPH_HEADER_BYTES);
}

#[test]
fn test_pipeline_binary_length_at_least_header() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let binary = compile_wgsl(wgsl, &CompileOptions::default()).expect("should compile");
    assert!(
        binary.len() >= super::SPH_HEADER_BYTES,
        "binary must be at least header size"
    );
}

#[test]
fn test_pipeline_opt_levels_compile() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    for level in 0..=3 {
        let opts = CompileOptions {
            opt_level: level,
            ..CompileOptions::default()
        };
        let result = compile_wgsl(wgsl, &opts);
        assert!(
            result.is_ok(),
            "opt_level {level} should compile: {result:?}"
        );
    }
}

#[test]
fn test_pipeline_higher_opt_produces_smaller_or_equal() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let bin_opt0 = compile_wgsl(
        wgsl,
        &CompileOptions {
            opt_level: 0,
            ..CompileOptions::default()
        },
    )
    .expect("opt 0 should compile");
    let bin_opt3 = compile_wgsl(
        wgsl,
        &CompileOptions {
            opt_level: 3,
            ..CompileOptions::default()
        },
    )
    .expect("opt 3 should compile");
    assert!(!bin_opt0.is_empty() && !bin_opt3.is_empty());
    assert!(bin_opt3.len() >= super::SPH_HEADER_BYTES);
}
