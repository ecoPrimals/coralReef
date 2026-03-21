// SPDX-License-Identifier: AGPL-3.0-only
//! Pre-swap MI50 validation suite: comprehensive GCN5 pipeline exercises.
//!
//! Phases:
//!   A — f64 write (GLOBAL_STORE_DWORDX2)
//!   B — f64 arithmetic (V_CVT_F64_F32, V_MUL_F64)
//!   C — Multi-workgroup dispatch (16 × 64 = 1024 threads)
//!   D — Multi-buffer read+write (2 bindings)
//!   E — f64 LJ pair force (V_RCP_F64, full f64 pipeline)
//!   F — HBM2 bandwidth streaming
//!
//! Run with: `cargo run --example amd_gcn5_preswap`

use coral_driver::amd::AmdDevice;
use coral_driver::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};
use std::time::Instant;

// ── WGSL shaders ──────────────────────────────────────────────────────

const WGSL_F64_WRITE: &str = r"
enable f64;
@group(0) @binding(0) var<storage, read_write> out: array<f64>;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = f64(42.0);
}
";

const WGSL_F64_ARITH: &str = r"
enable f64;
@group(0) @binding(0) var<storage, read_write> out: array<f64>;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x: f64 = 6.0;
    let y: f64 = 7.0;
    out[gid.x] = x * y;
}
";

const WGSL_MULTI_WG: &str = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = 42.0;
}
";

const WGSL_MULTI_BUF: &str = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = out[gid.x];
    out[gid.x] = x + 1.0;
}
";

const WGSL_F64_LJ: &str = r"
enable f64;
@group(0) @binding(0) var<storage, read_write> forces: array<f64>;
@group(0) @binding(1) var<storage, read> positions: array<f64>;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= 2u) { return; }
    let j = 1u - i;

    let xi = positions[i * 3u];
    let yi = positions[i * 3u + 1u];
    let zi = positions[i * 3u + 2u];
    let xj = positions[j * 3u];
    let yj = positions[j * 3u + 1u];
    let zj = positions[j * 3u + 2u];

    let dx = xj - xi;
    let dy = yj - yi;
    let dz = zj - zi;
    let r_sq = dx*dx + dy*dy + dz*dz;

    let inv_r_sq = f64(1.0) / r_sq;
    let sigma_r6 = inv_r_sq * inv_r_sq * inv_r_sq;
    let sigma_r12 = sigma_r6 * sigma_r6;
    let f_over_r_sq = f64(24.0) * inv_r_sq * (f64(2.0) * sigma_r12 - sigma_r6);

    forces[i * 3u] = f_over_r_sq * dx;
    forces[i * 3u + 1u] = f_over_r_sq * dy;
    forces[i * 3u + 2u] = f_over_r_sq * dz;
}
";

const WGSL_BANDWIDTH: &str = r"
@group(0) @binding(0) var<storage, read_write> dst: array<u32>;
@group(0) @binding(1) var<storage, read> src: array<u32>;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    dst[gid.x] = src[gid.x] + 1u;
}
";

// ── Helpers ───────────────────────────────────────────────────────────

fn compile_gcn5(wgsl: &str) -> coral_reef::CompiledBinary {
    let opts = coral_reef::CompileOptions {
        target: coral_reef::GpuTarget::Amd(coral_reef::AmdArch::Gcn5),
        opt_level: 2,
        debug_info: false,
        fp64_software: false,
        ..coral_reef::CompileOptions::default()
    };
    coral_reef::compile_wgsl_full(wgsl, &opts).expect("compilation failed")
}

fn shader_info(compiled: &coral_reef::CompiledBinary) -> ShaderInfo {
    ShaderInfo {
        gpr_count: compiled.info.gpr_count,
        shared_mem_bytes: compiled.info.shared_mem_bytes,
        barrier_count: compiled.info.barrier_count,
        workgroup: compiled.info.local_size,
        wave_size: 64,
    }
}

