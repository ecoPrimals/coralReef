// SPDX-License-Identifier: AGPL-3.0-only
//! Sovereign pipeline integration test: coral-parse → compile_wgsl_full → VFIO dispatch → readback.
//!
//! This exercises the full sovereign chain with no CUDA driver in the path:
//!   1. Compile a WGSL compute shader via coral-reef (coral-parse frontend)
//!   2. Open NvVfioComputeDevice (auto-detect SM from BOOT0)
//!   3. Upload input data, dispatch the compiled SASS binary
//!   4. Readback output and validate correctness
//!
//! Tests are `#[ignore]` by default — they require VFIO-bound GPU hardware.
//! Run with: `cargo test -p coral-driver --features vfio --test sovereign_pipeline_test -- --ignored`

use coral_parse::CoralFrontend;

#[cfg(feature = "vfio")]
use coral_driver::{ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

fn compile_wgsl(wgsl: &str, target: coral_reef::GpuTarget) -> coral_reef::CompiledBinary {
    let opts = coral_reef::CompileOptions {
        target,
        ..coral_reef::CompileOptions::default()
    };
    coral_reef::compile_wgsl_full_with(&CoralFrontend, wgsl, &opts)
        .expect("sovereign compile")
}

#[cfg(feature = "vfio")]
fn sm_to_target(sm: u32) -> coral_reef::GpuTarget {
    let arch = match sm {
        35..=37 => coral_reef::NvArch::Sm35,
        70 => coral_reef::NvArch::Sm70,
        75 => coral_reef::NvArch::Sm75,
        80 => coral_reef::NvArch::Sm80,
        86 => coral_reef::NvArch::Sm86,
        89 => coral_reef::NvArch::Sm89,
        _ => coral_reef::NvArch::parse(&format!("sm_{sm}"))
            .unwrap_or(coral_reef::NvArch::Sm70),
    };
    coral_reef::GpuTarget::Nvidia(arch)
}

#[cfg(feature = "vfio")]
fn open_device() -> coral_driver::nv::NvVfioComputeDevice {
    let bdf = std::env::var("CORAL_BDF").unwrap_or_else(|_| {
        eprintln!("Set CORAL_BDF=<pci_bdf> to run sovereign pipeline tests");
        eprintln!("Example: CORAL_BDF=0000:05:00.0 cargo test ...");
        "0000:05:00.0".to_string()
    });

    // Try ember fd-sharing first (works when coral-ember holds the VFIO group).
    // Falls back to direct open for standalone use without ember.
    match coral_driver::vfio::ember_client::request_vfio_fds(&bdf) {
        Ok(fds) => {
            eprintln!("Opened via ember fd-sharing for {bdf}");
            coral_driver::nv::NvVfioComputeDevice::open_from_fds(&bdf, fds, 0, 0)
                .expect("open VFIO device from ember fds")
        }
        Err(e) => {
            eprintln!("Ember not available ({e}), falling back to direct VFIO open");
            coral_driver::nv::NvVfioComputeDevice::open(&bdf, 0, 0)
                .expect("open VFIO device directly")
        }
    }
}

/// Ensure FECS is running on the device. Runs the falcon boot solver if needed.
#[cfg(feature = "vfio")]
fn ensure_fecs_running(dev: &coral_driver::nv::NvVfioComputeDevice) {
    if dev.fecs_is_alive() {
        eprintln!("FECS already alive — skipping boot solver");
        return;
    }

    eprintln!("FECS not alive — probing falcon state...");
    let probe = dev.falcon_probe();
    eprintln!("Falcon probe: {probe:?}");

    eprintln!("Running falcon boot solver...");
    match dev.falcon_boot_solver(None) {
        Ok(results) => {
            for r in &results {
                eprintln!("  boot result: {r:?}");
            }
        }
        Err(e) => {
            eprintln!("WARNING: falcon boot solver error: {e}");
        }
    }

    if dev.fecs_is_alive() {
        eprintln!("FECS now alive after boot solver");
    } else {
        eprintln!("WARNING: FECS still not alive — dispatch may fail");
    }
}

/// NOP shader — compile and dispatch, verify no crash.
#[cfg(feature = "vfio")]
#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn sovereign_pipeline_nop_shader() {
    let mut dev = open_device();
    let sm = dev.sm_version();
    eprintln!("Device SM: {sm}");

    ensure_fecs_running(&dev);

    let wgsl = "@compute @workgroup_size(64) fn main() {}";
    let compiled = compile_wgsl(wgsl, sm_to_target(sm));

    eprintln!(
        "Compiled: {} bytes, {} GPRs, {} instructions",
        compiled.binary.len(),
        compiled.info.gpr_count,
        compiled.info.instr_count,
    );

    let info = ShaderInfo {
        gpr_count: compiled.info.gpr_count,
        shared_mem_bytes: compiled.info.shared_mem_bytes,
        barrier_count: compiled.info.barrier_count,
        workgroup: compiled.info.local_size,
        wave_size: 32,
    };

    dev.dispatch(&compiled.binary, &[], DispatchDims::linear(1), &info)
        .expect("dispatch NOP shader");
    dev.sync().expect("sync NOP shader");

    eprintln!("NOP shader dispatch+sync succeeded — sovereign pipeline alive");
}

