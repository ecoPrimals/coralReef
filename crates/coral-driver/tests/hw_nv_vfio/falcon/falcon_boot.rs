// SPDX-License-Identifier: AGPL-3.0-only

use crate::helpers::{init_tracing, open_vfio};
use coral_driver::{ComputeDevice, DispatchDims, ShaderInfo};

///
/// Loads FECS firmware directly into the falcon IMEM/DMEM ports,
/// bypassing ACR secure boot. If FECS reports `secure=false` (as
/// discovered in Exp 078), the firmware should load and execute.
///
/// After FECS boot, attempts a full compute dispatch. If sync
/// succeeds, we have achieved full sovereign GPU compute.
#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn vfio_sovereign_fecs_boot() {
    let mut dev = open_vfio();
    eprintln!("\n=== Sovereign FECS Boot (Exp 080) ===\n");

    let diag_pre = dev.layer7_diagnostics("SOVEREIGN-PRE");
    eprintln!("FECS state before boot:");
    eprintln!(
        "  cpuctl={:#010x} mailbox0={:#010x} secure={}",
        diag_pre.fecs.cpuctl,
        diag_pre.fecs.mailbox0,
        diag_pre.fecs.requires_signed_firmware()
    );

    if !diag_pre.fecs.is_in_reset() {
        eprintln!("FECS not in HRESET — already running? Checking mailbox...");
        if diag_pre.fecs.mailbox0 != 0 {
            eprintln!(
                "  FECS firmware active (mailbox0={:#010x}), skipping boot.",
                diag_pre.fecs.mailbox0
            );
        }
    }

    eprintln!("\n── Sovereign FECS Boot Attempt ──");
    match dev.sovereign_fecs_boot() {
        Ok(result) => {
            eprintln!("{result}");
            if result.running {
                eprintln!("\n*** FECS firmware is RUNNING! ***\n");
            } else if result.mailbox0 != 0 {
                eprintln!(
                    "\nFECS responded (mb0={:#010x}) but may not be fully running.",
                    result.mailbox0
                );
            } else {
                eprintln!("\nFECS did not respond — boot may have failed.");
                eprintln!(
                    "  cpuctl={:#010x} — check if halted or still in reset.",
                    result.cpuctl_after
                );
            }
        }
        Err(e) => {
            eprintln!("FECS boot error: {e}");
            let diag_fail = dev.layer7_diagnostics("SOVEREIGN-BOOT-FAIL");
            eprintln!("\n{diag_fail}");
            eprintln!("\n=== End Sovereign FECS Boot (FAILED) ===");
            return;
        }
    }

    // Also try GPCCS.
    eprintln!("\n── Sovereign GPCCS Boot Attempt ──");
    match coral_driver::nv::vfio_compute::fecs_boot::boot_gpccs(dev.bar0_ref(), "gv100") {
        Ok(result) => eprintln!("{result}"),
        Err(e) => eprintln!("GPCCS boot: {e}"),
    }

    let diag_post = dev.layer7_diagnostics("SOVEREIGN-POST-BOOT");
    eprintln!("\n{diag_post}");

    // Attempt dispatch if FECS appears running.
    let fecs_post = &diag_post.fecs;
    let fecs_alive = !fecs_post.is_in_reset() && !fecs_post.is_halted() || fecs_post.mailbox0 != 0;

    if !fecs_alive {
        eprintln!("\nFECS not running after boot — skipping dispatch attempt.");
        eprintln!("=== End Sovereign FECS Boot (no dispatch) ===");
        return;
    }

    eprintln!("\n── Dispatch attempt with sovereign-booted FECS ──");
    let sm = dev.sm_version();
    let wgsl = "@compute @workgroup_size(64) fn main() {}";
    let opts = coral_reef::CompileOptions {
        target: match sm {
            70 => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm70),
            75 => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm75),
            80 => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm80),
            _ => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm86),
        },
        ..coral_reef::CompileOptions::default()
    };
    let compiled = coral_reef::compile_wgsl_full(wgsl, &opts).expect("compile");
    let info = ShaderInfo {
        gpr_count: compiled.info.gpr_count,
        shared_mem_bytes: compiled.info.shared_mem_bytes,
        barrier_count: compiled.info.barrier_count,
        workgroup: compiled.info.local_size,
        wave_size: 32,
    };

    match dev.dispatch_traced(&compiled.binary, &[], DispatchDims::linear(1), &info) {
        Ok(captures) => {
            eprintln!("\nTimed post-doorbell captures:");
            for cap in &captures {
                eprintln!("{cap}");
            }
        }
        Err(e) => eprintln!("dispatch_traced: {e}"),
    }

    eprintln!("\n── Sync attempt ──");
    match dev.sync() {
        Ok(()) => {
            eprintln!("****************************************************");
            eprintln!("*  SYNC SUCCEEDED — SOVEREIGN COMPUTE ACHIEVED!    *");
            eprintln!("*  FECS firmware loaded directly via DMA upload.    *");
            eprintln!("*  No external driver dependency.                   *");
            eprintln!("****************************************************");
        }
        Err(e) => {
            eprintln!("sync failed: {e}");
            let diag_timeout = dev.layer7_diagnostics("SOVEREIGN-POST-TIMEOUT");
            eprintln!("\n{diag_timeout}");
        }
    }

    eprintln!("\n=== End Sovereign FECS Boot ===");
}