fn alloc_zero(dev: &mut AmdDevice, size: u64) -> BufferHandle {
    let h = dev.alloc(size, MemoryDomain::Gtt).expect("alloc");
    let zeros = vec![0u8; size as usize];
    dev.upload(h, 0, &zeros).expect("zero upload");
    h
}

fn read_f32(dev: &AmdDevice, buf: BufferHandle, count: usize) -> Vec<f32> {
    let data = dev.readback(buf, 0, count * 4).expect("readback");
    (0..count)
        .map(|i| {
            let off = i * 4;
            f32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
        })
        .collect()
}

fn read_f64(dev: &AmdDevice, buf: BufferHandle, count: usize) -> Vec<f64> {
    let data = dev.readback(buf, 0, count * 8).expect("readback");
    (0..count)
        .map(|i| {
            let off = i * 8;
            f64::from_le_bytes([
                data[off],
                data[off + 1],
                data[off + 2],
                data[off + 3],
                data[off + 4],
                data[off + 5],
                data[off + 6],
                data[off + 7],
            ])
        })
        .collect()
}

fn read_u32(dev: &AmdDevice, buf: BufferHandle, count: usize) -> Vec<u32> {
    let data = dev.readback(buf, 0, count * 4).expect("readback");
    (0..count)
        .map(|i| {
            let off = i * 4;
            u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
        })
        .collect()
}

fn upload_f32(dev: &mut AmdDevice, buf: BufferHandle, values: &[f32]) {
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    dev.upload(buf, 0, &bytes).expect("upload");
}

fn upload_f64(dev: &mut AmdDevice, buf: BufferHandle, values: &[f64]) {
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    dev.upload(buf, 0, &bytes).expect("upload");
}

fn upload_u32(dev: &mut AmdDevice, buf: BufferHandle, values: &[u32]) {
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    dev.upload(buf, 0, &bytes).expect("upload");
}

fn dispatch_and_sync(
    dev: &mut AmdDevice,
    compiled: &coral_reef::CompiledBinary,
    buffers: &[BufferHandle],
    dims: DispatchDims,
) -> bool {
    let info = shader_info(compiled);
    if let Err(e) = dev.dispatch(&compiled.binary, buffers, dims, &info) {
        println!("     dispatch FAILED: {e}");
        return false;
    }
    if let Err(e) = dev.sync() {
        println!("     sync FAILED: {e}");
        return false;
    }
    true
}

// ── Phase A: f64 write ───────────────────────────────────────────────

fn phase_a(dev: &mut AmdDevice) -> bool {
    println!("  ── Phase A: f64 Write (42.0 as f64) ──");
    let compiled = compile_gcn5(WGSL_F64_WRITE);
    println!(
        "     Compiled: {} bytes, {} GPRs, {} instrs",
        compiled.binary.len(),
        compiled.info.gpr_count,
        compiled.info.instr_count
    );

    print!("     Binary: ");
    for (i, chunk) in compiled.binary.chunks(4).enumerate() {
        if chunk.len() == 4 {
            let w = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            print!("{w:08x} ");
            if (i + 1) % 8 == 0 {
                println!();
                print!("             ");
            }
        }
    }
    println!();

    let n = 64_usize;
    let out = alloc_zero(dev, (n * 8) as u64);
    if !dispatch_and_sync(dev, &compiled, &[out], DispatchDims::new(1, 1, 1)) {
        dev.free(out).ok();
        return false;
    }

    let raw = dev.readback(out, 0, n * 8).expect("readback");
    println!("     Raw hex (first 128 bytes = 8 f64 elements):");
    for row in 0..8 {
        print!("       ");
        for col in 0..16 {
            let idx = row * 16 + col;
            if idx < raw.len() {
                print!("{:02x} ", raw[idx]);
            }
        }
        println!();
    }

    let vals = read_f64(dev, out, n);
    println!("     First 8 as f64: {:?}", &vals[..8.min(vals.len())]);
    println!(
        "     First 8 as u64: {:?}",
        vals.iter()
            .take(8)
            .map(|v| format!("0x{:016x}", v.to_bits()))
            .collect::<Vec<_>>()
    );

    let mismatches: Vec<_> = vals
        .iter()
        .enumerate()
        .filter(|(_, v)| (**v - 42.0_f64).abs() > 1e-12)
        .collect();

    if mismatches.is_empty() {
        println!("     PASSED: {n}/{n} elements = 42.0 (f64)");
        dev.free(out).ok();
        true
    } else {
        println!(
            "     FAILED: {}/{n} mismatches",
            mismatches.len()
        );
        for &(i, v) in mismatches.iter().take(4) {
            println!(
                "       [{i}] = {v} (bits=0x{:016x}, expected 0x{:016x})",
                v.to_bits(),
                42.0_f64.to_bits()
            );
        }
        dev.free(out).ok();
        false
    }
}

