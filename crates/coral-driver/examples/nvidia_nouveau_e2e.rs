// SPDX-License-Identifier: AGPL-3.0-or-later
//! Titan V (GV100) nouveau DRM dispatch — end-to-end compute validation.
//!
//! Phases:
//!   A — f32 write (basic dispatch + readback)
//!   B — f32 arithmetic (mul + verify)
//!   C — Multi-workgroup dispatch (4 × 64 = 256 threads)
//!   D — f64 write (V_CVT_F64_F32, GLOBAL_STORE_DWORDX2)
//!   E — f64 LJ pair force (full f64 pipeline, Newton's 3rd law)
//!
//! Requires: Titan V bound to nouveau (not vfio-pci).
//! Run with: `cargo run --example nvidia_nouveau_e2e`

use coral_driver::nv::NvDevice;
use coral_driver::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

const WGSL_F32_WRITE: &str = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = 42.0;
}
";

const WGSL_F32_ARITH: &str = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x: f32 = 6.0;
    let y: f32 = 7.0;
    out[gid.x] = x * y;
}
";

const WGSL_MULTI_WG: &str = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = f32(gid.x) + 1.0;
}
";

const WGSL_F64_WRITE: &str = r"
enable f64;
@group(0) @binding(0) var<storage, read_write> out: array<f64>;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = f64(42.0);
}
";

const WGSL_F64_LJ: &str = r"
enable f64;
@group(0) @binding(0) var<storage, read_write> forces: array<f64>;
@group(0) @binding(1) var<storage, read> positions: array<f64>;
@compute @workgroup_size(2) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
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

fn compile_sm70(wgsl: &str) -> coral_reef::CompiledBinary {
    let opts = coral_reef::CompileOptions {
        target: coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm70),
        opt_level: 2,
        debug_info: false,
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
        wave_size: 32,
    }
}