/// Buffer write shader: write known values to an output buffer and readback.
#[cfg(feature = "vfio")]
#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn sovereign_pipeline_buffer_readback() {
    let mut dev = open_device();
    let sm = dev.sm_version();
    eprintln!("Device SM: {sm}");

    let wgsl = r#"
@group(0) @binding(0) var<storage, read_write> output: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if idx < arrayLength(&output) {
        output[idx] = idx * 7u + 42u;
    }
}
"#;

    let compiled = compile_wgsl(wgsl, sm_to_target(sm));

    eprintln!(
        "Compiled: {} bytes, {} GPRs, {} instructions, workgroup={:?}",
        compiled.binary.len(),
        compiled.info.gpr_count,
        compiled.info.instr_count,
        compiled.info.local_size,
    );

    let info = ShaderInfo {
        gpr_count: compiled.info.gpr_count,
        shared_mem_bytes: compiled.info.shared_mem_bytes,
        barrier_count: compiled.info.barrier_count,
        workgroup: compiled.info.local_size,
        wave_size: 32,
    };

    const N: usize = 256;
    let output_size = N * std::mem::size_of::<u32>();

    let buf = dev.alloc(output_size as u64, MemoryDomain::VramOrGtt)
        .expect("alloc output buffer");

    // Zero-fill the buffer
    let zeros = vec![0u8; output_size];
    dev.upload(buf, 0, &zeros).expect("upload zeros");

    dev.dispatch(
        &compiled.binary,
        &[buf],
        DispatchDims::linear((N / 64) as u32),
        &info,
    )
    .expect("dispatch buffer write");
    dev.sync().expect("sync buffer write");

    let readback = dev.readback(buf, 0, output_size).expect("readback");
    let values: &[u32] = bytemuck::cast_slice(&readback);

    let mut mismatches = 0;
    for (i, &val) in values.iter().enumerate() {
        let expected = (i as u32) * 7 + 42;
        if val != expected {
            if mismatches < 10 {
                eprintln!("MISMATCH output[{i}]: expected={expected}, got={val}");
            }
            mismatches += 1;
        }
    }

    if mismatches > 0 {
        panic!(
            "Readback validation FAILED: {mismatches}/{N} elements differ. \
             First 10 values: {:?}",
            &values[..10.min(N)]
        );
    }

    eprintln!("Readback validation PASSED: all {N} elements correct");
    eprintln!("Sovereign pipeline end-to-end: WGSL → coral-parse → SASS → VFIO dispatch → readback ✓");
}

/// Input+output shader: read from input buffer, compute, write to output.
#[cfg(feature = "vfio")]
#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn sovereign_pipeline_input_output() {
    let mut dev = open_device();
    let sm = dev.sm_version();
    eprintln!("Device SM: {sm}");

    let wgsl = r#"