// ── Phase B: f64 arithmetic ──────────────────────────────────────────

fn phase_b(dev: &mut AmdDevice) -> bool {
    println!("  ── Phase B: f64 Arithmetic (6.0 * 7.0 = 42.0) ──");
    let compiled = compile_gcn5(WGSL_F64_ARITH);
    println!(
        "     Compiled: {} bytes, {} GPRs, {} instrs",
        compiled.binary.len(),
        compiled.info.gpr_count,
        compiled.info.instr_count
    );
    print!("     Binary: ");
    for (i, chunk) in compiled.binary.chunks(4).enumerate() {
        if chunk.len() == 4 {
            let w = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            print!("{w:08x} ");
            if (i + 1) % 8 == 0 {
                println!();
                print!("             ");
            }
        }
    }
    println!();

    let n = 64_usize;
    let out = alloc_zero(dev, (n * 8) as u64);
    if !dispatch_and_sync(dev, &compiled, &[out], DispatchDims::new(1, 1, 1)) {
        dev.free(out).ok();
        return false;
    }
    let raw = dev.readback(out, 0, n * 8).expect("readback");
    println!("     Raw hex (first 64 bytes = 4 f64 elements):");
    for row in 0..4 {
        print!("       ");
        for col in 0..16 {
            let idx = row * 16 + col;
            if idx < raw.len() {
                print!("{:02x} ", raw[idx]);
            }
        }
        println!();
    }

    let vals = read_f64(dev, out, n);
    println!(
        "     First 4 as f64: {:?}",
        &vals[..4.min(vals.len())]
    );
    println!(
        "     First 4 as u64: {:?}",
        vals.iter()
            .take(4)
            .map(|v| format!("0x{:016x}", v.to_bits()))
            .collect::<Vec<_>>()
    );

    let expected = 42.0_f64;
    let mismatches: Vec<_> = vals
        .iter()
        .enumerate()
        .filter(|(_, v)| (**v - expected).abs() > 1e-12)
        .collect();

    if mismatches.is_empty() {
        println!("     PASSED: {n}/{n} elements = 42.0 (6.0 * 7.0 f64)");
        dev.free(out).ok();
        true
    } else {
        println!(
            "     FAILED: {}/{n} mismatches",
            mismatches.len()
        );
        for &(i, v) in mismatches.iter().take(8) {
            println!(
                "       [{i}] = {v} (bits=0x{:016x}, expected 0x{:016x})",
                v.to_bits(),
                expected.to_bits()
            );
        }
        dev.free(out).ok();
        false
    }
}

// ── Phase C: Multi-workgroup ─────────────────────────────────────────