fn dispatch_and_sync(
    dev: &mut NvDevice,
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

fn read_f32(dev: &NvDevice, handle: BufferHandle, count: usize) -> Vec<f32> {
    let bytes = dev.readback(handle, 0, count * 4).unwrap_or_default();
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

fn read_f64(dev: &NvDevice, handle: BufferHandle, count: usize) -> Vec<f64> {
    let bytes = dev.readback(handle, 0, count * 8).unwrap_or_default();
    bytes
        .chunks_exact(8)
        .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

// ── Phase implementations ────────────────────────────────────

fn phase_a(dev: &mut NvDevice) -> bool {
    println!("  [A] f32 Write (64 threads)");
    let compiled = compile_sm70(WGSL_F32_WRITE);
    println!(
        "     Compiled: {} bytes, {} GPRs",
        compiled.binary.len(),
        compiled.info.gpr_count
    );

    let buf = dev.alloc(64 * 4, MemoryDomain::Gtt).unwrap();
    dev.upload(buf, 0, &vec![0u8; 64 * 4]).unwrap();

    if !dispatch_and_sync(dev, &compiled, &[buf], DispatchDims { x: 1, y: 1, z: 1 }) {
        dev.free(buf).ok();
        return false;
    }

    let vals = read_f32(dev, buf, 4);
    dev.free(buf).ok();
    let pass = vals.iter().all(|&v| (v - 42.0).abs() < 0.001);
    println!(
        "     first 4: {:?} → {}",
        vals,
        if pass { "PASS" } else { "FAIL" }
    );
    pass
}

fn phase_b(dev: &mut NvDevice) -> bool {
    println!("  [B] f32 Arithmetic (6*7=42)");
    let compiled = compile_sm70(WGSL_F32_ARITH);

    let buf = dev.alloc(64 * 4, MemoryDomain::Gtt).unwrap();
    dev.upload(buf, 0, &vec![0u8; 64 * 4]).unwrap();

    if !dispatch_and_sync(dev, &compiled, &[buf], DispatchDims { x: 1, y: 1, z: 1 }) {
        dev.free(buf).ok();
        return false;
    }

    let vals = read_f32(dev, buf, 4);
    dev.free(buf).ok();
    let pass = vals.iter().all(|&v| (v - 42.0).abs() < 0.001);
    println!(
        "     first 4: {:?} → {}",
        vals,
        if pass { "PASS" } else { "FAIL" }
    );
    pass
}

fn phase_c(dev: &mut NvDevice) -> bool {
    println!("  [C] Multi-Workgroup (4×64 = 256 threads)");
    let compiled = compile_sm70(WGSL_MULTI_WG);

    let n = 256;
    let buf = dev.alloc(n * 4, MemoryDomain::Gtt).unwrap();
    dev.upload(buf, 0, &vec![0u8; (n * 4) as usize]).unwrap();

    if !dispatch_and_sync(dev, &compiled, &[buf], DispatchDims { x: 4, y: 1, z: 1 }) {
        dev.free(buf).ok();
        return false;
    }

    let vals = read_f32(dev, buf, n as usize);
    dev.free(buf).ok();
    let pass = vals
        .iter()
        .enumerate()
        .all(|(i, &v)| (v - (i as f32 + 1.0)).abs() < 0.001);
    println!(
        "     [0]={:.1}, [127]={:.1}, [255]={:.1} → {}",
        vals[0],
        vals[127],
        vals[255],
        if pass { "PASS" } else { "FAIL" }
    );
    pass
}

fn phase_d(dev: &mut NvDevice) -> bool {
    println!("  [D] f64 Write (64 threads)");
    let compiled = compile_sm70(WGSL_F64_WRITE);
    println!(
        "     Compiled: {} bytes, {} GPRs",
        compiled.binary.len(),
        compiled.info.gpr_count
    );

    let buf = dev.alloc(64 * 8, MemoryDomain::Gtt).unwrap();
    dev.upload(buf, 0, &vec![0u8; 64 * 8]).unwrap();

    if !dispatch_and_sync(dev, &compiled, &[buf], DispatchDims { x: 1, y: 1, z: 1 }) {
        dev.free(buf).ok();
        return false;
    }

    let vals = read_f64(dev, buf, 4);
    dev.free(buf).ok();
    let pass = vals.iter().all(|&v| (v - 42.0).abs() < 0.001);
    println!(
        "     first 4: {:?} → {}",
        vals,
        if pass { "PASS" } else { "FAIL" }
    );
    pass
}

fn phase_e(dev: &mut NvDevice) -> bool {
    println!("  [E] f64 LJ Force (2-particle Lennard-Jones)");
    let compiled = compile_sm70(WGSL_F64_LJ);
    println!(
        "     Compiled: {} bytes, {} GPRs",
        compiled.binary.len(),
        compiled.info.gpr_count
    );

    let force_buf = dev.alloc(2 * 3 * 8, MemoryDomain::Gtt).unwrap();
    let pos_buf = dev.alloc(2 * 3 * 8, MemoryDomain::Gtt).unwrap();
    dev.upload(force_buf, 0, &[0u8; 2 * 3 * 8]).unwrap();

    let positions: [f64; 6] = [-0.55, 0.0, 0.0, 0.55, 0.0, 0.0];
    let pos_bytes: Vec<u8> = positions.iter().flat_map(|v| v.to_le_bytes()).collect();
    dev.upload(pos_buf, 0, &pos_bytes).unwrap();

    if !dispatch_and_sync(
        dev,
        &compiled,
        &[force_buf, pos_buf],
        DispatchDims { x: 1, y: 1, z: 1 },
    ) {
        dev.free(force_buf).ok();
        dev.free(pos_buf).ok();
        return false;
    }

    let forces = read_f64(dev, force_buf, 6);
    dev.free(force_buf).ok();
    dev.free(pos_buf).ok();

    let r = 1.1_f64;
    let r_sq = r * r;
    let inv_r_sq = 1.0 / r_sq;
    let sigma_r6 = inv_r_sq * inv_r_sq * inv_r_sq;
    let sigma_r12 = sigma_r6 * sigma_r6;
    let f_over_r_sq = 24.0 * inv_r_sq * (2.0 * sigma_r12 - sigma_r6);
    let cpu_fx = f_over_r_sq * 1.1;

    println!("     CPU ref: f_x[0]={cpu_fx:.12}, f_x[1]={:.12}", -cpu_fx);
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
    let fx_ok = (forces[0] - (-cpu_fx)).abs() < tol && (forces[3] - cpu_fx).abs() < tol;
    let fy_ok = forces[1].abs() < tol && forces[4].abs() < tol;
    let fz_ok = forces[2].abs() < tol && forces[5].abs() < tol;

    let newton3 = (forces[0] + forces[3]).abs() < tol
        && (forces[1] + forces[4]).abs() < tol
        && (forces[2] + forces[5]).abs() < tol;

    let pass = fx_ok && fy_ok && fz_ok;
    if pass {
        println!("     PASSED: LJ forces match CPU reference (tol={tol})");
    } else {
        println!("     FAILED: LJ forces mismatch");
    }
    if newton3 {
        println!("     Newton's 3rd law: VERIFIED");
    } else {
        println!("     Newton's 3rd law: VIOLATED");
    }
    pass
}

fn main() {
    println!("╔═══════════════════════════════════════════════════╗");
    println!("║  Titan V (GV100) nouveau DRM E2E Validation      ║");
    println!("╚═══════════════════════════════════════════════════╝");
    println!();

    let mut dev = match NvDevice::open() {
        Ok(d) => {
            println!("  Opened nouveau device");
            d
        }
        Err(e) => {
            eprintln!("  FATAL: Cannot open nouveau device: {e}");
            eprintln!("  Is the Titan V bound to nouveau?");
            eprintln!("  Run: sudo bash scripts/rebind-titanv-nouveau.sh");
            std::process::exit(1);
        }
    };

    let results = [
        ("A: f32 Write", phase_a(&mut dev)),
        ("B: f32 Arithmetic", phase_b(&mut dev)),
        ("C: Multi-Workgroup", phase_c(&mut dev)),
        ("D: f64 Write", phase_d(&mut dev)),
        ("E: f64 LJ Force", phase_e(&mut dev)),
    ];

    println!();
    println!("╔═══════════════════════════════════════════════════╗");
    println!("║  RESULTS                                         ║");
    println!("╠═══════════════════════════════════════════════════╣");
    let mut pass_count = 0;
    let total = results.len();
    for (name, passed) in &results {
        let tag = if *passed {
            pass_count += 1;
            "PASS"
        } else {
            "FAIL"
        };
        println!("║  [{tag}] {name:<44}║");
    }
    println!("╠═══════════════════════════════════════════════════╣");
    let status = if pass_count == total {
        "ALL PASSED"
    } else {
        "INCOMPLETE"
    };
    println!("║  {pass_count}/{total} phases passed{status:>28}║");
    println!("╚═══════════════════════════════════════════════════╝");

    if pass_count < total {
        std::process::exit(1);
    }
}