@group(0) @binding(0) var<storage, read> input: array<u32>;
@group(0) @binding(1) var<storage, read_write> output: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if idx < arrayLength(&input) {
        output[idx] = input[idx] + 1u;
    }
}
"#;

    let compiled = compile_wgsl(wgsl, sm_to_target(sm));

    let info = ShaderInfo {
        gpr_count: compiled.info.gpr_count,
        shared_mem_bytes: compiled.info.shared_mem_bytes,
        barrier_count: compiled.info.barrier_count,
        workgroup: compiled.info.local_size,
        wave_size: 32,
    };

    const N: usize = 128;
    let buf_size = N * std::mem::size_of::<u32>();

    let input_data: Vec<u32> = (0..N as u32).map(|i| i * 3 + 100).collect();
    let input_bytes: &[u8] = bytemuck::cast_slice(&input_data);

    let input_buf = dev.alloc(buf_size as u64, MemoryDomain::VramOrGtt)
        .expect("alloc input");
    dev.upload(input_buf, 0, input_bytes).expect("upload input");

    let output_buf = dev.alloc(buf_size as u64, MemoryDomain::VramOrGtt)
        .expect("alloc output");
    dev.upload(output_buf, 0, &vec![0u8; buf_size])
        .expect("zero output");

    dev.dispatch(
        &compiled.binary,
        &[input_buf, output_buf],
        DispatchDims::linear((N / 64) as u32),
        &info,
    )
    .expect("dispatch");
    dev.sync().expect("sync");

    let readback = dev.readback(output_buf, 0, buf_size).expect("readback");
    let values: &[u32] = bytemuck::cast_slice(&readback);

    for (i, &val) in values.iter().enumerate() {
        let expected = input_data[i] + 1;
        assert_eq!(
            val, expected,
            "output[{i}]: expected {expected} (input={} + 1), got {val}",
            input_data[i]
        );
    }

    eprintln!("Input+output pipeline PASSED: all {N} elements correct (input+1)");
}

// ---------------------------------------------------------------------------
// K80 sovereign cold boot + dispatch — full bare-metal to readback
// ---------------------------------------------------------------------------

/// Auto-detect firmware directory for GK110 (K80/K20).
#[cfg(feature = "vfio")]
fn find_gk110_firmware_dir() -> Option<std::path::PathBuf> {
    let candidates = [
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/firmware/nvidia/gk110"),
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("data/firmware/nvidia/gk110"),
        std::path::PathBuf::from("/usr/share/coralreef/firmware/nvidia/gk110"),
    ];
    candidates.into_iter().find(|p| p.join("fecs_inst.bin").exists())
}

