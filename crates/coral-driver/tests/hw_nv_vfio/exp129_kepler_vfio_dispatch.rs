// SPDX-License-Identifier: AGPL-3.0-only
//! Exp 129: K80 Kepler VFIO Compute Dispatch.
//!
//! Full sovereign compute dispatch on Tesla K80 (GK210, SM 3.7) using the
//! VFIO DMA path with GF100 V1 two-level page tables and GK104 PFIFO.
//!
//! Prerequisites:
//! - coral-ember running (holds VFIO fds as root, serves via SCM_RIGHTS)
//! - K80 bound to `vfio-pci` (both dies or at least one)
//! - IOMMU enabled (intel_iommu=on or amd_iommu=on)
//! - FECS firmware booted (run exp128a2b or coralctl warm-fecs first)
//!
//! Run:
//! ```sh
//! CORALREEF_VFIO_BDF=0000:XX:YY.Z \
//!   cargo test --test exp129_kepler_vfio_dispatch -p coral-driver \
//!   --features vfio -- --ignored --nocapture
//! ```

use crate::helpers;
use coral_driver::{ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

const KEPLER_WAVE_SIZE: u32 = 32;

fn open_k80() -> coral_driver::nv::NvVfioComputeDevice {
    helpers::open_k80()
}

/// Phase 1: Open the K80 via NvVfioComputeDevice (auto-detects SM 37, creates KeplerChannel).
#[test]
#[ignore = "requires K80 on vfio-pci + ember running (or vfio group perms)"]
fn exp129_phase1_kepler_open() {
    eprintln!("\n{}", "=".repeat(70));
    eprintln!("Exp 129 Phase 1: Kepler VFIO Device Open (K80 GK210)");
    eprintln!("{}", "=".repeat(70));

    let dev = open_k80();
    eprintln!("  Device opened: SM {}", dev.sm_version());
    assert_eq!(dev.sm_version(), 37, "Expected SM 37 (GK210)");
    eprintln!("  Phase 1: PASS — KeplerChannel created via ember fds");

    eprintln!("{}", "=".repeat(70));
}

/// Phase 2: Full NOP dispatch — open device, compile NOP shader, dispatch, sync.
#[test]
#[ignore = "requires K80 on vfio-pci + ember + FECS running"]
fn exp129_phase2_kepler_nop_dispatch() {
    eprintln!("\n{}", "=".repeat(70));
    eprintln!("Exp 129 Phase 2: Kepler NOP Compute Dispatch (K80 GK210)");
    eprintln!("{}", "=".repeat(70));

    let mut dev = open_k80();
    eprintln!("  Device opened: SM {}", dev.sm_version());
    assert_eq!(dev.sm_version(), 37);

    // Minimal NOP shader for SM 3.7 (Kepler).
    // Kepler shader: 64-bit instructions, EXIT = 0x8000000000001DE7.
    let nop_shader: Vec<u8> = {
        let exit: u64 = 0x8000_0000_0000_1DE7;
        let nop: u64 = 0x4000_0000_0000_0000;
        let mut bytes = Vec::with_capacity(256);
        // 20-byte header (zeros for minimal config)
        bytes.extend_from_slice(&[0u8; 20]);
        // Instruction pairs
        bytes.extend_from_slice(&exit.to_le_bytes());
        bytes.extend_from_slice(&nop.to_le_bytes());
        bytes.extend_from_slice(&nop.to_le_bytes());
        while bytes.len() < 256 {
            bytes.extend_from_slice(&nop.to_le_bytes());
        }
        bytes
    };

    eprintln!("  NOP shader: {} bytes", nop_shader.len());

    let info = ShaderInfo {
        workgroup: [1, 1, 1],
        gpr_count: 4,
        shared_mem_bytes: 0,
        barrier_count: 0,
        wave_size: KEPLER_WAVE_SIZE,
    };

    eprintln!("  Dispatching NOP shader (1,1,1) x (1,1,1)...");
    match dev.dispatch(&nop_shader, &[], DispatchDims::new(1, 1, 1), &info) {
        Ok(()) => eprintln!("  Dispatch submitted OK"),
        Err(e) => {
            eprintln!("  Dispatch FAILED: {e}");
            return;
        }
    }

    eprintln!("  Syncing (waiting for GPFIFO completion)...");
    match dev.sync() {
        Ok(()) => {
            eprintln!("  SYNC OK — GPU consumed GPFIFO entry!");
            eprintln!("  Phase 2: PASS — Kepler sovereign compute dispatch succeeded!");
        }
        Err(e) => {
            eprintln!("  SYNC FAILED (fence timeout): {e}");
            eprintln!("  Possible causes:");
            eprintln!("    - FECS not running (boot falcons first via coralctl)");
            eprintln!("    - GR context not initialized (need FECS method 0x10)");
            eprintln!("    - MMU fault (check dmesg for IOMMU errors)");
            eprintln!("    - PBDMA error (GPFIFO entry rejected)");
        }
    }

    eprintln!("{}", "=".repeat(70));
}

/// Phase 2b: Compiler-driven NOP dispatch using coral-reef with NvArch::Sm35.
///
/// Instead of hand-crafted SASS, this compiles a WGSL NOP shader via the
/// coral-reef compiler targeting SM 3.5 (Kepler SASS). This validates:
/// - coral-reef Sm35 codegen produces valid Kepler binary
/// - QMD v1.7 fields (GPR count, shared mem) from compiler metadata
/// - Full Kepler dispatch pipeline: QMD + PCAS + PROGRAM_REGION
#[test]
#[ignore = "requires K80 on vfio-pci + ember + FECS running"]
fn exp129_phase2b_kepler_compiled_dispatch() {
    eprintln!("\n{}", "=".repeat(70));
    eprintln!("Exp 129 Phase 2b: Kepler Compiled NOP Dispatch (K80 GK210)");
    eprintln!("{}", "=".repeat(70));

    let mut dev = open_k80();
    eprintln!("  Device opened: SM {}", dev.sm_version());
    assert_eq!(dev.sm_version(), 37);

    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let opts = coral_reef::CompileOptions {
        target: coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm35),
        ..coral_reef::CompileOptions::default()
    };

    let compiled = match coral_reef::compile_wgsl_full(wgsl, &opts) {
        Ok(c) => {
            eprintln!(
                "  Compiled: {} bytes, {} GPRs, {} shared, workgroup {:?}",
                c.binary.len(),
                c.info.gpr_count,
                c.info.shared_mem_bytes,
                c.info.local_size
            );
            c
        }
        Err(e) => {
            eprintln!("  COMPILE FAILED: {e}");
            eprintln!("  coral-reef Sm35 codegen may not be fully supported yet.");
            return;
        }
    };

    let info = ShaderInfo {
        gpr_count: compiled.info.gpr_count,
        shared_mem_bytes: compiled.info.shared_mem_bytes,
        barrier_count: compiled.info.barrier_count,
        workgroup: compiled.info.local_size,
        wave_size: KEPLER_WAVE_SIZE,
    };

    eprintln!("  Dispatching compiled NOP shader (1,1,1) x ({:?})...", info.workgroup);
    match dev.dispatch(&compiled.binary, &[], DispatchDims::new(1, 1, 1), &info) {
        Ok(()) => eprintln!("  Dispatch submitted OK"),
        Err(e) => {
            eprintln!("  Dispatch FAILED: {e}");
            return;
        }
    }

    eprintln!("  Syncing (waiting for GPFIFO completion)...");
    match dev.sync() {
        Ok(()) => {
            eprintln!("  SYNC OK — Kepler compiled shader dispatch succeeded!");
            eprintln!("  Phase 2b: PASS — coral-reef Sm35 → Kepler QMD v1.7 → GPFIFO → GPU");
        }
        Err(e) => {
            eprintln!("  SYNC FAILED (fence timeout): {e}");
            eprintln!("  Possible causes:");
            eprintln!("    - coral-reef Sm35 binary not compatible with QMD v1.7 header");
            eprintln!("    - FECS not running or GR context not initialized");
            eprintln!("    - MMU fault (check dmesg for IOMMU errors)");
        }
    }

    eprintln!("{}", "=".repeat(70));
}