// ── Experiment 081: Falcon Boot Solver ──────────────────────────

#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn vfio_sec2_probe() {
    init_tracing();
    let dev = open_vfio();

    eprintln!("\n=== Exp 081: SEC2 Probe (corrected base 0x087000) ===\n");

    let probe = dev.falcon_probe();
    eprintln!("{probe}");

    let sec2 = dev.sec2_probe();
    eprintln!("\nDetailed SEC2:\n{sec2}");

    eprintln!("\nSEC2 state classification: {:?}", sec2.state);
    eprintln!(
        "  HS-locked: {}",
        sec2.state == coral_driver::nv::vfio_compute::acr_boot::Sec2State::HsLocked
    );
    eprintln!(
        "  Clean reset: {}",
        sec2.state == coral_driver::nv::vfio_compute::acr_boot::Sec2State::CleanReset
    );

    // EMEM accessibility test
    let bar0 = dev.bar0_ref();
    let test_data = [0x42u8, 0x43, 0x44, 0x45];
    coral_driver::nv::vfio_compute::acr_boot::sec2_emem_write(bar0, 0, &test_data);
    let readback = coral_driver::nv::vfio_compute::acr_boot::sec2_emem_read(bar0, 0, 4);
    eprintln!("\nEMEM write/read test:");
    eprintln!("  wrote: {:02x?}", test_data);
    eprintln!("  read:  {:#010x}", readback.first().copied().unwrap_or(0));
    let expected = u32::from_le_bytes(test_data);
    let emem_ok = readback.first().copied() == Some(expected);
    eprintln!("  match: {emem_ok}");

    eprintln!("\n=== End SEC2 Probe ===");
}

#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn vfio_falcon_boot_solver() {
    init_tracing();
    let dev = open_vfio();

    eprintln!("\n=== Exp 081: Falcon Boot Solver (all strategies) ===\n");

    let results = dev
        .falcon_boot_solver(None)
        .expect("solver should not panic");

    eprintln!("Solver returned {} result(s):", results.len());
    for (i, result) in results.iter().enumerate() {
        eprintln!("\n── Strategy {} ──\n{result}", i + 1);
    }

    // Post-solver diagnostic
    let probe = dev.falcon_probe();
    eprintln!("\n── Post-solver falcon state ──\n{probe}");

    // Check for success
    let any_success = results.iter().any(|r| r.success);
    if any_success {
        eprintln!("\n****************************************************");
        eprintln!("*  FALCON BOOT SOLVER SUCCEEDED!                   *");
        eprintln!("*  FECS is running — GR engine should be ready.    *");
        eprintln!("****************************************************");
    } else {
        eprintln!("\nNo strategy achieved FECS boot.");
        eprintln!("Full ACR WPR chain (080b-d) needed for sovereign boot.");
    }

    let diag = dev.layer7_diagnostics("POST-SOLVER");
    eprintln!("\n{diag}");

    eprintln!("\n=== End Falcon Boot Solver ===");
}

/// Exp 083: System-memory ACR boot — WPR/inst/page tables in IOMMU DMA.
#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn vfio_sysmem_acr_boot() {
    init_tracing();
    let dev = open_vfio();

    eprintln!("\n=== Exp 083: System-Memory ACR Boot ===\n");

    let pre = dev.falcon_probe();
    eprintln!("Pre-boot falcon state:\n{pre}");

    // 083a: Pure system memory (all DMA buffers)
    let result_a = dev.sysmem_acr_boot();
    eprintln!("\n── 083a: Pure SysMem ──\n{result_a}");

    // 083b: Hybrid (VRAM page tables + sysmem data)
    let result_b = dev.hybrid_acr_boot();
    eprintln!("\n── 083b: Hybrid (VRAM PT + SysMem data) ──\n{result_b}");

    let post = dev.falcon_probe();
    eprintln!("\nPost-boot falcon state:\n{post}");

    if result_a.success || result_b.success {
        eprintln!("\n** ACR BOOT SUCCEEDED — FECS running! **");
    }

    eprintln!("\n=== End Exp 083 ===");
}

