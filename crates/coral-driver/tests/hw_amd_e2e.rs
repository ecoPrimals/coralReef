// SPDX-License-Identifier: AGPL-3.0-only
//! Layer 4 — AMD E2E: compile + dispatch + readback + verify.
//!
//! This is the full sovereign pipeline test: WGSL source → coral-reef
//! compiler → native GFX1030 binary → coral-driver dispatch → GPU
//! executes → readback → host verifies.
//!
//! The PM4 layer passes buffer VAs via `COMPUTE_USER_DATA` registers,
//! and the compiler materializes `CBuf` references as `V_MOV` from the
//! corresponding user SGPRs.
//!
//! Run: `cargo test --test hw_amd_e2e -- --ignored`

use coral_driver::amd::AmdDevice;
use coral_driver::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};
use coral_reef::CompileOptions;
use coral_reef::gpu_arch::{AmdArch, GpuTarget};

const WRITE_42_SHADER: &str = r"
@group(0) @binding(0)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main() {
    out[0] = 42u;
}
";

fn open_amd() -> AmdDevice {
    AmdDevice::open().expect("AmdDevice::open() failed — is amdgpu loaded?")
}

fn try_compile_for_rdna2(
    wgsl: &str,
) -> Result<coral_reef::backend::CompiledBinary, coral_reef::error::CompileError> {
    let opts = CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        opt_level: 2,
        debug_info: false,
        fp64_software: false,
        fma_policy: coral_reef::FmaPolicy::AllowFusion,
        ..Default::default()
    };
    coral_reef::compile_wgsl_full(wgsl, &opts)
}

fn encode_v_mov_b32(dst_vgpr: u8, imm32: u32) -> Vec<u32> {
    let vop1_base = |dst: u8, src: u16| -> u32 {
        (0b011_1111_u32 << 25) | (u32::from(dst) << 17) | (1 << 9) | u32::from(src)
    };
    match imm32 {
        0 => vec![vop1_base(dst_vgpr, 128)],
        v @ 1..=64 => vec![vop1_base(dst_vgpr, 128 + u16::try_from(v).unwrap())],
        _ => vec![vop1_base(dst_vgpr, 255), imm32],
    }
}

/// Verify the storage-write shader compiles with the unified ops encoder.
/// This confirms `Ldc` (`SMEM`), `MemBar`, and all required ops are implemented.
#[test]
#[ignore = "requires amdgpu hardware"]
fn storage_write_shader_compiles_for_rdna2() {
    let compiled = try_compile_for_rdna2(WRITE_42_SHADER)
        .expect("storage-write shader should compile with unified ops encoder");
    assert!(
        !compiled.binary.is_empty(),
        "compiled binary should not be empty"
    );
    eprintln!(
        "storage-write shader compiled: {} instruction words, {} GPRs",
        compiled.binary.len(),
        compiled.info.gpr_count,
    );
}

/// Verify a bare `S_ENDPGM` dispatches and syncs without hanging.
/// This isolates the PM4/dispatch pipeline from any shader logic.
#[test]
#[ignore = "requires amdgpu hardware"]
fn nop_shader_dispatches_and_syncs() {
    let mut dev = open_amd();
    // S_ENDPGM = 0xBF810000 (little-endian bytes)
    let shader_bytes: Vec<u8> = 0xBF81_0000_u32.to_le_bytes().to_vec();
    let info = ShaderInfo {
        gpr_count: 4,
        shared_mem_bytes: 0,
        barrier_count: 0,
        workgroup: [1, 1, 1],
    };
    dev.dispatch(&shader_bytes, &[], DispatchDims::linear(1), &info)
        .expect("dispatch nop shader");
    dev.sync().expect("sync nop shader");
}