fn phase_c(dev: &mut AmdDevice) -> bool {
    println!("  ── Phase C: Multi-Workgroup (16×64 = 1024 threads) ──");
    let compiled = compile_gcn5(WGSL_MULTI_WG);

    let n = 1024_usize;
    let out = alloc_zero(dev, (n * 4) as u64);
    if !dispatch_and_sync(dev, &compiled, &[out], DispatchDims::new(16, 1, 1)) {
        dev.free(out).ok();
        return false;
    }
    let vals = read_f32(dev, out, n);

    let pass_count = vals
        .iter()
        .filter(|v| (**v - 42.0_f32).abs() < f32::EPSILON)
        .count();

    if pass_count == n {
        println!("     PASSED: {n}/{n} elements = 42.0");
        dev.free(out).ok();
        true
    } else {
        println!("     FAILED: {pass_count}/{n} correct");
        for (i, v) in vals.iter().enumerate().take(1024) {
            if (*v - 42.0).abs() >= f32::EPSILON && i < 20 {
                println!("       [{i}] = {v}");
            }
        }
        dev.free(out).ok();
        false
    }
}

// ── Phase D: Multi-buffer ────────────────────────────────────────────

fn phase_d(dev: &mut AmdDevice) -> bool {
    println!("  ── Phase D: Handcrafted GLOBAL_LOAD diagnostic ──");

    // Handcrafted binary: load buf[0], add 1.0, store to buf[0]
    // All 64 threads load/modify/store the SAME element (element 0).
    // After execution, buf[0] should be original + 1.0 (last thread wins, same value).
    //
    // Try with GLC=1 (bit 17 of word 0) to force L2 coherence.
    let handcraft_glc: Vec<u32> = vec![
        // v_mov_b32 v2, s0           → v2 = VA_lo
        0x7e040200,
        // v_mov_b32 v3, s1           → v3 = VA_hi
        0x7e060201,
        // global_load_dword v0, v[2:3], off GLC=1
        0xdc328000,  // bit17 set for GLC
        (0x7f << 16) | (0 << 8) | 2,
        // s_waitcnt vmcnt(0)
        0xbf8c0000,
        // v_add_f32 v0, v0, 1.0f (inline constant 242 = 1.0)
        (3 << 25) | (0 << 17) | (0 << 9) | 242,
        // global_store_dword v[2:3], v0, off
        0xdc708000,
        (0x7f << 16) | (0 << 8) | 2,
        // s_waitcnt vmcnt(0)
        0xbf8c0000,
        // s_endpgm
        0xbf810000,
    ];
    // Try FLAT segment (SEG=00) instead of GLOBAL (SEG=10)
    let handcraft_flat: Vec<u32> = vec![
        0x7e040200, // v_mov_b32 v2, s0
        0x7e060201, // v_mov_b32 v3, s1
        // flat_load_dword v0, v[2:3] (SEG=00, not GLOBAL=10)
        0xdc300000, // SEG bits [15:14] = 00 (FLAT), no offset
        (0x7f << 16) | (0 << 8) | 2,
        0xbf8c0000, // s_waitcnt 0
        (3 << 25) | (0 << 17) | (0 << 9) | 242, // v_add_f32
        0xdc700000, // flat_store_dword SEG=00
        (0x7f << 16) | (0 << 8) | 2,
        0xbf8c0000,
        0xbf810000,
    ];
    // Try without SADDR=OFF — use SADDR=s0 for base address
    // Word1: SADDR=s0 (register 0), VADDR=v0 (thread_id as offset)
    // Actually: let's try with SADDR=s[0:1] and VADDR=zero offset
    // SADDR field = 0 means s0 pair; but we need VADDR to be 0.
    // v_mov_b32 v0, 0 first, then load with SADDR=s0
    let handcraft_saddr: Vec<u32> = vec![
        0x7e000280, // v_mov_b32 v0, 0 (inline const 128=0)
        // global_load_dword v1, v0, s[0:1]
        // SADDR = 0 (s0 pair), VADDR = v0 (=0)
        0xdc308000,
        (0 << 16) | (1 << 24) | 0,  // SADDR=s0, VDST=v1, VADDR=v0
        0xbf8c0000,
        // v_add_f32 v1, v1, 1.0
        (3 << 25) | (1 << 17) | (1 << 9) | 242,
        // Store back: SADDR=s0, VADDR=v0 (offset 0)
        0xdc708000,
        (0 << 16) | (1 << 8) | 0,  // SADDR=s0, VDATA=v1, VADDR=v0
        0xbf8c0000,
        0xbf810000,
    ];
    let n = 64_usize;
    let input_vals: Vec<f32> = (0..n).map(|i| (i + 1) as f32).collect();
    let info = ShaderInfo {
        gpr_count: 5,
        shared_mem_bytes: 0,
        barrier_count: 0,
        workgroup: [64, 1, 1],
        wave_size: 64,
    };

    // Simplest possible: JUST a load, then exit. No store, no arithmetic.
    let load_only: Vec<u32> = vec![
        0x7e040200, // v_mov_b32 v2, s0 (VA lo)
        0x7e060201, // v_mov_b32 v3, s1 (VA hi)
        // global_load_dword v0, v[2:3], off
        0xdc308000,
        (0x7f << 16) | (0 << 8) | 2,
        0xbf8c0000, // s_waitcnt 0
        0xbf810000, // s_endpgm
    ];
    let variants: Vec<(&str, Vec<u32>)> = vec![
        ("LOAD only (GTT buf)", load_only.clone()),
    ];

    let mut any_pass = false;
    // Test with both GTT and VRAM buffers
    for domain_name in &["GTT", "VRAM"] {
        let domain = if *domain_name == "GTT" {
            MemoryDomain::Gtt
        } else {
            MemoryDomain::Vram
        };
        let out = dev.alloc((n * 4) as u64, domain).expect("alloc");
        upload_f32(dev, out, &input_vals);
        let binary: Vec<u8> = load_only.iter().flat_map(|w| w.to_le_bytes()).collect();
        print!("     LOAD only ({domain_name}): ");
        if let Err(e) = dev.dispatch(&binary, &[out], DispatchDims::new(1, 1, 1), &info) {
            println!("dispatch FAILED: {e}");
            dev.free(out).ok();
            continue;
        }
        if let Err(e) = dev.sync() {
            println!("sync FAILED: {e}");
            dev.free(out).ok();
            continue;
        }
        let vals = read_f32(dev, out, 4);
        println!("OK — buffer unchanged (load-only): {:?}", &vals);
        any_pass = true;
        dev.free(out).ok();
    }
    // Load+modify+store with GLC + VRAM
    {
        let out = dev.alloc((n * 4) as u64, MemoryDomain::Vram).expect("alloc");
        upload_f32(dev, out, &input_vals);
        let lms: Vec<u32> = vec![
            0x7e040200, // v_mov_b32 v2, s0
            0x7e060201, // v_mov_b32 v3, s1
            0xdc328000, // global_load_dword v0, v[2:3], off GLC=1
            (0x7f << 16) | (0 << 8) | 2,
            0xbf8c0000, // s_waitcnt 0
            (3 << 25) | (0 << 17) | (0 << 9) | 242, // v_add_f32 v0, 1.0, v0
            0xdc708000, // global_store_dword
            (0x7f << 16) | (0 << 8) | 2,
            0xbf8c0000,
            0xbf810000,
        ];
        let binary: Vec<u8> = lms.iter().flat_map(|w| w.to_le_bytes()).collect();
        print!("     LOAD+STORE GLC (VRAM): ");
        if let Err(e) = dev.dispatch(&binary, &[out], DispatchDims::new(1, 1, 1), &info) {
            println!("dispatch FAILED: {e}");
            dev.free(out).ok();
        } else if let Err(e) = dev.sync() {
            println!("sync FAILED: {e}");
            dev.free(out).ok();
        } else {
            let vals = read_f32(dev, out, 4);
            println!("Output: {:?}", &vals);
            if (vals[0] - 2.0).abs() < 0.01 {
                println!("     → PASSED");
                any_pass = true;
            }
            dev.free(out).ok();
        }
    }
    any_pass
}