/// Auto-detect K80 BIOS recipe file.
#[cfg(feature = "vfio")]
fn find_k80_recipe() -> Option<std::path::PathBuf> {
    let candidates = [
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../springs/hotSpring/data/k80/nvidia470-vm-captures/gk210_full_bios_recipe.json"),
        std::path::PathBuf::from("/home/biomegate/Development/ecoPrimals/springs/hotSpring/data/k80/nvidia470-vm-captures/gk210_full_bios_recipe.json"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

/// K80 E2E: sovereign cold boot → compile WGSL → VFIO dispatch → readback.
/// Exercises the fully sovereign pipeline from cold metal to shader output.
#[cfg(feature = "vfio")]
#[test]
#[ignore = "requires VFIO-bound K80 hardware + cold boot recipe"]
fn sovereign_k80_cold_boot_dispatch() {
    use coral_driver::vfio::ember_client;

    let bdf = std::env::var("CORAL_K80_BDF").unwrap_or_else(|_| "0000:4c:00.0".to_string());

    let recipe = find_k80_recipe().expect(
        "K80 BIOS recipe not found. Set CORAL_K80_RECIPE=<path> or check data/k80/ directory"
    );
    eprintln!("Recipe: {}", recipe.display());

    // Phase 1: Cold boot via ember BAR0 access
    let session = ember_client::EmberSession::connect(&bdf)
        .expect("ember session for cold boot");
    eprintln!("Ember session opened for {bdf}");

    let fw_dir = find_gk110_firmware_dir();
    let (_fecs_code, _fecs_data, _gpccs_code, _gpccs_data) = if let Some(ref dir) = fw_dir {
        eprintln!("Firmware dir: {}", dir.display());
        (
            Some(std::fs::read(dir.join("fecs_inst.bin")).expect("read fecs_inst")),
            Some(std::fs::read(dir.join("fecs_data.bin")).expect("read fecs_data")),
            Some(std::fs::read(dir.join("gpccs_inst.bin")).expect("read gpccs_inst")),
            Some(std::fs::read(dir.join("gpccs_data.bin")).expect("read gpccs_data")),
        )
    } else {
        eprintln!("WARNING: firmware directory not found, cold boot without firmware");
        (None, None, None, None)
    };

    // Split cold boot into clock phase, then re-map BAR0 for remaining phases.
    // The clock recipe writes can invalidate the original BAR0 mapping.
    use coral_driver::vfio::channel::diagnostic::boot_follower::RecipeStep;
    use coral_driver::vfio::channel::diagnostic::k80_cold_boot::load_recipe_auto;
    use coral_driver::vfio::channel::diagnostic::replay;

    let full_recipe = load_recipe_auto(&recipe).expect("load recipe");
    eprintln!("Recipe loaded: {} total steps", full_recipe.len());

    // Phase 1: Clock registers (priority 0-2)
    let clock_steps: Vec<RecipeStep> = full_recipe.iter()
        .filter(|s| s.priority <= 2)
        .cloned()
        .collect();
    eprintln!("Phase 1: applying {} clock registers...", clock_steps.len());
    let clock_result = replay::apply_recipe_to_bar0(&session.bar0, &clock_steps)
        .expect("clock replay");
    eprintln!("  clock: applied={} failed={} ptimer={}",
        clock_result.applied, clock_result.failed, clock_result.ptimer_ticking);

    // Drop the initial session and get a fresh one — BAR0 may be stale
    drop(session);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let session2 = ember_client::EmberSession::connect(&bdf)
        .expect("ember session for phase 2");

    // Verify BAR0 still works
    let boot0 = session2.bar0.read_u32(0x0).unwrap_or(0xDEAD);
    eprintln!("Phase 2 session: BOOT0={boot0:#010x}");

    // Check PTIMER
    let pt_a = session2.bar0.read_u32(0x9400).unwrap_or(0);
    std::thread::sleep(std::time::Duration::from_millis(10));
    let pt_b = session2.bar0.read_u32(0x9400).unwrap_or(0);
    eprintln!("  PTIMER: {pt_a:#010x} → {pt_b:#010x} ticking={}", pt_a != pt_b);

    // Phase 2: Infrastructure devinit (priority 3-29)
    let devinit_steps: Vec<RecipeStep> = full_recipe.iter()
        .filter(|s| s.priority > 2 && s.priority < 30)
        .cloned()
        .collect();
    eprintln!("Phase 2: applying {} devinit registers...", devinit_steps.len());
    let devinit_result = replay::apply_recipe_to_bar0(&session2.bar0, &devinit_steps)
        .expect("devinit replay");
    eprintln!("  devinit: applied={} failed={} ptimer={}",
        devinit_result.applied, devinit_result.failed, devinit_result.ptimer_ticking);

    // Phase 3: PGRAPH (priority 30)
    let pgraph_steps: Vec<RecipeStep> = full_recipe.iter()
        .filter(|s| s.priority == 30)
        .cloned()
        .collect();
    if !pgraph_steps.is_empty() {
        eprintln!("Phase 3: applying {} PGRAPH registers...", pgraph_steps.len());
        let pgraph_result = replay::apply_recipe_to_bar0(&session2.bar0, &pgraph_steps)
            .expect("pgraph replay");
        eprintln!("  pgraph: applied={} failed={}",
            pgraph_result.applied, pgraph_result.failed);
    }

    // Diagnostics on session2 BAR0
    let fecs_cpuctl = session2.bar0.read_u32(0x409800).unwrap_or(0xDEAD);
    let pfifo_en = session2.bar0.read_u32(0x2200).unwrap_or(0xDEAD);
    eprintln!("Post-boot: FECS_CPUCTL={fecs_cpuctl:#010x} PFIFO_ENABLE={pfifo_en:#010x}");

    drop(session2);

    // Phase 2: Open compute device via ember fds
    let fds = ember_client::request_vfio_fds(&bdf)
        .expect("request VFIO fds after cold boot");
    let mut dev = coral_driver::nv::NvVfioComputeDevice::open_from_fds(&bdf, fds, 0, 0)
        .expect("open compute device after cold boot");

    let sm = dev.sm_version();
    eprintln!("Compute device opened: SM{sm}");

    eprintln!("FECS alive: {}", dev.fecs_is_alive());
    let probe = dev.falcon_probe();
    eprintln!("Falcon probe: fecs_cpuctl={:#010x} fecs_mb0={:#010x} fecs_pc={:#06x}",
        probe.fecs_cpuctl, probe.fecs_mailbox0, probe.fecs_pc);

    // Phase 3: Compile and dispatch
    let wgsl = r#"
@group(0) @binding(0) var<storage, read_write> output: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    output[idx] = idx * 7u + 42u;
}
"#;

    let compiled = compile_wgsl(wgsl, sm_to_target(sm));
    eprintln!(
        "Compiled: {} bytes, {} GPRs, {} instructions",
        compiled.binary.len(), compiled.info.gpr_count, compiled.info.instr_count,
    );

    let info = ShaderInfo {
        gpr_count: compiled.info.gpr_count,
        shared_mem_bytes: compiled.info.shared_mem_bytes,
        barrier_count: compiled.info.barrier_count,
        workgroup: compiled.info.local_size,
        wave_size: 32,
    };

    const N: usize = 256;
    let output_size = N * std::mem::size_of::<u32>();

    let buf = dev.alloc(output_size as u64, MemoryDomain::VramOrGtt)
        .expect("alloc output");
    dev.upload(buf, 0, &vec![0u8; output_size]).expect("zero output");

    dev.dispatch(
        &compiled.binary, &[buf],
        DispatchDims::linear((N / 64) as u32), &info,
    ).expect("dispatch K80");
    dev.sync().expect("sync K80");

    let readback = dev.readback(buf, 0, output_size).expect("readback");
    let values: &[u32] = bytemuck::cast_slice(&readback);

    let mut mismatches = 0;
    for (i, &val) in values.iter().enumerate() {
        let expected = (i as u32) * 7 + 42;
        if val != expected {
            if mismatches < 10 {
                eprintln!("MISMATCH output[{i}]: expected={expected}, got={val}");
            }
            mismatches += 1;
        }
    }

    if mismatches > 0 {
        panic!("K80 readback FAILED: {mismatches}/{N} differ");
    }

    eprintln!("K80 sovereign cold boot → dispatch → readback: ALL {N} elements correct");
    eprintln!("Full sovereign pipeline: cold metal → WGSL → SASS → VFIO → compute ✓");
}

// ---------------------------------------------------------------------------
// Compile-only tests (no hardware required)
// ---------------------------------------------------------------------------

/// Compile-only: NOP shader for SM35 and SM70 — baseline pipeline test.
#[test]
fn sovereign_compile_nop_sm35_and_sm70() {
    let wgsl = "@compute @workgroup_size(64) fn main() {}";

    for (arch, sm) in [
        (coral_reef::NvArch::Sm35, 35u32),
        (coral_reef::NvArch::Sm70, 70),
    ] {
        let compiled = compile_wgsl(wgsl, coral_reef::GpuTarget::Nvidia(arch));

        assert!(!compiled.binary.is_empty(), "SM{sm}: binary should not be empty");
        assert_eq!(compiled.info.local_size, [64, 1, 1], "SM{sm}: workgroup_size mismatch");

        eprintln!(
            "SM{sm} NOP: {} bytes, {} GPRs, {} instrs — OK",
            compiled.binary.len(),
            compiled.info.gpr_count,
            compiled.info.instr_count,
        );
    }
}

/// Compile-only: storage buffer shader with integer arithmetic for SM70.
/// Exercises the SM70 legalization: OpIMul → OpIMad, OpIAdd2 → OpIAdd3.
#[test]
fn sovereign_compile_storage_buffer_sm70() {
    let wgsl = r#"
@group(0) @binding(0) var<storage, read_write> output: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    output[idx] = idx * 7u + 42u;
}
"#;

    let compiled = compile_wgsl(wgsl, coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm70));

    assert!(!compiled.binary.is_empty(), "SM70 storage buffer: binary should not be empty");
    assert!(
        compiled.info.gpr_count >= 2,
        "SM70 storage buffer: expected >=2 GPRs, got {}",
        compiled.info.gpr_count,
    );
    eprintln!(
        "SM70 storage buffer: {} bytes, {} GPRs, {} instrs — OK",
        compiled.binary.len(),
        compiled.info.gpr_count,
        compiled.info.instr_count,
    );
}