/// Hand-crafted shader: `V_MOV` + `FLAT_STORE` + `S_WAITCNT` + `S_ENDPGM`.
/// Verifies that buffer VA user SGPRs → VGPR → FLAT store → readback works.
#[test]
#[ignore = "requires amdgpu hardware"]
fn handcrafted_store_42_shader() {
    let mut dev = open_amd();
    let out_buf = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc");
    dev.upload(out_buf, 0, &[0u8; 4096]).expect("zero buf");

    // Hand-craft the shader binary (RDNA2 / GFX1030):
    //   v_mov_b32 v2, s0          ; addr_lo = user SGPR 0
    //   v_mov_b32 v3, s1          ; addr_hi = user SGPR 1
    //   v_mov_b32 v4, 42          ; data = literal 42
    //   flat_store_dword v[2:3], v4
    //   s_waitcnt vmcnt(0) lgkmcnt(0)
    //   s_endpgm
    let flat_w0: u32 = (0b11_0111 << 26) | (28 << 18);
    let flat_w1: u32 = 2 | (4 << 8) | (0x7F << 16);

    let code: Vec<u32> = vec![
        0x7E04_0200, // v_mov_b32 v2, s0 (addr_lo from SGPR0)
        0x7E06_0201, // v_mov_b32 v3, s1 (addr_hi from SGPR1)
        0x7E08_02FF, // v_mov_b32 v4, literal(42) — src0=255
        42,          // literal constant DWORD
        flat_w0,     // flat_store_dword v[2:3], v4 — word 0
        flat_w1,     // flat_store_dword v[2:3], v4 — word 1 (ADDR=2, DATA=4, SADDR=0x7F)
        0xBF8C_0000, // s_waitcnt vmcnt(0) lgkmcnt(0)
        0xBF81_0000, // s_endpgm
    ];

    let mut binary = Vec::with_capacity(code.len() * 4);
    for word in &code {
        binary.extend_from_slice(&word.to_le_bytes());
    }

    let info = ShaderInfo {
        gpr_count: 8,
        shared_mem_bytes: 0,
        barrier_count: 0,
        workgroup: [1, 1, 1],
    };

    dev.dispatch(&binary, &[out_buf], DispatchDims::linear(1), &info)
        .expect("dispatch handcrafted shader");
    dev.sync().expect("sync handcrafted shader");

    let readback = dev.readback(out_buf, 0, 4).expect("readback");
    let value = u32::from_le_bytes(readback[..4].try_into().expect("4 bytes"));
    eprintln!("handcrafted shader readback: {value}");
    assert_eq!(value, 42, "GPU should have written 42, got {value}");

    dev.free(out_buf).expect("free");
}

/// Regression: hardcoded-VA shader bypasses user SGPRs to verify
/// FLAT/GLOBAL memory addressing, inline constants, and wave32 dispatch.
#[test]
#[ignore = "requires amdgpu hardware"]
fn hardcoded_va_store_42_shader() {
    let mut dev = open_amd();
    let out_buf = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc");
    let sentinel = 0xDEAD_BEEF_u32;
    let sentinel_bytes = sentinel.to_le_bytes();
    let mut fill = vec![0u8; 4096];
    for chunk in fill.chunks_exact_mut(4) {
        chunk.copy_from_slice(&sentinel_bytes);
    }
    dev.upload(out_buf, 0, &fill).expect("fill buf");

    let buf_va = dev.buffer_gpu_va(out_buf).expect("buffer VA");
    #[expect(clippy::cast_possible_truncation, reason = "splitting 64-bit VA")]
    let va_lo = buf_va as u32;
    let va_hi = (buf_va >> 32) as u32;

    let mut code: Vec<u32> = Vec::new();
    code.extend(encode_v_mov_b32(2, va_lo));
    code.extend(encode_v_mov_b32(3, va_hi));
    code.extend(encode_v_mov_b32(4, 42));
    code.extend(encode_v_mov_b32(5, 7));

    let flat_store = |offset: u32, data_vgpr: u32| -> [u32; 2] {
        let w0 = (0b11_0111 << 26) | (28 << 18) | (0b10 << 14) | offset;
        let w1 = 2 | (data_vgpr << 8) | (0x7F << 16);
        [w0, w1]
    };
    code.extend(flat_store(0, 2));
    code.extend(flat_store(4, 4));
    code.extend(flat_store(8, 5));
    code.push(0xBF8C_0000); // s_waitcnt vmcnt(0) lgkmcnt(0)
    code.push(0xBF81_0000); // s_endpgm

    let binary: Vec<u8> = code.iter().flat_map(|w| w.to_le_bytes()).collect();
    let info = ShaderInfo {
        gpr_count: 8,
        shared_mem_bytes: 0,
        barrier_count: 0,
        workgroup: [1, 1, 1],
    };
    dev.dispatch(&binary, &[out_buf], DispatchDims::linear(1), &info)
        .expect("dispatch");
    dev.sync().expect("sync");

    let readback = dev.readback(out_buf, 0, 12).expect("readback");
    let v2_val = u32::from_le_bytes(readback[0..4].try_into().unwrap());
    let v4_val = u32::from_le_bytes(readback[4..8].try_into().unwrap());
    let v5_val = u32::from_le_bytes(readback[8..12].try_into().unwrap());
    assert_eq!(v2_val, va_lo, "v2 (va_lo) mismatch");
    assert_eq!(v4_val, 42, "v4 (inline 42) mismatch");
    assert_eq!(v5_val, 7, "v5 (inline 7) mismatch");

    dev.free(out_buf).expect("free");
}