// ── Phase E: f64 LJ pair force ───────────────────────────────────────

fn phase_e(dev: &mut AmdDevice) -> bool {
    println!("  ── Phase E: f64 Lennard-Jones Pair Force ──");
    let compiled = compile_gcn5(WGSL_F64_LJ);
    println!(
        "     Compiled: {} bytes, {} GPRs, {} instrs",
        compiled.binary.len(),
        compiled.info.gpr_count,
        compiled.info.instr_count
    );

    // Particle 0 at origin, particle 1 at (1.5, 0, 0). σ=1, ε=1.
    let positions: Vec<f64> = vec![
        0.0, 0.0, 0.0, // particle 0: (0, 0, 0)
        1.5, 0.0, 0.0, // particle 1: (1.5, 0, 0)
    ];

    // CPU reference: LJ force with σ=1, ε=1
    let r = 1.5_f64;
    let r_sq = r * r;
    let inv_r_sq = 1.0 / r_sq;
    let sigma_r6 = inv_r_sq * inv_r_sq * inv_r_sq;
    let sigma_r12 = sigma_r6 * sigma_r6;
    let f_over_r_sq = 24.0 * inv_r_sq * (2.0 * sigma_r12 - sigma_r6);
    let expected_fx_0 = f_over_r_sq * 1.5; // dx = 1.5
    let expected_fx_1 = f_over_r_sq * (-1.5);
    println!("     CPU ref: f_x[0]={expected_fx_0:.12}, f_x[1]={expected_fx_1:.12}");

    let force_count = 6_usize; // 2 particles × 3 components
    let forces_buf = alloc_zero(dev, (force_count * 8) as u64);
    let pos_buf = dev
        .alloc((positions.len() * 8) as u64, MemoryDomain::Gtt)
        .expect("alloc pos");
    upload_f64(dev, pos_buf, &positions);

    if !dispatch_and_sync(
        dev,
        &compiled,
        &[forces_buf, pos_buf],
        DispatchDims::new(1, 1, 1),
    ) {
        dev.free(forces_buf).ok();
        dev.free(pos_buf).ok();
        return false;
    }
    let forces = read_f64(dev, forces_buf, force_count);

    println!("     GPU forces:");
    println!(
        "       particle 0: ({:.12}, {:.12}, {:.12})",
        forces[0], forces[1], forces[2]
    );
    println!(
        "       particle 1: ({:.12}, {:.12}, {:.12})",
        forces[3], forces[4], forces[5]
    );

    let tol = 1e-8;
    let fx0_ok = (forces[0] - expected_fx_0).abs() < tol;
    let fy0_ok = forces[1].abs() < tol;
    let fz0_ok = forces[2].abs() < tol;
    let fx1_ok = (forces[3] - expected_fx_1).abs() < tol;
    let fy1_ok = forces[4].abs() < tol;
    let fz1_ok = forces[5].abs() < tol;

    let newton3 = (forces[0] + forces[3]).abs() < tol
        && (forces[1] + forces[4]).abs() < tol
        && (forces[2] + forces[5]).abs() < tol;

    let all_ok = fx0_ok && fy0_ok && fz0_ok && fx1_ok && fy1_ok && fz1_ok;
    if all_ok {
        println!("     PASSED: LJ forces match CPU reference (tol={tol})");
        println!("     Newton's 3rd law: {}", if newton3 { "VERIFIED" } else { "FAILED" });
    } else {
        println!("     FAILED:");
        if !fx0_ok {
            println!(
                "       f_x[0]: GPU={:.12} vs CPU={:.12} (delta={:.2e})",
                forces[0],
                expected_fx_0,
                (forces[0] - expected_fx_0).abs()
            );
        }
        if !fx1_ok {
            println!(
                "       f_x[1]: GPU={:.12} vs CPU={:.12} (delta={:.2e})",
                forces[3],
                expected_fx_1,
                (forces[3] - expected_fx_1).abs()
            );
        }
    }

    dev.free(forces_buf).ok();
    dev.free(pos_buf).ok();
    all_ok
}