/// Compile-only: storage buffer shader with integer arithmetic for SM35.
/// Exercises the carry_out fix (RegFile::Carry for Kepler).
#[test]
fn sovereign_compile_storage_buffer_sm35() {
    let wgsl = r#"
@group(0) @binding(0) var<storage, read_write> output: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    output[idx] = idx * 7u + 42u;
}
"#;

    let compiled = compile_wgsl(wgsl, coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm35));

    assert!(!compiled.binary.is_empty(), "SM35 storage buffer: binary should not be empty");
    assert!(
        compiled.info.gpr_count >= 2,
        "SM35 storage buffer: expected >=2 GPRs, got {}",
        compiled.info.gpr_count,
    );
    eprintln!(
        "SM35 storage buffer: {} bytes, {} GPRs, {} instrs — OK",
        compiled.binary.len(),
        compiled.info.gpr_count,
        compiled.info.instr_count,
    );
}

/// Compile-only: shader with bitwise ops and shifts for SM70.
/// Exercises Lop2 → Lop3, Shl → Shf, Shr → Shf legalization.
#[test]
fn sovereign_compile_bitwise_shift_sm70() {
    let wgsl = r#"
@group(0) @binding(0) var<storage, read_write> output: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let a = idx & 0xFFu;
    let b = idx | 0x100u;
    let c = a ^ b;
    let d = c << 2u;
    let e = d >> 1u;
    output[idx] = e;
}
"#;

    let compiled = compile_wgsl(wgsl, coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm70));

    assert!(!compiled.binary.is_empty(), "SM70 bitwise/shift: binary should not be empty");
    eprintln!(
        "SM70 bitwise/shift: {} bytes, {} GPRs, {} instrs — OK",
        compiled.binary.len(),
        compiled.info.gpr_count,
        compiled.info.instr_count,
    );
}