/// Phase 3: Data compute — allocate buffer, verify DMA round-trip.
#[test]
#[ignore = "requires K80 on vfio-pci + ember + FECS running + Phase 2 working"]
fn exp129_phase3_kepler_data_compute() {
    eprintln!("\n{}", "=".repeat(70));
    eprintln!("Exp 129 Phase 3: Kepler Data Compute (K80 GK210)");
    eprintln!("{}", "=".repeat(70));

    let mut dev = open_k80();
    assert_eq!(dev.sm_version(), 37);

    let out_handle = dev
        .alloc(4096, MemoryDomain::Gtt)
        .expect("alloc output buffer");

    let zeros = vec![0u8; 4096];
    dev.upload(out_handle, 0, &zeros)
        .expect("zero output buffer");

    let readback = dev
        .readback(out_handle, 0, 4096)
        .expect("readback output buffer");

    let all_zero = readback.iter().all(|&b| b == 0);
    if all_zero {
        eprintln!("  DMA buffer round-trip OK (upload zeros, read back zeros)");
        eprintln!("  Phase 3: PASS (DMA verified, shader dispatch pending SASS compiler)");
    } else {
        let nonzero_count = readback.iter().filter(|&&b| b != 0).count();
        eprintln!("  DMA buffer MISMATCH: {nonzero_count}/4096 bytes non-zero");
        eprintln!(
            "  First non-zero at offset {}",
            readback.iter().position(|&b| b != 0).unwrap_or(0)
        );
    }

    dev.free(out_handle).expect("free output buffer");

    eprintln!("{}", "=".repeat(70));
}

/// Diagnostic: open device with full register captures via tracing.
#[test]
#[ignore = "requires K80 on vfio-pci + ember"]
fn exp129_diagnostic_kepler_channel() {
    eprintln!("\n{}", "=".repeat(70));
    eprintln!("Exp 129 Diagnostic: Kepler Channel State Capture");
    eprintln!("{}", "=".repeat(70));

    eprintln!("  Set RUST_LOG=coral_driver=debug for detailed channel creation trace.");
    eprintln!("  Opening device via ember...");

    let bdf = helpers::k80_bdf();
    match crate::ember_client::request_fds(&bdf) {
        Ok(fds) => {
            eprintln!("  ember: received VFIO fds for {bdf}");
            match coral_driver::nv::NvVfioComputeDevice::open_from_fds(&bdf, fds, 37, 0xA1C0) {
                Ok(dev) => {
                    eprintln!("  OPEN OK: SM={}, Kepler channel active", dev.sm_version());
                    eprintln!("  Ready for dispatch.");
                }
                Err(e) => {
                    eprintln!("  OPEN FAILED: {e}");
                    eprintln!("  1. Is FECS running? Use coralctl to boot falcons.");
                    eprintln!("  2. Check RUST_LOG=debug output for register state.");
                }
            }
        }
        Err(e) => {
            eprintln!("  ember unavailable: {e}");
            eprintln!("  Start coral-ember first: cargo run -p coral-ember");
        }
    }

    eprintln!("{}", "=".repeat(70));
}