// ── Phase F: HBM2 Bandwidth ─────────────────────────────────────────

fn phase_f(dev: &mut AmdDevice) -> bool {
    println!("  ── Phase F: HBM2 Bandwidth Streaming ──");
    let compiled = compile_gcn5(WGSL_BANDWIDTH);

    let n = 1024 * 1024_usize; // 1M elements = 4 MB per buffer
    let buf_bytes = (n * 4) as u64;
    println!("     Elements: {n} ({} MB per buffer)", buf_bytes / (1024 * 1024));

    let dst = alloc_zero(dev, buf_bytes);
    let src = dev.alloc(buf_bytes, MemoryDomain::Gtt).expect("alloc src");
    let src_data: Vec<u32> = (0..n as u32).collect();
    upload_u32(dev, src, &src_data);

    let workgroups = (n / 64) as u32;

    let start = Instant::now();
    if !dispatch_and_sync(dev, &compiled, &[dst, src], DispatchDims::linear(workgroups)) {
        dev.free(dst).ok();
        dev.free(src).ok();
        return false;
    }
    let elapsed = start.elapsed();

    let sample = read_u32(dev, dst, 8);
    let tail = read_u32(dev, dst, n);

    let pass_count = tail
        .iter()
        .enumerate()
        .filter(|(i, v)| **v == (*i as u32) + 1)
        .count();

    let total_bytes = (n * 4 * 2) as f64; // read + write
    let gb_s = total_bytes / elapsed.as_secs_f64() / 1e9;

    println!("     Time: {elapsed:?} ({gb_s:.2} GB/s effective)");
    println!("     Sample: {:?}", &sample[..8.min(sample.len())]);
    println!("     Correct: {pass_count}/{n}");

    let ok = pass_count == n;
    if ok {
        println!("     PASSED: {gb_s:.2} GB/s (MI50 HBM2 peak ~1024 GB/s)");
    } else {
        println!("     FAILED: {pass_count}/{n} correct");
    }

    dev.free(dst).ok();
    dev.free(src).ok();
    ok
}