/// Full E2E: compile + dispatch + readback + verify.
///
/// Compiles a WGSL storage-write shader, dispatches it on the GPU,
/// reads back the result, and verifies the GPU wrote 42.
#[test]
#[ignore = "requires amdgpu hardware"]
fn dispatch_writes_42_and_readback_verifies() {
    let compiled =
        try_compile_for_rdna2(WRITE_42_SHADER).expect("storage-write shader should compile");

    let mut dev = open_amd();

    let out_buf: BufferHandle = dev
        .alloc(4096, MemoryDomain::Gtt)
        .expect("alloc output buffer");

    let zero_data = vec![0u8; 4096];
    dev.upload(out_buf, 0, &zero_data)
        .expect("zero output buffer");

    let info = ShaderInfo {
        gpr_count: compiled.info.gpr_count,
        shared_mem_bytes: compiled.info.shared_mem_bytes,
        barrier_count: compiled.info.barrier_count,
        workgroup: compiled.info.local_size,
    };

    dev.dispatch(&compiled.binary, &[out_buf], DispatchDims::linear(1), &info)
        .expect("dispatch");

    dev.sync().expect("sync");

    let readback = dev.readback(out_buf, 0, 4).expect("readback");
    let value = u32::from_le_bytes(readback[..4].try_into().expect("4 bytes"));
    assert_eq!(value, 42, "GPU should have written 42, got {value}");

    dev.free(out_buf).expect("free output buffer");
}

const DOUBLE_FIRST_SHADER: &str = r"
@group(0) @binding(0)
var<storage, read_write> data: array<u32>;

@compute @workgroup_size(1)
fn main() {
    data[0] = data[0] * 2u;
}
";

/// E2E compute validation: upload known data, dispatch `data[0] *= 2`, readback and verify.
///
/// Validates the full read-modify-write pipeline through the GPU:
/// host upload → shader load → ALU multiply → shader store → host readback.
///
/// Currently blocked: the AMD backend's compiled buffer-read path does
/// not yet produce correct SMEM loads for storage buffer reads. The GPU
/// reads 0 instead of the uploaded value. Gated behind `rdna2_buffer_read`
/// cfg flag so it doesn't run in normal `--ignored` sweeps.
#[test]
#[cfg(feature = "rdna2-buffer-read")]
#[ignore = "requires amdgpu hardware"]
fn compute_double_readback_verifies() {
    let compiled =
        try_compile_for_rdna2(DOUBLE_FIRST_SHADER).expect("double shader should compile");

    let mut dev = open_amd();
    let data_buf = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc");

    let input_val: u32 = 21;
    dev.upload(data_buf, 0, &input_val.to_le_bytes())
        .expect("upload");

    let info = ShaderInfo {
        gpr_count: compiled.info.gpr_count,
        shared_mem_bytes: compiled.info.shared_mem_bytes,
        barrier_count: compiled.info.barrier_count,
        workgroup: compiled.info.local_size,
    };

    dev.dispatch(
        &compiled.binary,
        &[data_buf],
        DispatchDims::linear(1),
        &info,
    )
    .expect("dispatch");
    dev.sync().expect("sync");

    let readback = dev.readback(data_buf, 0, 4).expect("readback");
    let value = u32::from_le_bytes(readback[..4].try_into().unwrap());
    assert_eq!(value, 42, "data[0] should be 21*2=42, got {value}");

    dev.free(data_buf).expect("free");
}

const ADD_ONE_SHADER: &str = r"
@group(0) @binding(0) var<storage> a: array<f32>;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(1)
fn main() {
    out[0] = a[0] + 1.0;
}
";

/// E2E: dual-buffer test — read from one buffer, write to another.
///
/// Validates multi-binding dispatch where the shader reads `a[0]` and
/// writes `a[0] + 1.0` to a separate output buffer.
///
/// Currently blocked: same RDNA2 buffer-read limitation as
/// `compute_double_readback_verifies`. Also hits VOP2 VSRC1 limitation
/// for the `a[0] + 1.0` pattern. Gated behind `rdna2-buffer-read`.
#[test]
#[cfg(feature = "rdna2-buffer-read")]
#[ignore = "requires amdgpu hardware"]
fn compute_add_dual_buffer_verifies() {
    let compiled = try_compile_for_rdna2(ADD_ONE_SHADER).expect("add shader should compile");

    let mut dev = open_amd();

    let a_buf = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc a");
    let out_buf = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc out");

    let input_val: f32 = 3.14;
    dev.upload(a_buf, 0, &input_val.to_le_bytes())
        .expect("upload a");
    dev.upload(out_buf, 0, &[0u8; 4]).expect("zero out");

    let info = ShaderInfo {
        gpr_count: compiled.info.gpr_count,
        shared_mem_bytes: compiled.info.shared_mem_bytes,
        barrier_count: compiled.info.barrier_count,
        workgroup: compiled.info.local_size,
    };

    dev.dispatch(
        &compiled.binary,
        &[a_buf, out_buf],
        DispatchDims::linear(1),
        &info,
    )
    .expect("dispatch");
    dev.sync().expect("sync");

    let readback = dev.readback(out_buf, 0, 4).expect("readback");
    let result = f32::from_le_bytes(readback[..4].try_into().unwrap());
    let expected = input_val + 1.0;
    let diff = (result - expected).abs();
    assert!(
        diff < 1e-6,
        "out[0]: expected {expected}, got {result} (diff={diff})"
    );

    dev.free(out_buf).expect("free out");
    dev.free(a_buf).expect("free a");
}
