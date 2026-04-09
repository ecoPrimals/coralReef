// SPDX-License-Identifier: AGPL-3.0-or-later
//! GCN5 end-to-end: coral-reef compile → coral-driver dispatch → readback verify.
//!
//! Compiles a WGSL shader with `coral-reef` targeting GCN5 (GFX906),
//! dispatches via `coral-driver` on the MI50, reads back the output
//! buffer, and verifies that 42.0 was written by the GPU.
//!
//! This proves the entire sovereign compute pipeline from source to silicon:
//!   WGSL → coral-reef (GCN5 ISA) → coral-driver (PM4/DRM) → MI50 → readback
//!
//! Run with: `cargo run --example amd_gcn5_e2e`
//! Requires: MI50/Radeon VII bound to `amdgpu` driver.

use coral_driver::amd::AmdDevice;
use coral_driver::{ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

const WGSL_WRITE_42: &str = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = 42.0;
}
";

const NUM_ELEMENTS: usize = 64;

fn main() {
    println!("╔═══════════════════════════════════════════════════╗");
    println!("║  GCN5 E2E: coral-reef → coral-driver → MI50     ║");
    println!("╚═══════════════════════════════════════════════════╝\n");

    // Phase 1: Compile WGSL to GCN5 native ISA
    print!("  Phase 1: Compile WGSL → GCN5 (gfx906)... ");
    let opts = coral_reef::CompileOptions {
        target: coral_reef::GpuTarget::Amd(coral_reef::AmdArch::Gcn5),
        opt_level: 2,
        debug_info: false,
        fp64_software: false,
        ..coral_reef::CompileOptions::default()
    };
    let compiled = match coral_reef::compile_wgsl_full(WGSL_WRITE_42, &opts) {
        Ok(c) => {
            println!(
                "OK ✓ ({} bytes, {} GPRs, {} instrs)",
                c.binary.len(),
                c.info.gpr_count,
                c.info.instr_count
            );
            print!("  Binary: ");
            for (i, chunk) in c.binary.chunks(4).enumerate() {
                if chunk.len() == 4 {
                    let w = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                    print!("{w:08x} ");
                    if (i + 1) % 8 == 0 {
                        println!();
                        print!("          ");
                    }
                }
            }
            println!();
            c
        }
        Err(e) => {
            println!("FAILED: {e}");
            std::process::exit(1);
        }
    };

    // Phase 2: Open AMD device
    print!("  Phase 2: AmdDevice::open()... ");
    let mut dev = match AmdDevice::open() {
        Ok(d) => {
            println!("OK ✓");
            d
        }
        Err(e) => {
            println!("FAILED: {e}");
            std::process::exit(1);
        }
    };

    // Phase 3: Allocate output buffer
    let buf_size = (NUM_ELEMENTS * 4) as u64;
    print!("  Phase 3: Alloc output buffer ({buf_size} bytes)... ");
    let out_buf = match dev.alloc(buf_size, MemoryDomain::Gtt) {
        Ok(h) => {
            println!("OK ✓");
            h
        }
        Err(e) => {
            println!("FAILED: {e}");
            std::process::exit(1);
        }
    };

    // Zero the buffer first
    let zeros = vec![0u8; NUM_ELEMENTS * 4];
    dev.upload(out_buf, 0, &zeros).expect("zero upload");

    // Phase 3b: Also test a hand-crafted minimal binary that bypasses the compiler.
    // This stores 42.0 to buffer[tid_x] using the simplest possible instruction
    // sequence, to isolate PM4/DRM issues from compiler issues.
    {
        // Test A: ALL threads write 42.0 to element 0 (fixed address, no per-thread offset)
        let test_a: Vec<u32> = vec![
            // v_mov_b32 v2, s0           → v2 = VA_low
            0x7e040200,
            // v_mov_b32 v3, s1           → v3 = VA_high
            0x7e060201,
            // v_mov_b32 v4, 0x42280000   → v4 = 42.0f (literal)
            0x7e0802ff,
            0x42280000,
            // global_store_dword v[2:3], v4, off
            0xdc708000,
            (0x7f << 16) | (4 << 8) | 2,
            // s_waitcnt vmcnt(0)
            0xbf8c0000,
            // s_endpgm
            0xbf810000,
        ];

        // Test B: Thread-indexed store using v_lshlrev + v_add
        let handcraft: Vec<u32> = vec![
            // v_lshlrev_b32 v1, 2, v0   → v1 = tid_x * 4  (GFX9 VOP2 op 18)
            ((18 << 25) | (1 << 17)) | 130,
            // v_mov_b32 v2, s0           → v2 = VA_low
            0x7e040200,
            // v_mov_b32 v3, s1           → v3 = VA_high
            0x7e060201,
            // v_add_co_u32 v2, vcc, v2, v1   (GFX9 VOP2 op 25)
            (25 << 25) | (2 << 17) | (1 << 9) | (256 + 2),
            // v_mov_b32 v4, 0x42280000   → v4 = 42.0f (literal)
            0x7e0802ff,
            0x42280000,
            // global_store_dword v[2:3], v4, off
            0xdc708000,
            (0x7f << 16) | (4 << 8) | 2,
            // s_waitcnt vmcnt(0)
            0xbf8c0000,
            // s_endpgm
            0xbf810000,
        ];

        // Test C: Store raw v0 (thread_id_x) to element 0 — last thread wins
        // This tells us what v0 actually contains.
        let test_c: Vec<u32> = vec![
            // v_mov_b32 v2, s0
            0x7e040200,
            // v_mov_b32 v3, s1
            0x7e060201,
            // global_store_dword v[2:3], v0, off — store v0 to element 0
            0xdc708000,
            (0x7f << 16) | 2,
            // s_waitcnt vmcnt(0)
            0xbf8c0000,
            // s_endpgm
            0xbf810000,
        ];

        for (name, binary) in [
            ("Test A (fixed addr)", &test_a),
            ("Test B (per-thread)", &handcraft),
            ("Test C (dump v0)", &test_c),
        ] {
            dev.upload(out_buf, 0, &zeros).expect("zero buffer");
            let bytes: Vec<u8> = binary.iter().flat_map(|w| w.to_le_bytes()).collect();
            let hc_info = ShaderInfo {
                gpr_count: 5,
                shared_mem_bytes: 0,
                barrier_count: 0,
                workgroup: [64, 1, 1],
                wave_size: 64,
                local_mem_bytes: None,
            };
            print!("  {name}: ");
            match dev.dispatch(&bytes, &[out_buf], DispatchDims::new(1, 1, 1), &hc_info) {
                Ok(()) => {}
                Err(e) => {
                    println!("dispatch FAILED: {e}");
                    continue;
                }
            }
            match dev.sync() {
                Ok(()) => {}
                Err(e) => {
                    println!("sync FAILED: {e}");
                    continue;
                }
            }
            let d = dev
                .readback(out_buf, 0, NUM_ELEMENTS * 4)
                .expect("readback");
            let mut vals = Vec::new();
            for i in 0..NUM_ELEMENTS.min(8) {
                let off = i * 4;
                let w = u32::from_le_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]]);
                let v = f32::from_le_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]]);
                vals.push(format!("[{i}]=0x{w:08x}({v})"));
            }
            let pass = (0..NUM_ELEMENTS)
                .filter(|&i| {
                    let off = i * 4;
                    let v = f32::from_le_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]]);
                    (v - 42.0).abs() < f32::EPSILON
                })
                .count();
            println!("{pass}/{NUM_ELEMENTS} = 42.0 | {}", vals.join(" "));
        }
        // Test F: Dump V_MAD result — all lanes write MAD(v0,4,0) to element 0
        let test_f: Vec<u32> = vec![
            // v_mad_u32_u24 v1, v0, 4, 0  (GFX9 VOP3a: OP=451 at [25:16])
            (0b110100u32 << 26) | (451 << 16) | 1,
            0x02010900, // SRC0=v0(256), SRC1=4(132), SRC2=0(128)
            // v_mov_b32 v2, s0
            0x7E040200,
            // v_mov_b32 v3, s1
            0x7E060201,
            // global_store_dword v[2:3], v1, off — store MAD result to element 0
            0xDC708000,
            (0x7F << 16) | (1 << 8) | 2,
            0xBF8C0000, // s_waitcnt
            0xBF810000, // s_endpgm
        ];

        // Test G: Use VOP2 V_MUL_U32_U24 (op 8 on GFX9) for multiply
        let test_g: Vec<u32> = vec![
            // v_mul_u32_u24 v1, 4, v0  → v1 = 4 * v0  (GFX9 VOP2 op 8)
            ((8 << 25) | (1 << 17)) | 132,
            // v_mov_b32 v2, s0
            0x7E040200,
            // v_mov_b32 v3, s1
            0x7E060201,
            // v_add_co_u32 v2, vcc, v2, v1  (GFX9 VOP2 op 25)
            (25 << 25) | (2 << 17) | (1 << 9) | (256 + 2),
            // v_mov_b32 v4, 42.0f (literal)
            0x7E0802FF,
            0x42280000,
            // global_store_dword v[2:3], v4, off
            0xDC708000,
            (0x7F << 16) | (4 << 8) | 2,
            0xBF8C0000,
            0xBF810000,
        ];

        for (name, binary) in [
            ("Test F (MAD dump)", &test_f),
            ("Test G (VOP2 MUL)", &test_g),
        ] {
            dev.upload(out_buf, 0, &zeros).expect("zero buffer");
            let bytes: Vec<u8> = binary.iter().flat_map(|w| w.to_le_bytes()).collect();
            let hc_info = ShaderInfo {
                gpr_count: 5,
                shared_mem_bytes: 0,
                barrier_count: 0,
                workgroup: [64, 1, 1],
                wave_size: 64,
                local_mem_bytes: None,
            };
            print!("  {name}: ");
            match dev.dispatch(&bytes, &[out_buf], DispatchDims::new(1, 1, 1), &hc_info) {
                Ok(()) => {}
                Err(e) => {
                    println!("dispatch FAILED: {e}");
                    continue;
                }
            }
            match dev.sync() {
                Ok(()) => {}
                Err(e) => {
                    println!("sync FAILED: {e}");
                    continue;
                }
            }
            let d = dev
                .readback(out_buf, 0, NUM_ELEMENTS * 4)
                .expect("readback");
            let mut vals = Vec::new();
            for i in 0..NUM_ELEMENTS.min(8) {
                let off = i * 4;
                let w = u32::from_le_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]]);
                let v = f32::from_le_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]]);
                vals.push(format!("[{i}]=0x{w:08x}({v})"));
            }
            let pass = (0..NUM_ELEMENTS)
                .filter(|&i| {
                    let off = i * 4;
                    let v = f32::from_le_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]]);
                    (v - 42.0).abs() < f32::EPSILON
                })
                .count();
            println!("{pass}/{NUM_ELEMENTS} = 42.0 | {}", vals.join(" "));
        }

        // Test E: Use V_MAD_U32_U24 (VOP3) for byte offset instead of v_lshlrev
        let test_e: Vec<u32> = vec![
            // v_mad_u32_u24 v1, v0, 4, 0  (GFX9 VOP3a: OP=451 at [25:16])
            (0b110100u32 << 26) | (451 << 16) | 1,
            0x02010900, // word 1: SRC0=v0(256), SRC1=4(132), SRC2=0(128)
            // v_mov_b32 v2, s0
            0x7E040200,
            // v_mov_b32 v3, s1
            0x7E060201,
            // v_add_co_u32 v2, vcc, v2, v1  (GFX9 VOP2 op 25)
            (25 << 25) | (2 << 17) | (1 << 9) | (256 + 2),
            // v_mov_b32 v4, 42.0f (literal)
            0x7E0802FF,
            0x42280000,
            // global_store_dword v[2:3], v4, off
            0xDC708000,
            (0x7F << 16) | (4 << 8) | 2,
            // s_waitcnt vmcnt(0)
            0xBF8C0000,
            // s_endpgm
            0xBF810000,
        ];

        for (name, binary) in [("Test E (VOP3 MAD)", &test_e)] {
            dev.upload(out_buf, 0, &zeros).expect("zero buffer");
            let bytes: Vec<u8> = binary.iter().flat_map(|w| w.to_le_bytes()).collect();
            let hc_info = ShaderInfo {
                gpr_count: 5,
                shared_mem_bytes: 0,
                barrier_count: 0,
                workgroup: [64, 1, 1],
                wave_size: 64,
                local_mem_bytes: None,
            };
            print!("  {name}: ");
            match dev.dispatch(&bytes, &[out_buf], DispatchDims::new(1, 1, 1), &hc_info) {
                Ok(()) => {}
                Err(e) => {
                    println!("dispatch FAILED: {e}");
                    continue;
                }
            }
            match dev.sync() {
                Ok(()) => {}
                Err(e) => {
                    println!("sync FAILED: {e}");
                    continue;
                }
            }
            let d = dev
                .readback(out_buf, 0, NUM_ELEMENTS * 4)
                .expect("readback");
            let mut vals = Vec::new();
            for i in 0..NUM_ELEMENTS.min(8) {
                let off = i * 4;
                let w = u32::from_le_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]]);
                let v = f32::from_le_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]]);
                vals.push(format!("[{i}]=0x{w:08x}({v})"));
            }
            let pass = (0..NUM_ELEMENTS)
                .filter(|&i| {
                    let off = i * 4;
                    let v = f32::from_le_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]]);
                    (v - 42.0).abs() < f32::EPSILON
                })
                .count();
            println!("{pass}/{NUM_ELEMENTS} = 42.0 | {}", vals.join(" "));
        }

        // Test D: Run the COMPILER binary with minimal ShaderInfo (gpr=5)
        // to isolate whether ShaderInfo differences cause the failure.
        {
            dev.upload(out_buf, 0, &zeros).expect("zero buffer");
            let d_info = ShaderInfo {
                gpr_count: 5,
                shared_mem_bytes: 0,
                barrier_count: 0,
                workgroup: [64, 1, 1],
                wave_size: 64,
                local_mem_bytes: None,
            };
            print!("  Test D (compiler binary, gpr=5): ");
            match dev.dispatch(
                &compiled.binary,
                &[out_buf],
                DispatchDims::new(1, 1, 1),
                &d_info,
            ) {
                Ok(()) => {}
                Err(e) => {
                    println!("dispatch FAILED: {e}");
                }
            }
            match dev.sync() {
                Ok(()) => {}
                Err(e) => {
                    println!("sync FAILED: {e}");
                }
            }
            let d = dev
                .readback(out_buf, 0, NUM_ELEMENTS * 4)
                .expect("readback");
            let mut vals = Vec::new();
            for i in 0..NUM_ELEMENTS.min(8) {
                let off = i * 4;
                let w = u32::from_le_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]]);
                let v = f32::from_le_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]]);
                vals.push(format!("[{i}]=0x{w:08x}({v})"));
            }
            let pass = (0..NUM_ELEMENTS)
                .filter(|&i| {
                    let off = i * 4;
                    let v = f32::from_le_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]]);
                    (v - 42.0).abs() < f32::EPSILON
                })
                .count();
            println!("{pass}/{NUM_ELEMENTS} = 42.0 | {}", vals.join(" "));
        }

        // Re-zero the buffer for the compiler test
        dev.upload(out_buf, 0, &zeros).expect("re-zero");
    }

    // Phase 4: Dispatch
    let info = ShaderInfo {
        gpr_count: compiled.info.gpr_count,
        shared_mem_bytes: compiled.info.shared_mem_bytes,
        barrier_count: compiled.info.barrier_count,
        workgroup: compiled.info.local_size,
        wave_size: 64,
        local_mem_bytes: None,
    };
    let dims = DispatchDims::new(1, 1, 1);

    print!("  Phase 4: Dispatch (1 workgroup × 64 threads)... ");
    match dev.dispatch(&compiled.binary, &[out_buf], dims, &info) {
        Ok(()) => println!("OK ✓"),
        Err(e) => {
            println!("FAILED: {e}");
            std::process::exit(1);
        }
    }

    // Phase 5: Sync
    print!("  Phase 5: Sync... ");
    match dev.sync() {
        Ok(()) => println!("OK ✓"),
        Err(e) => {
            println!("FAILED: {e}");
            std::process::exit(1);
        }
    }

    // Phase 6: Readback and verify
    print!("  Phase 6: Readback and verify... ");
    let data = match dev.readback(out_buf, 0, NUM_ELEMENTS * 4) {
        Ok(d) => d,
        Err(e) => {
            println!("FAILED: {e}");
            std::process::exit(1);
        }
    };

    let mut pass_count = 0;
    let mut fail_count = 0;
    print!("  Raw data: ");
    for i in 0..NUM_ELEMENTS {
        let off = i * 4;
        let val = f32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        if i < 16 || (val - 42.0).abs() >= f32::EPSILON {
            print!("[{i}]={val} ");
        }
        if (val - 42.0).abs() < f32::EPSILON {
            pass_count += 1;
        } else {
            fail_count += 1;
            if fail_count <= 5 {
                eprintln!("    [MISMATCH] out[{i}] = {val}, expected 42.0");
            }
        }
    }
    println!();

    if fail_count == 0 {
        println!("OK ✓ ({pass_count}/{NUM_ELEMENTS} elements = 42.0)");
    } else {
        println!("FAILED ({fail_count}/{NUM_ELEMENTS} mismatches)");
    }

    // Phase 7: Scan full page for 42.0 — check if stores land at wrong offsets
    {
        let full_data = dev.readback(out_buf, 0, 4096).unwrap_or_default();
        let mut found_offsets = Vec::new();
        for i in 0..(full_data.len() / 4) {
            let off = i * 4;
            if off + 4 <= full_data.len() {
                let w = u32::from_le_bytes([
                    full_data[off],
                    full_data[off + 1],
                    full_data[off + 2],
                    full_data[off + 3],
                ]);
                if w == 0x42280000 {
                    found_offsets.push(i);
                } else if w != 0 && (found_offsets.len() < 20 || i < 128) {
                    println!("  [scan] dword[{i}] = 0x{w:08x} (non-zero)");
                }
            }
        }
        println!("  Full page scan: 42.0f found at dword offsets: {found_offsets:?}");
    }

    // Summary
    println!();
    if fail_count == 0 {
        println!("  ═══════════════════════════════════════════════════");
        println!("  ✓ GCN5 E2E DISPATCH: ALL PHASES PASSED");
        println!("  ═══════════════════════════════════════════════════");
        println!();
        println!("  WGSL → coral-reef (GCN5/GFX906) → coral-driver (PM4)");
        println!("  → MI50 GPU execution → readback verification PASSED.");
        println!("  The sovereign compute pipeline compiles and executes");
        println!("  real compute shaders on AMD GCN5 hardware.");
    } else {
        println!("  ═══════════════════════════════════════════════════");
        println!("  ✗ GCN5 E2E DISPATCH: VERIFICATION FAILED");
        println!("  ═══════════════════════════════════════════════════");
        println!("  Dispatch succeeded but output data is wrong.");
        println!("  This may indicate an encoding or address issue.");
        std::process::exit(1);
    }
}