/// Test: VFIO FLR + PCI D3→D0 power cycle → check SEC2 state.
/// If either puts SEC2 into HRESET, we can boot it with STARTCPU.
#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn vfio_flr_then_falcon_probe() {
    init_tracing();
    let dev = open_vfio();

    eprintln!("\n=== GPU Reset + Falcon Probe ===\n");

    let pre = dev.falcon_probe();
    eprintln!("Pre-reset:\n{pre}");

    // Try 1: VFIO device reset (FLR)
    eprintln!("\n--- Try 1: VFIO DEVICE_RESET (FLR) ---");
    match dev.device_reset() {
        Ok(()) => eprintln!("FLR succeeded"),
        Err(e) => eprintln!("FLR failed: {e}"),
    }
    std::thread::sleep(std::time::Duration::from_millis(100));
    let after_flr = dev.falcon_probe();
    eprintln!("After FLR:\n{after_flr}");
    eprintln!(
        "SEC2 cpuctl: {:#010x} HRESET={}",
        after_flr.sec2.cpuctl,
        after_flr.sec2.cpuctl & 0x10 != 0
    );

    // Try 2: PCI D3→D0 power cycle
    eprintln!("\n--- Try 2: PCI D3→D0 power cycle ---");
    match dev.pci_power_cycle() {
        Ok((before, after)) => {
            eprintln!("Power cycle: D{before} → D3 → D{after}");
        }
        Err(e) => eprintln!("Power cycle failed: {e}"),
    }
    std::thread::sleep(std::time::Duration::from_millis(200));
    let after_pm = dev.falcon_probe();
    eprintln!("After D3→D0:\n{after_pm}");
    eprintln!(
        "SEC2 cpuctl: {:#010x} HRESET={}",
        after_pm.sec2.cpuctl,
        after_pm.sec2.cpuctl & 0x10 != 0
    );

    eprintln!("\n=== End GPU Reset Probe ===");
}

/// Exp 091c: Direct host firmware upload → STARTCPU.
///
/// SCTL=0x3000 (LS mode) is fuse-enforced and does NOT block PIO or STARTCPU.
/// PIO works with correct IMEMC format (BIT(24) write, BIT(25) read).
/// FLR is not supported on Titan V (GV100 reports FLReset-).
#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn vfio_flr_direct_falcon_boot() {
    use coral_driver::nv::vfio_compute::acr_boot;

    init_tracing();
    let dev = open_vfio();
    let bar0 = dev.bar0_ref();

    eprintln!("\n=== Exp 091c: FLR + Direct Host Falcon Boot ===\n");

    let pre = dev.falcon_probe();
    eprintln!("Pre-FLR state:\n{pre}");

    // Step 1: Probe security state (informational — SCTL does NOT block PIO)
    eprintln!("\n--- Step 1: Security state probe ---");
    let gpccs_sctl = bar0.read_u32(0x41a240).unwrap_or(0xDEAD);
    let fecs_sctl = bar0.read_u32(0x409240).unwrap_or(0xDEAD);
    eprintln!("GPCCS sctl={gpccs_sctl:#010x} (LS=0x1000/0x3000 is normal, PIO works regardless)");
    eprintln!("FECS  sctl={fecs_sctl:#010x}");

    // D3→D0 power cycle to reset execution state (CPUCTL/EXCI), not for SCTL clearing
    eprintln!("\n--- Step 1b: PCI D3→D0 power cycle (reset execution state) ---");
    match dev.pci_power_cycle() {
        Ok((before, after)) => eprintln!("Power cycle: D{before} → D3 → D{after}"),
        Err(e) => eprintln!("Power cycle failed: {e}"),
    }
    std::thread::sleep(std::time::Duration::from_millis(200));

    let post_cycle = dev.falcon_probe();
    eprintln!("After power cycle:\n{post_cycle}");

    // Step 2: Load firmware
    eprintln!("\n--- Step 2: Load firmware ---");
    let chip = "gv100";
    let fw = acr_boot::AcrFirmwareSet::load(chip).expect("firmware load");
    eprintln!("{fw:?}");

    // Step 3: Direct host upload + start via the strategy function
    eprintln!("\n--- Step 3: Direct upload + STARTCPU ---");
    let result = acr_boot::attempt_direct_falcon_upload(bar0, &fw);
    eprintln!("{result}");

    // Step 4: Final state
    let final_state = dev.falcon_probe();
    eprintln!("\nFinal state:\n{final_state}");

    if result.success {
        eprintln!("\n****************************************************");
        eprintln!("*  DIRECT FALCON BOOT SUCCEEDED!                   *");
        eprintln!("*  GPCCS+FECS running — GR engine ready.           *");
        eprintln!("****************************************************");
    }

    eprintln!("\n=== End Exp 091c ===");
}