/// Compile-only: input+output shader for SM70 and SM35.
/// Full I/O pattern: read from one buffer, compute, write to another.
#[test]
fn sovereign_compile_input_output_sm70_and_sm35() {
    let wgsl = r#"
@group(0) @binding(0) var<storage, read> input: array<u32>;
@group(0) @binding(1) var<storage, read_write> output: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    output[idx] = input[idx] + 1u;
}
"#;

    for (arch, sm) in [
        (coral_reef::NvArch::Sm70, 70u32),
        (coral_reef::NvArch::Sm35, 35),
    ] {
        let compiled = compile_wgsl(wgsl, coral_reef::GpuTarget::Nvidia(arch));

        assert!(
            !compiled.binary.is_empty(),
            "SM{sm} input+output: binary should not be empty",
        );
        eprintln!(
            "SM{sm} input+output: {} bytes, {} GPRs, {} instrs — OK",
            compiled.binary.len(),
            compiled.info.gpr_count,
            compiled.info.instr_count,
        );
    }
}

// ---------------------------------------------------------------------------
// Forward architecture validation — sovereign path on newer NVIDIA + AMD
// ---------------------------------------------------------------------------

/// Compile-only: validate sovereign pipeline on SM89 (Ada / RTX 4090) and
/// SM120 (Blackwell / RTX 5090). Both share the SM70 encoder with the new
/// pre-Volta op legalization, confirming forward portability without vendor code.
#[test]
fn sovereign_compile_forward_nvidia_arches() {
    let wgsl = r#"
@group(0) @binding(0) var<storage, read_write> output: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let a = idx * 7u + 42u;
    let b = a & 0xFFu;
    let c = b << 2u;
    output[idx] = c | 1u;
}
"#;

    for (arch, label) in [
        (coral_reef::NvArch::Sm89, "SM89/Ada"),
        (coral_reef::NvArch::Sm120, "SM120/Blackwell"),
    ] {
        let compiled = compile_wgsl(wgsl, coral_reef::GpuTarget::Nvidia(arch));

        assert!(
            !compiled.binary.is_empty(),
            "{label}: binary should not be empty",
        );
        assert!(
            compiled.info.gpr_count >= 2,
            "{label}: expected >=2 GPRs, got {}",
            compiled.info.gpr_count,
        );
        eprintln!(
            "{label}: {} bytes, {} GPRs, {} instrs — OK",
            compiled.binary.len(),
            compiled.info.gpr_count,
            compiled.info.instr_count,
        );
    }
}

/// Compile-only: validate sovereign pipeline on AMD RDNA2 (GFX1030 / RX 6950 XT).
/// Confirms the architecture-neutral IR from coral-parse also works with the AMD
/// encoder path (no NVIDIA-specific assumptions leak from the frontend).
///
/// Uses a NOP shader — the AMD encoder has known gaps for storage buffer ops
/// (pre-existing, not related to the sovereign frontend). Full AMD buffer I/O
/// coverage will come as the RDNA2 encoder evolves.
#[test]
fn sovereign_compile_amd_rdna2() {
    let wgsl = "@compute @workgroup_size(64) fn main() {}";

    let compiled = compile_wgsl(
        wgsl,
        coral_reef::GpuTarget::Amd(coral_reef::AmdArch::Rdna2),
    );

    assert!(
        !compiled.binary.is_empty(),
        "RDNA2: binary should not be empty",
    );
    eprintln!(
        "RDNA2/GFX1030: {} bytes, {} GPRs, {} instrs — OK",
        compiled.binary.len(),
        compiled.info.gpr_count,
        compiled.info.instr_count,
    );
}