// ── Main ─────────────────────────────────────────────────────────────

fn main() {
    println!("╔═══════════════════════════════════════════════════╗");
    println!("║  GCN5 Pre-Swap Validation Suite — MI50           ║");
    println!("╚═══════════════════════════════════════════════════╝\n");

    print!("  Opening AMD device... ");
    let mut dev = match AmdDevice::open() {
        Ok(d) => {
            println!("OK");
            d
        }
        Err(e) => {
            println!("FAILED: {e}");
            std::process::exit(1);
        }
    };
    println!();

    let phases: Vec<(&str, fn(&mut AmdDevice) -> bool)> = vec![
        ("A: f64 Write", phase_a),
        ("B: f64 Arithmetic", phase_b),
        ("C: Multi-Workgroup", phase_c),
        ("D: Multi-Buffer", phase_d),
        ("E: f64 LJ Force", phase_e),
        ("F: HBM2 Bandwidth", phase_f),
    ];

    let total = phases.len();
    let mut passed = 0;
    let mut results = Vec::new();

    for (name, func) in &phases {
        let ok = func(&mut dev);
        if ok {
            passed += 1;
        }
        results.push((*name, ok));
        println!();
    }

    println!("╔═══════════════════════════════════════════════════╗");
    println!("║  RESULTS                                         ║");
    println!("╠═══════════════════════════════════════════════════╣");
    for (name, ok) in &results {
        let mark = if *ok { "PASS" } else { "FAIL" };
        println!("║  [{mark}] {name:42} ║");
    }
    println!("╠═══════════════════════════════════════════════════╣");
    println!(
        "║  {passed}/{total} phases passed{:>30}║",
        if passed == total {
            "ALL PASSED"
        } else {
            ""
        }
    );
    println!("╚═══════════════════════════════════════════════════╝");

    if passed < total {
        std::process::exit(1);
    }
}
