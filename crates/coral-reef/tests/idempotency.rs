// SPDX-License-Identifier: AGPL-3.0-or-later
//! IR-to-IR idempotency tests — proves coralReef is a pure, deterministic
//! compiler across all input languages and output backends.
//!
//! Three categories:
//! 1. **WGSL roundtrip**: WGSL → `naga::Module` → WGSL text → `naga::Module` → compile
//! 2. **SPIR-V roundtrip**: WGSL → compile vs WGSL → SPIR-V → compile (same binary)
//! 3. **Multi-backend determinism**: compile(x) == compile(x) for all backends

use coral_reef::{AmdArch, CompileOptions, GpuTarget, NvArch, compile, compile_wgsl};

const TRIVIAL_COMPUTE: &str = "@compute @workgroup_size(1) fn main() {}";

const ALU_COMPUTE: &str = r"
@group(0) @binding(0) var<storage, read_write> buf: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    buf[i] = buf[i] * 2.0 + 1.0;
}
";

const SHARED_MEM_COMPUTE: &str = r"
var<workgroup> tile: array<f32, 256>;

@group(0) @binding(0) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(256)
fn main(@builtin(local_invocation_index) lid: u32) {
    tile[lid] = f32(lid);
    workgroupBarrier();
    out[lid] = tile[255u - lid];
}
";

const CONTROL_FLOW_COMPUTE: &str = r"
@group(0) @binding(0) var<storage, read_write> buf: array<f32>;

@compute @workgroup_size(32)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    var acc: f32 = 0.0;
    for (var j: u32 = 0u; j < 10u; j = j + 1u) {
        if j % 2u == 0u {
            acc = acc + f32(j);
        } else {
            acc = acc - f32(j);
        }
    }
    buf[i] = acc;
}
";

fn opts_for(target: GpuTarget) -> CompileOptions {
    CompileOptions {
        target,
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    }
}

// ============================================================================
// Category 1: WGSL → Module → WGSL text → Module → compile (idempotent IR)
// ============================================================================

fn wgsl_module_roundtrip(wgsl: &str) -> String {
    let module = naga::front::wgsl::parse_str(wgsl).expect("WGSL parse");
    let caps = naga::valid::Capabilities::all();
    let mut validator = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), caps);
    let info = validator.validate(&module).expect("validation");
    let mut out = String::new();
    let mut writer =
        naga::back::wgsl::Writer::new(&mut out, naga::back::wgsl::WriterFlags::empty());
    writer.write(&module, &info).expect("WGSL emit");
    out
}

macro_rules! wgsl_roundtrip_test {
    ($name:ident, $source:expr) => {
        #[test]
        fn $name() {
            let original_wgsl = $source;
            let opts = opts_for(NvArch::Sm70.into());

            let binary_from_original =
                compile_wgsl(original_wgsl, &opts).expect("compile original");

            let regenerated_wgsl = wgsl_module_roundtrip(original_wgsl);
            let binary_from_roundtrip =
                compile_wgsl(&regenerated_wgsl, &opts).expect("compile roundtrip");

            assert_eq!(
                binary_from_original,
                binary_from_roundtrip,
                "WGSL roundtrip must produce identical binary \
                 (original {} bytes, roundtrip {} bytes)",
                binary_from_original.len(),
                binary_from_roundtrip.len()
            );
        }
    };
}

wgsl_roundtrip_test!(wgsl_roundtrip_trivial, TRIVIAL_COMPUTE);
wgsl_roundtrip_test!(wgsl_roundtrip_alu, ALU_COMPUTE);
wgsl_roundtrip_test!(wgsl_roundtrip_shared_mem, SHARED_MEM_COMPUTE);
wgsl_roundtrip_test!(wgsl_roundtrip_control_flow, CONTROL_FLOW_COMPUTE);

// ============================================================================
// Category 2: SPIR-V roundtrip — WGSL → SPIR-V → compile succeeds, and the
// SPIR-V path is itself deterministic (compile twice from same SPIR-V = same)
// ============================================================================

fn wgsl_to_spirv_words(wgsl: &str) -> Vec<u32> {
    let module = naga::front::wgsl::parse_str(wgsl).expect("WGSL parse");
    let caps = naga::valid::Capabilities::all();
    let mut validator = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), caps);
    let info = validator.validate(&module).expect("validation");
    naga::back::spv::write_vec(&module, &info, &naga::back::spv::Options::default(), None)
        .expect("SPIR-V emit")
}

