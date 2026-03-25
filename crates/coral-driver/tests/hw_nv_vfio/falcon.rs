// SPDX-License-Identifier: AGPL-3.0-only

use crate::helpers::{init_tracing, open_vfio};
use coral_driver::{ComputeDevice, DispatchDims, ShaderInfo};

/// Exp 080: Sovereign FECS boot + compute dispatch.
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

    let results = dev.falcon_boot_solver().expect("solver should not panic");

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

/// Exp 091c: FLR → direct host firmware upload → STARTCPU.
///
/// LS mode (sctl=0x3000) blocks host STARTCPU. FLR clears sctl to 0,
/// enabling direct host-driven GPCCS/FECS boot without SEC2/ACR.
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

    // Step 1: GPU reset to clear falcon security state (sctl)
    // Try PCI D3→D0 power cycle first (resets all GPU engines)
    eprintln!("\n--- Step 1a: PCI D3→D0 power cycle ---");
    match dev.pci_power_cycle() {
        Ok((before, after)) => eprintln!("Power cycle: D{before} → D3 → D{after}"),
        Err(e) => eprintln!("Power cycle failed: {e}"),
    }
    std::thread::sleep(std::time::Duration::from_millis(200));

    let gpccs_sctl_pd3 = bar0.read_u32(0x41a240).unwrap_or(0xDEAD);
    eprintln!("GPCCS sctl after D3→D0: {gpccs_sctl_pd3:#010x}");

    if gpccs_sctl_pd3 == 0x3000 {
        // D3→D0 didn't clear sctl, try FLR
        eprintln!("\n--- Step 1b: VFIO FLR ---");
        match dev.device_reset() {
            Ok(()) => eprintln!("FLR succeeded"),
            Err(e) => eprintln!("FLR failed: {e} — continuing anyway"),
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    let post_flr = dev.falcon_probe();
    eprintln!("After FLR:\n{post_flr}");
    // Read sctl directly — falcon probe doesn't include it
    let gpccs_sctl = bar0.read_u32(0x41a240).unwrap_or(0xDEAD);
    let fecs_sctl = bar0.read_u32(0x409240).unwrap_or(0xDEAD);
    eprintln!("GPCCS sctl={gpccs_sctl:#010x} (want 0x0)");
    eprintln!("FECS sctl={fecs_sctl:#010x} (want 0x0)");

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

/// Exp 091d: Direct ACR IMEM load — bypass BL DMA.
///
/// The BL faults on DMA (exci=0x201f0007). Instead, load ACR firmware
/// directly into SEC2 IMEM via PIO while the ROM is idle.
///
/// Flow:
/// 0. Primer (Strategy 1) — gets SEC2 to ROM idle (cpuctl=0x10)
/// 1. Build VRAM WPR + page tables (for ACR to find FECS/GPCCS firmware)
/// 2. Upload ACR non_sec_code to SEC2 IMEM[0] via PIO
/// 3. Upload patched ACR data section to SEC2 DMEM[0]
/// 4. BOOTVEC=0, STARTCPU — ACR starts directly, no BL DMA needed
/// 5. BOOTSTRAP_FALCON via mailbox
/// 6. FECS method probe
#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn vfio_fecs_acr_boot_and_probe() {
    use coral_driver::nv::vfio_compute::acr_boot;
    use coral_driver::vfio::memory::{MemoryRegion, PraminRegion};

    // Falcon register offsets (not re-exported from the crate)
    const SEC2_BASE: usize = 0x087000;
    const FECS_BASE: usize = 0x409000;
    const GPCCS_BASE: usize = 0x41a000;
    const CPUCTL: usize = 0x100;
    const BOOTVEC: usize = 0x104;
    const DMACTL: usize = 0x10C;
    const MAILBOX0: usize = 0x040;
    const MAILBOX1: usize = 0x044;
    const EXCI: usize = 0x148;
    const IMEMC: usize = 0x180;
    const IMEMD: usize = 0x184;
    // Bit 4 (0x10): Despite the constant name CPUCTL_HRESET in the crate,
    // this is actually the HALTED bit — nouveau's wait_for_halt checks it.
    const CPUCTL_HRESET: u32 = 0x10;

    init_tracing();
    let dev = open_vfio();
    let bar0 = dev.bar0_ref();

    eprintln!("\n=== Exp 091e: Direct ACR IMEM Load (FLR clean slate) ===\n");

    // Step 0: VFIO device reset (FLR) — clears ALL falcon state including
    // secure mode. PMC_ENABLE bit 22 (SEC2) is hardware-locked on GV100 and
    // cannot be toggled, so FLR is the only reliable full reset.
    eprintln!("--- Step 0: VFIO device reset (FLR) ---");
    match dev.vfio_device_reset() {
        Ok(()) => eprintln!("FLR OK"),
        Err(e) => eprintln!("FLR failed: {e} — proceeding with existing state"),
    }
    std::thread::sleep(std::time::Duration::from_millis(100));

    let probe = dev.falcon_probe();
    eprintln!("After FLR:\n{probe}");

    // Step 0b: Build VRAM page tables (identity map first 2MB).
    // Must be done AFTER FLR (VRAM may be cleared) and BEFORE bind.
    eprintln!("\n--- Step 0b: Build VRAM page tables ---");
    let pt_ok = acr_boot::build_vram_falcon_inst_block(bar0);
    eprintln!("Page tables built: ok={pt_ok}");

    // Step 1: Parse ACR firmware and set up VRAM
    eprintln!("\n--- Step 1: Parse ACR + build VRAM WPR ---");
    let chip = "gv100";
    let fw = acr_boot::AcrFirmwareSet::load(chip).expect("firmware load");
    let parsed = acr_boot::ParsedAcrFirmware::parse(&fw).expect("ACR parse");
    eprintln!(
        "ACR: non_sec=[{:#x}+{:#x}] data=[{:#x}+{:#x}]",
        parsed.load_header.non_sec_code_off,
        parsed.load_header.non_sec_code_size,
        parsed.load_header.data_dma_base,
        parsed.load_header.data_size,
    );

    let wpr_vram_base: u32 = 0x0007_0000;
    let wpr_data = acr_boot::build_wpr(&fw, wpr_vram_base as u64);
    let wpr_end = wpr_vram_base as u64 + wpr_data.len() as u64;
    eprintln!("WPR: {}B at VRAM {wpr_vram_base:#x}..{wpr_end:#x}", wpr_data.len());

    // Write WPR to VRAM via PRAMIN
    let mut off = 0usize;
    while off < wpr_data.len() {
        let chunk_vram = wpr_vram_base + off as u32;
        let chunk_size = (wpr_data.len() - off).min(0xC000);
        if let Ok(mut region) = PraminRegion::new(bar0, chunk_vram, chunk_size) {
            for word_off in (0..chunk_size).step_by(4) {
                let src = off + word_off;
                if src >= wpr_data.len() { break; }
                let end = (src + 4).min(wpr_data.len());
                let mut bytes = [0u8; 4];
                bytes[..end - src].copy_from_slice(&wpr_data[src..end]);
                let _ = region.write_u32(word_off, u32::from_le_bytes(bytes));
            }
        }
        off += chunk_size;
    }
    eprintln!("WPR written to VRAM");

    // Step 1b: Fresh reset + rebind — clears primer's exception state,
    // re-establishes MMU binding with full nouveau-style bind sequence,
    // and configures DMA. IMEM/DMEM are clean after this.
    eprintln!("\n--- Step 1b: Fresh reset + rebind (sec2_prepare_direct_boot) ---");
    let (bind_ok, prep_notes) = acr_boot::sec2_prepare_direct_boot(bar0);
    for note in &prep_notes {
        eprintln!("  {note}");
    }
    eprintln!("Bind: ok={bind_ok}");

    let sec2_post_prep = bar0.read_u32(SEC2_BASE + CPUCTL).unwrap_or(0xDEAD);
    let sec2_pc_prep = bar0.read_u32(SEC2_BASE + 0x030).unwrap_or(0xDEAD);
    eprintln!("After prepare: cpuctl={sec2_post_prep:#010x} pc={sec2_pc_prep:#06x}");

    // Step 2: Upload ACR firmware to SEC2 IMEM/DMEM (IMEM/DMEM are clean from reset)
    eprintln!("\n--- Step 2: Upload ACR firmware to SEC2 IMEM/DMEM ---");
    let base = SEC2_BASE;

    let non_sec_off = parsed.load_header.non_sec_code_off as usize;
    let non_sec_size = parsed.load_header.non_sec_code_size as usize;
    let non_sec_end = (non_sec_off + non_sec_size).min(parsed.acr_payload.len());
    let non_sec_code = &parsed.acr_payload[non_sec_off..non_sec_end];
    eprintln!("non_sec_code: {}B [{non_sec_off:#x}..{non_sec_end:#x}]", non_sec_code.len());

    let data_off = parsed.load_header.data_dma_base as usize;
    let data_size = parsed.load_header.data_size as usize;
    let data_end = (data_off + data_size).min(parsed.acr_payload.len());

    let mut patched_payload = parsed.acr_payload.clone();
    acr_boot::patch_acr_desc(&mut patched_payload, data_off, wpr_vram_base as u64, wpr_end);
    let data_section = &patched_payload[data_off..data_end];
    eprintln!("data_section: {}B [{data_off:#x}..{data_end:#x}] (patched WPR bounds)", data_section.len());

    acr_boot::falcon_imem_upload_nouveau(bar0, base, 0, non_sec_code, 0);

    if let Some(&(sec_off, sec_size)) = parsed.load_header.apps.first() {
        let sec_off = sec_off as usize;
        let sec_end = (sec_off + sec_size as usize).min(parsed.acr_payload.len());
        let sec_code = &parsed.acr_payload[sec_off..sec_end];
        let imem_addr = non_sec_size as u32;
        let start_tag = (non_sec_size / 256) as u32;
        acr_boot::falcon_imem_upload_nouveau(bar0, base, imem_addr, sec_code, start_tag);
        eprintln!("sec_code: {}B → IMEM@{imem_addr:#x} tag={start_tag:#x}", sec_code.len());
    }

    let _ = bar0.write_u32(base + IMEMC, 0x0200_0000);
    let imem_word0 = bar0.read_u32(base + IMEMD).unwrap_or(0);
    let expected_word0 = if non_sec_code.len() >= 4 {
        u32::from_le_bytes([non_sec_code[0], non_sec_code[1], non_sec_code[2], non_sec_code[3]])
    } else { 0 };
    let imem_ok = imem_word0 == expected_word0;
    eprintln!("IMEM[0] verify: read={imem_word0:#010x} expected={expected_word0:#010x} ok={imem_ok}");

    acr_boot::falcon_dmem_upload(bar0, base, 0, data_section);
    eprintln!("ACR data → DMEM[0]");

    // Step 3: Configure DMA + STARTCPU
    eprintln!("\n--- Step 3: STARTCPU ---");

    // If the bind failed (stuck at state 2), try physical DMA mode.
    // FBIF_TRANSCFG controls how DMA addresses are routed:
    //   bits[1:0] = mode: 0=VIRT (needs bind+page tables), 1=PHYS_VID (physical VRAM)
    // Write PHYS_VID mode to BOTH DMA port configs (0x624 and 0x628)
    // so the ACR firmware accesses the WPR at physical VRAM addresses.
    if !bind_ok {
        eprintln!("Bind failed — configuring physical DMA mode as fallback");

        // Dump all FBIF_TRANSCFG ports (0x624 + n*4 for n=0..7)
        for port in 0..8u32 {
            let off = 0x624 + port as usize * 4;
            let val = bar0.read_u32(base + off).unwrap_or(0xDEAD);
            if val != 0 && val != 0xDEAD {
                eprint!("  FBIF[{port}]@{off:#x}={val:#010x}");
            }
        }
        eprintln!();

        // Set PHYS_VID (bits[1:0]=01) on all FBIF_TRANSCFG ports
        for port in 0..8u32 {
            let off = 0x624 + port as usize * 4;
            let before = bar0.read_u32(base + off).unwrap_or(0);
            let _ = bar0.write_u32(base + off, (before & !0x03) | 0x01);
        }

        // Enable DMA: bit 0 = DMA enable
        let _ = bar0.write_u32(base + DMACTL, 0x01);

        // Enable ITFEN ACCESS_EN (bit 0 of 0x048) — belt and suspenders
        let itfen = bar0.read_u32(base + 0x048).unwrap_or(0);
        let _ = bar0.write_u32(base + 0x048, itfen | 0x01);

        // Readback diagnostics
        let cfg0 = bar0.read_u32(base + 0x624).unwrap_or(0xDEAD);
        let dmactl = bar0.read_u32(base + DMACTL).unwrap_or(0xDEAD);
        let itfen_rb = bar0.read_u32(base + 0x048).unwrap_or(0xDEAD);
        eprintln!(
            "After phys DMA setup: FBIF[0]={cfg0:#010x} DMACTL={dmactl:#010x} ITFEN={itfen_rb:#010x}"
        );
    }

    // If CPU is already running (cpuctl bit 4 = 0), HALT it first.
    // Writing STARTCPU to a running CPU doesn't restart from BOOTVEC.
    let cpuctl_pre = bar0.read_u32(base + CPUCTL).unwrap_or(0xDEAD);
    if cpuctl_pre & CPUCTL_HRESET == 0 && cpuctl_pre != 0xDEAD_DEAD {
        eprintln!("CPU running (cpuctl={cpuctl_pre:#010x}), halting before BOOTVEC/STARTCPU");
        // 0x3C0 local reset pulse to halt + clear exception state
        let _ = bar0.write_u32(base + 0x3C0, 0x01);
        std::thread::sleep(std::time::Duration::from_micros(10));
        let _ = bar0.write_u32(base + 0x3C0, 0x00);
        // Wait for halt (bit 4)
        let halt_start = std::time::Instant::now();
        loop {
            let c = bar0.read_u32(base + CPUCTL).unwrap_or(0);
            if c & CPUCTL_HRESET != 0 {
                eprintln!("CPU halted: cpuctl={c:#010x} ({:?})", halt_start.elapsed());
                break;
            }
            if halt_start.elapsed() > std::time::Duration::from_millis(500) {
                eprintln!("Halt timeout: cpuctl={c:#010x}");
                break;
            }
            std::thread::sleep(std::time::Duration::from_micros(100));
        }
        // Re-upload firmware after reset (IMEM/DMEM cleared by scrub)
        eprintln!("Re-uploading ACR firmware after halt...");
        acr_boot::falcon_imem_upload_nouveau(bar0, base, 0, non_sec_code, 0);
        if let Some(&(sec_off, sec_size)) = parsed.load_header.apps.first() {
            let sec_off = sec_off as usize;
            let sec_end = (sec_off + sec_size as usize).min(parsed.acr_payload.len());
            let sec_code = &parsed.acr_payload[sec_off..sec_end];
            let imem_addr = non_sec_size as u32;
            let start_tag = (non_sec_size / 256) as u32;
            acr_boot::falcon_imem_upload_nouveau(bar0, base, imem_addr, sec_code, start_tag);
        }
        acr_boot::falcon_dmem_upload(bar0, base, 0, data_section);
        let verify = bar0.read_u32(base + IMEMD).unwrap_or(0);
        eprintln!("Post-halt IMEM verify (from last read pos): {verify:#010x}");
    }

    let _ = bar0.write_u32(base + MAILBOX0, 0);
    let _ = bar0.write_u32(base + MAILBOX1, 0);
    let _ = bar0.write_u32(base + BOOTVEC, 0);
    eprintln!("BOOTVEC=0, issuing STARTCPU");

    acr_boot::falcon_start_cpu(bar0, base);

    // Poll for ACR to settle.
    // Bit 4 (0x10, our CPUCTL_HRESET constant) is actually the HALTED bit
    // on GV100 — nouveau's nvkm_falcon_v1_wait_for_halt checks this same bit.
    let start = std::time::Instant::now();
    let mut last_pc = 0u32;
    let mut settled = 0u32;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let cpuctl = bar0.read_u32(base + CPUCTL).unwrap_or(0xDEAD);
        let mb0 = bar0.read_u32(base + MAILBOX0).unwrap_or(0);
        let pc = bar0.read_u32(base + 0x030).unwrap_or(0);
        let exci = bar0.read_u32(base + EXCI).unwrap_or(0);

        if pc != last_pc { last_pc = pc; settled = 0; } else { settled += 1; }

        if mb0 != 0 || cpuctl & CPUCTL_HRESET != 0 {
            eprintln!(
                "SEC2 response: cpuctl={cpuctl:#010x} mb0={mb0:#010x} pc={pc:#06x} exci={exci:#010x} ({}ms)",
                start.elapsed().as_millis()
            );
            break;
        }
        if settled > 100 {
            eprintln!(
                "SEC2 settled: cpuctl={cpuctl:#010x} mb0={mb0:#010x} pc={pc:#06x} exci={exci:#010x} ({}ms)",
                start.elapsed().as_millis()
            );
            break;
        }
        if start.elapsed() > std::time::Duration::from_secs(5) {
            eprintln!(
                "SEC2 timeout: cpuctl={cpuctl:#010x} mb0={mb0:#010x} pc={pc:#06x} exci={exci:#010x}"
            );
            break;
        }
    }

    // Post-STARTCPU diagnostics
    let bind_stat_post = bar0.read_u32(base + 0x0dc).unwrap_or(0xDEAD);
    let dmactl_post = bar0.read_u32(base + DMACTL).unwrap_or(0xDEAD);
    let reg624_post = bar0.read_u32(base + 0x624).unwrap_or(0xDEAD);
    eprintln!(
        "Post-start DMA state: bind_stat={bind_stat_post:#010x} bits[14:12]={} DMACTL={dmactl_post:#010x} 0x624={reg624_post:#010x}",
        (bind_stat_post >> 12) & 0x7
    );

    let probe3 = dev.falcon_probe();
    eprintln!("After ACR start:\n{probe3}");

    // Step 4: BOOTSTRAP_FALCON via mailbox.
    // SEC2 is "alive" if not halted (bit 4 clear) and not a PRI error.
    eprintln!("\n--- Step 4: BOOTSTRAP_FALCON ---");
    let sec2_cpuctl = bar0.read_u32(base + CPUCTL).unwrap_or(0xDEAD);
    let sec2_mb0 = bar0.read_u32(base + MAILBOX0).unwrap_or(0);
    let sec2_exci = bar0.read_u32(base + EXCI).unwrap_or(0);
    let sec2_alive = sec2_cpuctl & CPUCTL_HRESET == 0 && sec2_cpuctl != 0xDEAD_DEAD;
    eprintln!(
        "SEC2 alive for BOOTSTRAP: {sec2_alive} (cpuctl={sec2_cpuctl:#010x} mb0={sec2_mb0:#010x} exci={sec2_exci:#010x})"
    );

    if sec2_alive {
        let r4 = acr_boot::attempt_acr_mailbox_command(bar0);
        eprintln!("{r4}");
    } else if sec2_mb0 != 0 {
        eprintln!("SEC2 halted with mb0={sec2_mb0:#010x} — ACR error code, trying BOOTSTRAP anyway");
        let r4 = acr_boot::attempt_acr_mailbox_command(bar0);
        eprintln!("{r4}");
    }

    // Step 5: Check FECS state
    let probe5 = dev.falcon_probe();
    eprintln!("\nFinal state:\n{probe5}");

    let fecs_running = probe5.fecs_cpuctl & CPUCTL_HRESET == 0
        && probe5.fecs_cpuctl != 0xDEAD_DEAD;

    if fecs_running {
        eprintln!("\n--- Step 6: FECS method probe ---");
        acr_boot::fecs_method::fecs_init_exceptions(bar0);
        let mprobe = acr_boot::fecs_method::fecs_probe_methods(bar0);
        eprintln!("{mprobe}");

        if mprobe.ctx_size.is_ok() {
            eprintln!("\n****************************************************");
            eprintln!("*  FECS METHOD INTERFACE RESPONDING!                *");
            eprintln!("*  GR engine is accessible — Layer 11 unblocked!   *");
            eprintln!("****************************************************");
        }
    } else {
        eprintln!("FECS not running (cpuctl={:#010x})", probe5.fecs_cpuctl);

        let _ = bar0.write_u32(GPCCS_BASE + IMEMC, 0x0200_0000);
        let gpccs_imem0 = bar0.read_u32(GPCCS_BASE + IMEMD).unwrap_or(0);
        let _ = bar0.write_u32(FECS_BASE + IMEMC, 0x0200_0000);
        let fecs_imem0 = bar0.read_u32(FECS_BASE + IMEMD).unwrap_or(0);
        eprintln!("GPCCS IMEM[0]={gpccs_imem0:#010x} FECS IMEM[0]={fecs_imem0:#010x}");
    }

    eprintln!("\n=== End Exp 091e ===");
}