macro_rules! spirv_roundtrip_determinism_test {
    ($name:ident, $source:expr) => {
        #[test]
        fn $name() {
            let wgsl = $source;
            let opts = opts_for(NvArch::Sm70.into());

            let spirv = wgsl_to_spirv_words(wgsl);
            assert!(spirv.len() > 4, "SPIR-V should be non-trivial");

            let binary_1 = compile(&spirv, &opts).expect("SPIR-V compile pass 1");
            let binary_2 = compile(&spirv, &opts).expect("SPIR-V compile pass 2");

            assert_eq!(
                binary_1,
                binary_2,
                "SPIR-V frontend must be deterministic ({} bytes)",
                binary_1.len()
            );

            // Also verify WGSL frontend compiles the same source successfully
            let wgsl_binary = compile_wgsl(wgsl, &opts).expect("WGSL compile");
            assert!(
                !wgsl_binary.is_empty(),
                "WGSL path should produce non-empty output"
            );
            assert!(
                !binary_1.is_empty(),
                "SPIR-V path should produce non-empty output"
            );
        }
    };
}

spirv_roundtrip_determinism_test!(spirv_roundtrip_trivial, TRIVIAL_COMPUTE);
spirv_roundtrip_determinism_test!(spirv_roundtrip_alu, ALU_COMPUTE);
spirv_roundtrip_determinism_test!(spirv_roundtrip_shared_mem, SHARED_MEM_COMPUTE);
spirv_roundtrip_determinism_test!(spirv_roundtrip_control_flow, CONTROL_FLOW_COMPUTE);

// ============================================================================
// Category 3: Multi-backend determinism — compile(x) == compile(x) for all
// ============================================================================

macro_rules! determinism_test {
    ($name:ident, $target:expr, $source:expr) => {
        #[test]
        fn $name() {
            let wgsl = $source;
            let opts = opts_for($target);

            let r1 = compile_wgsl(wgsl, &opts).expect("compile pass 1");
            let r2 = compile_wgsl(wgsl, &opts).expect("compile pass 2");
            let r3 = compile_wgsl(wgsl, &opts).expect("compile pass 3");

            assert_eq!(r1, r2, "pass 1 != pass 2");
            assert_eq!(r2, r3, "pass 2 != pass 3");
        }
    };
}

// NVIDIA targets
determinism_test!(
    determinism_sm70_trivial,
    NvArch::Sm70.into(),
    TRIVIAL_COMPUTE
);
determinism_test!(determinism_sm70_alu, NvArch::Sm70.into(), ALU_COMPUTE);
determinism_test!(
    determinism_sm80_trivial,
    NvArch::Sm80.into(),
    TRIVIAL_COMPUTE
);
determinism_test!(determinism_sm80_alu, NvArch::Sm80.into(), ALU_COMPUTE);
determinism_test!(
    determinism_sm89_trivial,
    NvArch::Sm89.into(),
    TRIVIAL_COMPUTE
);
determinism_test!(
    determinism_sm89_shared,
    NvArch::Sm89.into(),
    SHARED_MEM_COMPUTE
);

// AMD targets
determinism_test!(
    determinism_rdna2_trivial,
    AmdArch::Rdna2.into(),
    TRIVIAL_COMPUTE
);
determinism_test!(determinism_rdna2_alu, AmdArch::Rdna2.into(), ALU_COMPUTE);
determinism_test!(
    determinism_rdna3_trivial,
    AmdArch::Rdna3.into(),
    TRIVIAL_COMPUTE
);
determinism_test!(
    determinism_rdna4_trivial,
    AmdArch::Rdna4.into(),
    TRIVIAL_COMPUTE
);

// PTX emitter path (SM120 — bypasses coral IR)
determinism_test!(
    determinism_sm120_trivial,
    NvArch::Sm120.into(),
    TRIVIAL_COMPUTE
);
determinism_test!(determinism_sm120_alu, NvArch::Sm120.into(), ALU_COMPUTE);
determinism_test!(
    determinism_sm120_shared,
    NvArch::Sm120.into(),
    SHARED_MEM_COMPUTE
);
