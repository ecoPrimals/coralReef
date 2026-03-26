// SPDX-License-Identifier: AGPL-3.0-only

use crate::ember_client;
use crate::helpers::{init_tracing, open_vfio, vfio_bdf};
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

    let results = dev.falcon_boot_solver(None).expect("solver should not panic");

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
    acr_boot::patch_acr_desc(&mut patched_payload, data_off, wpr_vram_base as u64, wpr_end, wpr_vram_base as u64);
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
        // Verify IMEM[0] with proper read setup: BIT(25) for read mode
        let _ = bar0.write_u32(base + IMEMC, 0x0200_0000);
        let verify = bar0.read_u32(base + IMEMD).unwrap_or(0);
        eprintln!("Post-halt IMEM[0] verify: {verify:#010x}");
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

    let bootvec_offsets = acr_boot::FalconBootvecOffsets {
        gpccs: fw.gpccs_bl.bl_imem_off(),
        fecs: fw.fecs_bl.bl_imem_off(),
    };
    if sec2_alive {
        let r4 = acr_boot::attempt_acr_mailbox_command(bar0, &bootvec_offsets);
        eprintln!("{r4}");
    } else if sec2_mb0 != 0 {
        eprintln!("SEC2 halted with mb0={sec2_mb0:#010x} — ACR error code, trying BOOTSTRAP anyway");
        let r4 = acr_boot::attempt_acr_mailbox_command(bar0, &bootvec_offsets);
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

/// GR context lifecycle test — requires FECS to be running (warm handoff from nouveau).
///
/// Exercises the full chain: discover sizes → allocate → bind → golden save.
/// Swap to nouveau first to get FECS warm, then swap to VFIO and test.
#[test]
#[ignore = "requires VFIO-bound GPU hardware with warm FECS"]
fn vfio_gr_context_lifecycle() {
    use coral_driver::nv::vfio_compute::gr_context;

    init_tracing();
    let mut dev = open_vfio();
    let bar0 = dev.bar0_ref();

    eprintln!("\n=== GR Context Lifecycle Test ===\n");

    let alive = gr_context::fecs_is_alive(bar0);
    eprintln!("FECS alive: {alive}");

    if !alive {
        eprintln!("FECS not running — need warm handoff from nouveau first.");
        eprintln!("Run: coralctl swap <bdf> nouveau && coralctl swap <bdf> vfio");
        eprintln!("=== End GR Context Lifecycle (skipped) ===");
        return;
    }

    let status = dev.probe_gr_context();
    eprintln!("{status}");

    eprintln!("\n=== End GR Context Lifecycle ===");
}

/// Exp 092: Full sovereign GR falcon boot — no SEC2/ACR, no nouveau.
///
/// Loads fresh GPCCS+FECS firmware directly via IMEM PIO, applies
/// the complete init sequence (sw_nonctx + INTR_EN_SET + watchdog),
/// and waits for FECS to signal ready via CTXSW_MAILBOX.
///
/// Key fixes over earlier attempts:
/// - Boot GPCCS first (FECS depends on GPCCS)
/// - Set INTR_EN_SET at falcon_base + 0x00C (not 0x010 which is INTR_EN_CLR)
/// - Set watchdog timer (0x7FFFFFFF)
/// - Apply GR BAR0 init (sw_nonctx + dynamic) before starting falcons
#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn vfio_sovereign_gr_boot() {
    use coral_driver::nv::vfio_compute::fecs_boot;

    init_tracing();
    let dev = open_vfio();
    let bar0 = dev.bar0_ref();
    let chip = "gv100";

    eprintln!("\n=== Exp 092: Sovereign GR Boot (complete sequence) ===\n");

    let fecs_base: usize = 0x409000;
    let gpccs_base: usize = 0x41a000;
    let r = |addr: usize| bar0.read_u32(addr).unwrap_or(0xDEAD_DEAD);
    let w = |addr: usize, val: u32| { let _ = bar0.write_u32(addr, val); };

    // Step 0: Pre-boot diagnostics
    eprintln!("── Step 0: Pre-boot state ──");
    let fecs_cpuctl_pre = r(fecs_base + 0x100);
    let gpccs_cpuctl_pre = r(gpccs_base + 0x100);
    let fecs_sctl = r(fecs_base + 0x240);
    let gpccs_sctl = r(gpccs_base + 0x240);
    eprintln!("FECS:  cpuctl={fecs_cpuctl_pre:#010x} sctl={fecs_sctl:#010x}");
    eprintln!("GPCCS: cpuctl={gpccs_cpuctl_pre:#010x} sctl={gpccs_sctl:#010x}");

    // SCTL is fuse-enforced on GV100 (always LS=0x3000). This is informational —
    // PIO works regardless with correct IMEMC format (BIT(24) write, BIT(25) read).
    // FLR is not supported on Titan V (FLReset-).
    if fecs_sctl != 0 || gpccs_sctl != 0 {
        eprintln!("SCTL non-zero (LS mode) — this is normal for GV100. PIO works regardless.");
    }

    // Step 1: Clock gating restore + halt falcons
    // PMC_UNK260=1 is critical — matches nouveau's nvkm_mc_unk260(device, 1)
    // before FECS/GPCCS start. Without it, falcon clocks may be gated.
    eprintln!("\n── Step 1: PMC clock gate + halt falcons ──");
    let pmc_unk260_before = r(0x260);
    w(0x260, 1); // PMC_UNK260 = 1 (restore clock gating)
    let pmc_unk260_after = r(0x260);
    eprintln!("PMC_UNK260: {pmc_unk260_before:#010x} → {pmc_unk260_after:#010x}");

    for (name, base) in [("FECS", fecs_base), ("GPCCS", gpccs_base)] {
        let cpuctl = r(base + 0x100);
        let exci_before = r(base + 0x148);
        let pc_before = r(base + 0x030);
        if cpuctl & 0x10 == 0 && cpuctl != 0xDEAD_DEAD {
            w(base + 0x100, 0x10); // set HRESET
            std::thread::sleep(std::time::Duration::from_millis(5));
            let after = r(base + 0x100);
            eprintln!("{name}: halted (cpuctl {cpuctl:#010x} → {after:#010x}) pre-exci={exci_before:#010x} pre-pc={pc_before:#06x}");
        } else {
            eprintln!("{name}: already halted cpuctl={cpuctl:#010x} pre-exci={exci_before:#010x} pre-pc={pc_before:#06x}");
        }
    }

    // Step 2: Load + upload firmware with secure page tags
    // In LS mode (SCTL=0x3000), the falcon may validate IMEM page tags on
    // execution. Upload with secure=true to mark pages as authenticated.
    eprintln!("\n── Step 2: Load fresh firmware (secure tags) ──");
    let gpccs_fw = fecs_boot::GpccsFirmware::load(chip).expect("GPCCS firmware");
    let fecs_fw = fecs_boot::FecsFirmware::load(chip).expect("FECS firmware");
    eprintln!(
        "GPCCS: bl={}B bl_imem_off={:#06x} inst={}B data={}B",
        gpccs_fw.bootloader.len(), gpccs_fw.bl_imem_off, gpccs_fw.inst.len(), gpccs_fw.data.len()
    );
    eprintln!(
        "FECS:  bl={}B bl_imem_off={:#06x} inst={}B data={}B",
        fecs_fw.bootloader.len(), fecs_fw.bl_imem_off, fecs_fw.inst.len(), fecs_fw.data.len()
    );

    // Upload with secure=true (IMEM page tags marked as authenticated)
    fecs_boot::falcon_upload_imem(bar0, gpccs_base, 0, &gpccs_fw.inst, true);
    fecs_boot::falcon_upload_imem(bar0, gpccs_base, gpccs_fw.bl_imem_off, &gpccs_fw.bootloader, true);
    fecs_boot::falcon_upload_dmem(bar0, gpccs_base, 0, &gpccs_fw.data);
    eprintln!("GPCCS firmware uploaded (inst@0x0000 bl@{:#06x}) secure=true", gpccs_fw.bl_imem_off);

    fecs_boot::falcon_upload_imem(bar0, fecs_base, 0, &fecs_fw.inst, true);
    fecs_boot::falcon_upload_imem(bar0, fecs_base, fecs_fw.bl_imem_off, &fecs_fw.bootloader, true);
    fecs_boot::falcon_upload_dmem(bar0, fecs_base, 0, &fecs_fw.data);
    eprintln!("FECS firmware uploaded (inst@0x0000 bl@{:#06x}) secure=true", fecs_fw.bl_imem_off);

    // IMEM verify: read back first 4 words at 0x0000 and bl_imem_off
    eprintln!("\n── Step 2b: IMEM verify ──");
    for (name, base, bv) in [
        ("GPCCS", gpccs_base, gpccs_fw.bl_imem_off),
        ("FECS", fecs_base, fecs_fw.bl_imem_off),
    ] {
        w(base + 0x180, 0x0200_0000); // IMEMC read @ 0
        let w0 = r(base + 0x184);
        let w1 = r(base + 0x184);
        w(base + 0x180, 0x0200_0000 | bv); // IMEMC read @ bl_imem_off
        let b0 = r(base + 0x184);
        let b1 = r(base + 0x184);
        eprintln!("{name}: IMEM[0x0000]={w0:#010x},{w1:#010x} IMEM[{bv:#06x}]={b0:#010x},{b1:#010x}");
    }

    // Step 3: Configure falcon environment
    eprintln!("\n── Step 3: Configure falcon environment ──");
    // GR engine pre-init (nouveau gf100_gr_init_fecs_exceptions + SCC)
    w(fecs_base + 0xC24, 0x000e_0002); // FECS EXCEPTION_REG
    w(fecs_base + 0x802C, 0x0000_0001); // GR_CLASS_CFG
    eprintln!("GR pre-init: EXCEPTION_REG=0x000e0002 GR_CLASS_CFG=0x00000001");

    for (name, base, bv) in [
        ("GPCCS", gpccs_base, gpccs_fw.bl_imem_off),
        ("FECS", fecs_base, fecs_fw.bl_imem_off),
    ] {
        w(base + 0x00C, 0xfc24);       // IRQMODE
        w(base + 0x048, 0x04);          // ITFEN
        w(base + 0x10C, 0);            // DMACTL = 0 (clear DMA state)
        w(base + 0x034, 0x7fff_ffff);   // WATCHDOG
        w(base + 0x040, 0);             // MAILBOX0 = 0
        w(base + 0x044, 0);             // MAILBOX1 = 0
        w(base + 0x104, bv);            // BOOTVEC = bl_imem_off
        w(base + 0x100, 0x01);          // CPUCTL_IINVAL
        std::thread::sleep(std::time::Duration::from_millis(1));
        let exci_pre = r(base + 0x148);
        let bootvec_rb = r(base + 0x104);
        eprintln!("{name}: BOOTVEC={bootvec_rb:#06x} IRQMODE=0xfc24 ITFEN=0x04 pre-start-exci={exci_pre:#010x}");
    }

    w(0x409800, 0); // FECS_CTXSW_MAILBOX
    w(fecs_base + 0x800, 0); // FECS MTHD_STATUS
    w(gpccs_base + 0x10C, 0); // GPCCS DMACTL

    // Step 4: Start GPCCS first, then FECS
    eprintln!("\n── Step 4: STARTCPU ──");
    w(gpccs_base + 0x100, 0x02); // STARTCPU
    std::thread::sleep(std::time::Duration::from_millis(20));
    let gpccs_cpu_post = r(gpccs_base + 0x100);
    let gpccs_exci_post = r(gpccs_base + 0x148);
    let gpccs_pc_post = r(gpccs_base + 0x030);
    eprintln!("GPCCS: cpuctl={gpccs_cpu_post:#010x} exci={gpccs_exci_post:#010x} pc={gpccs_pc_post:#06x}");

    // Sample GPCCS PC over 100ms to check if it's executing
    let mut gpccs_pcs = Vec::new();
    for _ in 0..10 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        gpccs_pcs.push(r(gpccs_base + 0x030));
    }
    eprintln!("GPCCS PC trace (100ms): {gpccs_pcs:02x?}");

    w(fecs_base + 0x100, 0x02); // STARTCPU
    std::thread::sleep(std::time::Duration::from_millis(20));
    let fecs_cpu_post = r(fecs_base + 0x100);
    let fecs_exci_post = r(fecs_base + 0x148);
    let fecs_pc_post = r(fecs_base + 0x030);
    eprintln!("FECS:  cpuctl={fecs_cpu_post:#010x} exci={fecs_exci_post:#010x} pc={fecs_pc_post:#06x}");

    // Step 5: Poll for FECS ready (CTXSW_MAILBOX bit 0)
    eprintln!("\n── Step 5: Wait for FECS ready ──");
    let start = std::time::Instant::now();
    let mut ready = false;
    let mut last_pc = 0u32;
    let mut last_log = std::time::Instant::now();

    while start.elapsed() < std::time::Duration::from_secs(30) {
        std::thread::sleep(std::time::Duration::from_millis(100));

        let mbox = r(0x409800);
        let fecs_cpu = r(fecs_base + 0x100);
        let fecs_pc = r(fecs_base + 0x030);
        let gpccs_cpu = r(gpccs_base + 0x100);
        let gpccs_exci = r(gpccs_base + 0x148);

        if mbox & 1 != 0 {
            eprintln!(
                "*** FECS READY! mbox={mbox:#010x} fecs_cpu={fecs_cpu:#010x} ({}ms) ***",
                start.elapsed().as_millis()
            );
            ready = true;
            break;
        }

        // Log every 2 seconds or on PC change
        if fecs_pc != last_pc || last_log.elapsed() > std::time::Duration::from_secs(2) {
            eprintln!(
                "[{:>5}ms] FECS: cpu={fecs_cpu:#010x} pc={fecs_pc:#06x} | GPCCS: cpu={gpccs_cpu:#010x} exci={gpccs_exci:#010x} | mbox={mbox:#010x}",
                start.elapsed().as_millis()
            );
            last_pc = fecs_pc;
            last_log = std::time::Instant::now();
        }

        // Abort if FECS halted unexpectedly
        if fecs_cpu & 0x30 != 0 {
            eprintln!("FECS halted/reset: cpuctl={fecs_cpu:#010x} ({}ms)", start.elapsed().as_millis());
            break;
        }
    }

    if !ready {
        eprintln!("FECS did not signal ready within 30s");
    }

    // Step 6: Try FECS method probe if ready
    if ready {
        eprintln!("\n── Step 6: FECS method probe ──");
        use coral_driver::nv::vfio_compute::acr_boot::fecs_method;
        fecs_method::fecs_init_exceptions(bar0);
        let mprobe = fecs_method::fecs_probe_methods(bar0);
        eprintln!("{mprobe}");

        if mprobe.ctx_size.is_ok() {
            eprintln!("\n****************************************************");
            eprintln!("*  SOVEREIGN GR BOOT SUCCEEDED!                    *");
            eprintln!("*  FECS responding to methods — Layer 11 unblocked *");
            eprintln!("****************************************************");
        }
    }

    // Final state dump
    eprintln!("\n── Final state ──");
    let probe = dev.falcon_probe();
    eprintln!("{probe}");

    eprintln!("\n=== End Exp 092 ===");
}

/// Exp 095: Sovereign ACR boot with VRAM recovery via nouveau DEVINIT cycle.
///
/// GV100 DEVINIT (clock + HBM2 + VRAM init) requires signed PMU firmware —
/// only nouveau/nvidia drivers can execute it. If VRAM is dead (from a prior
/// SBR), we cycle the GPU through nouveau via GlowPlug to run DEVINIT, then
/// re-acquire the VFIO device with live VRAM.
///
/// Phase 0: Quick VRAM probe — if alive, skip to Phase 2.
/// Phase 1: Nouveau DEVINIT cycle via GlowPlug device.swap.
/// Phase 2: SEC2 soft reset (ENGCTL/PMC, preserves VRAM).
/// Phase 3: VRAM ACR boot → FECS/GPCCS bootstrap.
#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn vfio_clean_vram_acr_boot() {
    use coral_driver::vfio::memory::{MemoryRegion, PraminRegion};

    const SEC2_BASE: usize = 0x087000;
    const FECS_BASE: usize = 0x409000;
    const GPCCS_BASE: usize = 0x41a000;

    init_tracing();

    eprintln!("\n=== Exp 095: Sovereign ACR Boot ===\n");

    let bdf = vfio_bdf();

    // ── Phase 0: Quick VRAM probe ──
    eprintln!("── Phase 0: VRAM probe ──");
    let mut dev = open_vfio();
    let vram_alive = {
        let bar0 = dev.bar0_ref();
        let win = bar0.read_u32(0x1700).unwrap_or(0xDEAD);
        eprintln!("BAR0_WINDOW = {win:#010x}");

        match PraminRegion::new(bar0, 0x2_6000, 8) {
            Ok(mut region) => {
                let sentinel = 0xCAFE_DEAD_u32;
                let _ = region.write_u32(0, sentinel);
                let rb = region.read_u32(0).unwrap_or(0);
                let ok = rb == sentinel;
                eprintln!("PRAMIN sentinel: wrote={sentinel:#010x} read={rb:#010x} ok={ok}");
                ok
            }
            Err(e) => {
                eprintln!("PRAMIN failed: {e}");
                false
            }
        }
    };

    // ── Phase 1: Always cycle through nouveau for fresh DEVINIT + signed firmware ──
    // Nouveau's ACR boot loads properly signed firmware into SEC2's IMEM.
    // After swap back to vfio-pci, SEC2 goes to HRESET but IMEM is preserved.
    // We can then restart SEC2 with its existing authenticated code.
    {
        eprintln!("\n── Phase 1: Nouveau cycle (signed firmware load) ──");
        drop(dev);
        eprintln!("Dropped VFIO device handle");

        let mut gp = crate::glowplug_client::GlowPlugClient::connect()
            .expect("GlowPlug connection");

        eprintln!("Swapping to nouveau for DEVINIT + ACR boot...");
        match gp.swap(&bdf, "nouveau") {
            Ok(r) => eprintln!("swap→nouveau: {r}"),
            Err(e) => {
                eprintln!("swap→nouveau FAILED: {e}");
                eprintln!("=== End Exp 095 (no GlowPlug) ===");
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(3));

        eprintln!("Swapping back to vfio-pci...");
        match gp.swap(&bdf, "vfio-pci") {
            Ok(r) => eprintln!("swap→vfio-pci: {r}"),
            Err(e) => eprintln!("swap→vfio-pci FAILED: {e}"),
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    dev = open_vfio();
    let bar0 = dev.bar0_ref();
    let base = SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD_DEAD);

    // VRAM sanity check after nouveau cycle
    let vram_ok = match PraminRegion::new(bar0, 0x2_6000, 8) {
        Ok(mut region) => {
            let sentinel = 0xCAFE_DEAD_u32;
            let _ = region.write_u32(0, sentinel);
            let rb = region.read_u32(0).unwrap_or(0);
            let ok = rb == sentinel;
            eprintln!("VRAM after nouveau cycle: {ok}");
            ok
        }
        Err(_) => false,
    };
    if !vram_ok {
        eprintln!("VRAM dead after nouveau cycle — aborting");
        eprintln!("\n=== End Exp 095 ===");
        return;
    }

    // ── Phase 2: Probe SEC2/FECS/GPCCS + hardware WPR boundaries ──
    eprintln!("\n── Phase 2: Post-nouveau state ──");
    let cpuctl = r(0x100);
    let sctl = r(0x240);
    let pc = r(0x030);
    let tidx = r(0x148); // TRACE INDEX, NOT exci (see gm200_flcn_tracepc)
    let hwcfg = r(0x108);
    eprintln!("SEC2: cpuctl={cpuctl:#010x} sctl={sctl:#010x} pc={pc:#06x} tidx={tidx:#010x} hwcfg={hwcfg:#010x}");

    let fecs_cpu = bar0.read_u32(FECS_BASE + 0x100).unwrap_or(0xDEAD);
    let gpccs_cpu = bar0.read_u32(GPCCS_BASE + 0x100).unwrap_or(0xDEAD);
    eprintln!("FECS: cpuctl={fecs_cpu:#010x} | GPCCS: cpuctl={gpccs_cpu:#010x}");

    // PMU state — ACR may depend on PMU for power/clock management
    let pmu_cpu2 = bar0.read_u32(0x10A100).unwrap_or(0xDEAD);
    let pmu_sctl2 = bar0.read_u32(0x10A240).unwrap_or(0xDEAD);
    let pmu_pc = bar0.read_u32(0x10A030).unwrap_or(0xDEAD);
    eprintln!("PMU: cpuctl={pmu_cpu2:#010x} sctl={pmu_sctl2:#010x} pc={pmu_pc:#06x}");

    // IMEM probe: check if nouveau's firmware survived the driver swap
    // On falcon, IMEMC bit25=read mode, bits[15:2]=word address
    let read_imem = |base: usize, word_off: u32| -> u32 {
        let _ = bar0.write_u32(base + 0x180, (1u32 << 25) | (word_off & 0xFFFC));
        bar0.read_u32(base + 0x184).unwrap_or(0xDEAD)
    };
    let sec2_i0 = read_imem(SEC2_BASE, 0);
    let sec2_i4 = read_imem(SEC2_BASE, 4);
    let sec2_i8 = read_imem(SEC2_BASE, 8);
    let fecs_i0 = read_imem(FECS_BASE, 0);
    let fecs_i4 = read_imem(FECS_BASE, 4);
    let gpccs_i0 = read_imem(GPCCS_BASE, 0);
    let gpccs_i4 = read_imem(GPCCS_BASE, 4);
    let sec2_populated = sec2_i0 != 0 || sec2_i4 != 0 || sec2_i8 != 0;
    let fecs_populated = fecs_i0 != 0 || fecs_i4 != 0;
    let gpccs_populated = gpccs_i0 != 0 || gpccs_i4 != 0;
    eprintln!("IMEM after swap: SEC2[0..8]=[{sec2_i0:#010x} {sec2_i4:#010x} {sec2_i8:#010x}] populated={sec2_populated}");
    eprintln!("  FECS[0..4]=[{fecs_i0:#010x} {fecs_i4:#010x}] populated={fecs_populated}");
    eprintln!("  GPCCS[0..4]=[{gpccs_i0:#010x} {gpccs_i4:#010x}] populated={gpccs_populated}");

    // DMA state left by nouveau (preserved across vfio-pci bind?)
    let fbif_post = r(0x624);
    let dmactl_post = r(0x10C);
    let itfen_post = r(0x048);
    let bind_inst_post = bar0.read_u32(SEC2_BASE + 0x054).unwrap_or(0xDEAD);
    eprintln!("DMA state: FBIF={fbif_post:#010x} DMACTL={dmactl_post:#010x} ITFEN={itfen_post:#010x} BIND_INST={bind_inst_post:#010x}");

    // ── Clear priv ring faults (nouveau gk104_privring_intr pattern) ──
    // Nouveau: nvkm_mask(device, 0x12004c, 0x3f, 0x02) to ACK fault at 0x120058 bit 8.
    let pri_status = bar0.read_u32(0x120058).unwrap_or(0);
    if pri_status & 0x100 != 0 {
        let cur = bar0.read_u32(0x12004C).unwrap_or(0);
        let _ = bar0.write_u32(0x12004C, (cur & !0x3F) | 0x02);
        for _ in 0..200 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            let s = bar0.read_u32(0x120058).unwrap_or(0xDEAD);
            if s & 0x100 == 0 { break; }
        }
    }
    let pri_after = bar0.read_u32(0x120058).unwrap_or(0xDEAD);
    eprintln!("Priv ring: status_before={pri_status:#010x} status_after={pri_after:#010x}");

    // PFB WPR registers (direct, from experiment 086)
    // Exp 086 finding: "WPR is NEVER configured as persistent hardware state —
    // WPR must be set up dynamically by SEC2 firmware during ACR execution."
    let wpr1_beg = bar0.read_u32(0x100CE4).unwrap_or(0xDEAD);
    let wpr1_end = bar0.read_u32(0x100CE8).unwrap_or(0xDEAD);
    let wpr2_beg = bar0.read_u32(0x100CEC).unwrap_or(0xDEAD);
    let wpr2_end = bar0.read_u32(0x100CF0).unwrap_or(0xDEAD);
    eprintln!("PFB WPR1: beg={wpr1_beg:#010x} end={wpr1_end:#010x}");
    eprintln!("PFB WPR2: beg={wpr2_beg:#010x} end={wpr2_end:#010x}");

    // GM200 indexed register (0x100cd4) — used by gm200_acr_wpr_check
    let _ = bar0.write_u32(0x100cd4, 2);
    std::thread::sleep(std::time::Duration::from_micros(10));
    let wpr_lo_raw = bar0.read_u32(0x100cd4).unwrap_or(0xDEAD);
    let _ = bar0.write_u32(0x100cd4, 3);
    std::thread::sleep(std::time::Duration::from_micros(10));
    let wpr_hi_raw = bar0.read_u32(0x100cd4).unwrap_or(0xDEAD);
    let gm200_start = ((wpr_lo_raw as u64) & 0xFFFF_FF00) << 8;
    let gm200_end = (((wpr_hi_raw as u64) & 0xFFFF_FF00) << 8) + 0x20000;
    eprintln!("GM200 indexed: lo_raw={wpr_lo_raw:#010x} hi_raw={wpr_hi_raw:#010x} → {gm200_start:#x}..{gm200_end:#x}");

    // FBPA WPR2 (often returns 0xBADF1100 = priv ring dead)
    let fbpa_lo = bar0.read_u32(0x1fa824).unwrap_or(0xBADF_1100);
    let fbpa_hi = bar0.read_u32(0x1fa828).unwrap_or(0xBADF_1100);
    eprintln!("FBPA WPR2: lo={fbpa_lo:#010x} hi={fbpa_hi:#010x}");

    // FBHUB MMU fault buffer state (Exp 076: FBHUB stalls without a valid drain target)
    {
        let fb0_lo = bar0.read_u32(0x100E24).unwrap_or(0xDEAD);
        let fb0_hi = bar0.read_u32(0x100E28).unwrap_or(0xDEAD);
        let fb0_sz = bar0.read_u32(0x100E2C).unwrap_or(0xDEAD);
        let fb0_get = bar0.read_u32(0x100E30).unwrap_or(0xDEAD);
        let fb0_put = bar0.read_u32(0x100E34).unwrap_or(0xDEAD);
        let fb0_addr = ((fb0_hi as u64) << 44) | ((fb0_lo as u64) << 12);
        let fb0_enabled = fb0_put & 0x8000_0000 != 0;
        eprintln!("FAULT_BUF0 (non-replay): addr={fb0_addr:#x} sz={fb0_sz} get={fb0_get:#x} put={fb0_put:#010x} enabled={fb0_enabled}");

        let fb1_lo = bar0.read_u32(0x100E44).unwrap_or(0xDEAD);
        let fb1_hi = bar0.read_u32(0x100E48).unwrap_or(0xDEAD);
        let fb1_sz = bar0.read_u32(0x100E4C).unwrap_or(0xDEAD);
        let fb1_get = bar0.read_u32(0x100E50).unwrap_or(0xDEAD);
        let fb1_put = bar0.read_u32(0x100E54).unwrap_or(0xDEAD);
        let fb1_enabled = fb1_put & 0x8000_0000 != 0;
        eprintln!("FAULT_BUF1 (replay):     addr={:#x} sz={fb1_sz} get={fb1_get:#x} put={fb1_put:#010x} enabled={fb1_enabled}",
            ((fb1_hi as u64) << 44) | ((fb1_lo as u64) << 12));

        // FBHUB status: PRI accessibility check
        let fbhub_c2c = bar0.read_u32(0x100C2C).unwrap_or(0xDEAD);
        let mmu_ctrl = bar0.read_u32(0x100C80).unwrap_or(0xDEAD);
        let mmu_phys = bar0.read_u32(0x100C94).unwrap_or(0xDEAD);
        eprintln!("FBHUB: 0x100C2C={fbhub_c2c:#010x} MMU_CTRL={mmu_ctrl:#010x} MMU_PHYS={mmu_phys:#010x}");
        if fbhub_c2c == 0xBADF_5040 {
            eprintln!("  *** FBHUB PRI ERROR — hub not accessible! ***");
        }
    }

    // ── Probe SEC2 DMEM for nouveau's ACR descriptor (before our reset!) ──
    // The ACR data section in DMEM contains flcn_acr_desc_v1 with WPR addresses.
    // Layout: 0x200=reserved, 0x210=wpr_region_id, 0x260=ucode_blob_base
    let read_dmem_u32 = |off: u32| -> u32 {
        let _ = bar0.write_u32(SEC2_BASE + 0x1C0, (1u32 << 25) | off);
        bar0.read_u32(SEC2_BASE + 0x1C4).unwrap_or(0xDEAD)
    };
    let read_dmem_u64 = |off: u32| -> u64 {
        let lo = read_dmem_u32(off) as u64;
        let hi = read_dmem_u32(off + 4) as u64;
        lo | (hi << 32)
    };
    let dmem_wpr_id = read_dmem_u32(0x210);
    let dmem_wpr_offset = read_dmem_u32(0x214);
    let dmem_mmu_range = read_dmem_u32(0x218);
    let dmem_no_regions = read_dmem_u32(0x21C);
    let dmem_r0_start = read_dmem_u32(0x220);
    let dmem_r0_end = read_dmem_u32(0x224);
    let dmem_r0_shadow = read_dmem_u32(0x238);
    let dmem_blob_size = read_dmem_u32(0x258);
    let dmem_blob_base = read_dmem_u64(0x260);
    eprintln!("Nouveau ACR desc (DMEM):");
    eprintln!("  wpr_id={dmem_wpr_id} wpr_off={dmem_wpr_offset:#x} mmu_range={dmem_mmu_range:#x} regions={dmem_no_regions}");
    eprintln!("  r0: start={dmem_r0_start:#x} end={dmem_r0_end:#x} shadow={dmem_r0_shadow:#x}");
    eprintln!("  blob: base={dmem_blob_base:#x} size={dmem_blob_size:#x}");

    // Nouveau's actual WPR location: blob_base points to WPR in VRAM
    // r0.start/end are addr>>8, so real addresses are <<8
    let nouveau_wpr_start = (dmem_r0_start as u64) << 8;
    let nouveau_wpr_end = (dmem_r0_end as u64) << 8;
    eprintln!("  Nouveau WPR region: {nouveau_wpr_start:#x}..{nouveau_wpr_end:#x} ({} KB)",
        (nouveau_wpr_end.saturating_sub(nouveau_wpr_start)) / 1024);

    // ── Phase 2.5: Probe EMEM + try restarting nouveau's SEC2 ──
    //
    // Before we overwrite SEC2, probe its EMEM for queue descriptors that
    // nouveau's ACR firmware left behind, and attempt to restart the halted
    // firmware to see if it enters its command loop.
    eprintln!("\n── Phase 2.5: EMEM probe + SEC2 restart ──");
    {
        use coral_driver::nv::vfio_compute::acr_boot::sec2_emem_read;
        let emem = sec2_emem_read(bar0, 0, 128);
        let nonzero_count = emem.iter().filter(|&&w| w != 0).count();
        eprintln!("EMEM[0..128]: {nonzero_count} non-zero words");
        if nonzero_count > 0 {
            for (i, chunk) in emem.chunks(8).enumerate() {
                let any_nz = chunk.iter().any(|&w| w != 0);
                if any_nz {
                    let vals: Vec<String> = chunk.iter().map(|w| format!("{w:#010x}")).collect();
                    eprintln!("  EMEM[{:3}..{:3}]: {}", i*8, i*8+8, vals.join(" "));
                }
            }
        }

        // Also read queue registers before any reset
        let qh0_pre = bar0.read_u32(SEC2_BASE + 0xC00).unwrap_or(0xDEAD);
        let qt0_pre = bar0.read_u32(SEC2_BASE + 0xC04).unwrap_or(0xDEAD);
        let qh1_pre = bar0.read_u32(SEC2_BASE + 0xC08).unwrap_or(0xDEAD);
        let qt1_pre = bar0.read_u32(SEC2_BASE + 0xC0C).unwrap_or(0xDEAD);
        eprintln!("Pre-restart queues: Q0 h={qh0_pre:#x} t={qt0_pre:#x} | Q1 h={qh1_pre:#x} t={qt1_pre:#x}");

        // Try restarting from HALT via CPUCTL_ALIAS (accessible in LS mode)
        let cpuctl_pre = r(0x100);
        let pc_pre = bar0.read_u32(SEC2_BASE + 0x104).unwrap_or(0xDEAD);
        eprintln!("SEC2 before restart: cpuctl={cpuctl_pre:#010x} pc={pc_pre:#06x}");

        if cpuctl_pre & 0x10 != 0 {
            // HALTED — try resuming via CPUCTL_ALIAS (0x130)
            eprintln!("  SEC2 HALTED — writing CPUCTL_ALIAS START...");
            let _ = bar0.write_u32(SEC2_BASE + 0x130, 0x02); // START bit

            // Wait up to 500ms for it to do something
            for tick in 0..50 {
                std::thread::sleep(std::time::Duration::from_millis(10));
                let c = r(0x100);
                let p = bar0.read_u32(SEC2_BASE + 0x104).unwrap_or(0xDEAD);
                if tick == 0 || tick == 4 || tick == 49 || c != cpuctl_pre || p != pc_pre {
                    eprintln!("  t={:3}ms cpuctl={c:#010x} pc={p:#06x}", (tick+1)*10);
                }
                if c & 0x10 != 0 && tick > 5 { break; } // re-halted
            }

            let cpuctl_post = r(0x100);
            let pc_post = bar0.read_u32(SEC2_BASE + 0x104).unwrap_or(0xDEAD);
            let mb0_post = bar0.read_u32(SEC2_BASE + 0x040).unwrap_or(0xDEAD);
            let mb1_post = bar0.read_u32(SEC2_BASE + 0x044).unwrap_or(0xDEAD);
            eprintln!("SEC2 after restart: cpuctl={cpuctl_post:#010x} pc={pc_post:#06x} mb0={mb0_post:#010x} mb1={mb1_post:#010x}");

            // Check queues after restart
            let qh0_r = bar0.read_u32(SEC2_BASE + 0xA00).unwrap_or(0xDEAD);
            let qt0_r = bar0.read_u32(SEC2_BASE + 0xA04).unwrap_or(0xDEAD);
            let qh1_r = bar0.read_u32(SEC2_BASE + 0xA30).unwrap_or(0xDEAD);
            let qt1_r = bar0.read_u32(SEC2_BASE + 0xA34).unwrap_or(0xDEAD);
            let q_alive = qh0_r != 0 || qt0_r != 0 || qh1_r != 0 || qt1_r != 0;
            eprintln!("Post-restart queues: Q0 h={qh0_r:#x} t={qt0_r:#x} | Q1 h={qh1_r:#x} t={qt1_r:#x} alive={q_alive}");

            // Read EMEM after restart
            let emem_post = sec2_emem_read(bar0, 0, 32);
            let nz_post = emem_post.iter().filter(|&&w| w != 0).count();
            eprintln!("EMEM after restart: {nz_post}/32 non-zero");

            // Check priv ring for new faults
            let pri_post = bar0.read_u32(0x120058).unwrap_or(0xDEAD);
            eprintln!("Priv ring after restart: {pri_post:#010x}");
        } else {
            eprintln!("  SEC2 not halted (maybe running?) — skipping restart");
        }
    }

    // ── Phase 2.75: DEFERRED — runs after Phase 3 to avoid HS mode corruption ──
    // HS mode (SCTL bit 1) entered by sysmem BL authentication is NOT clearable
    // via PMC reset, corrupting all subsequent IMEM/DMEM operations.
    // Phase 3 needs fresh SEC2 (SCTL=0x3000) for VRAM DMA diagnostics.

    // ── Phase 3 first: ACR boot (VRAM DMA, v2 desc) — see below ──

    // ── Phase 2.75: Sysmem ACR boot (FBHUB bypass, blob_size=0) ──
    // Re-enabled: sysmem approach enters HS mode (SCTL=0x3002).
    // With blob_size=0 patch in strategy_sysmem.rs, the ACR should skip
    // the blob DMA that previously caused a trap.
    {
    eprintln!("\n── Phase 2.75: Sysmem ACR boot (FBHUB bypass attempt) ──");
    {
        let dump_sec2_post = |label: &str| {
            let sctl = bar0.read_u32(SEC2_BASE + 0x240).unwrap_or(0xDEAD);
            let cpuctl = bar0.read_u32(SEC2_BASE + 0x100).unwrap_or(0xDEAD);
            let pc = bar0.read_u32(SEC2_BASE + 0x030).unwrap_or(0xDEAD);
            let mb0 = bar0.read_u32(SEC2_BASE + 0x040).unwrap_or(0xDEAD);
            let hs_mode = sctl & 0x02 != 0;
            eprintln!("  {label}: sctl={sctl:#010x} cpuctl={cpuctl:#010x} pc={pc:#06x} mb0={mb0:#010x} HS={hs_mode}");

            // TRACEPC dump (may be readable even in HS mode)
            let tidx = bar0.read_u32(SEC2_BASE + 0x148).unwrap_or(0);
            let nr_traces = ((tidx & 0x00FF_0000) >> 16).min(32);
            if nr_traces > 0 {
                let mut traces = Vec::new();
                for i in 0..nr_traces {
                    let _ = bar0.write_u32(SEC2_BASE + 0x148, i);
                    let tpc = bar0.read_u32(SEC2_BASE + 0x14C).unwrap_or(0xDEAD);
                    traces.push(format!("{tpc:#06x}"));
                }
                eprintln!("  TRACEPC[0..{nr_traces}]: {}", traces.join(" "));
            } else {
                eprintln!("  TRACEPC: {nr_traces} entries (tidx={tidx:#010x})");
            }

            // DMA engine state
            let trfbase = bar0.read_u32(SEC2_BASE + 0x110).unwrap_or(0xDEAD);
            let trfcmd = bar0.read_u32(SEC2_BASE + 0x11C).unwrap_or(0xDEAD);
            let dmactl = bar0.read_u32(SEC2_BASE + 0x10C).unwrap_or(0xDEAD);
            let fbif0 = bar0.read_u32(SEC2_BASE + 0x620).unwrap_or(0xDEAD);
            let fbif1 = bar0.read_u32(SEC2_BASE + 0x624).unwrap_or(0xDEAD);
            eprintln!("  DMA: trfbase={trfbase:#010x} trfcmd={trfcmd:#010x} dmactl={dmactl:#010x} fbif[0]={fbif0:#06x} fbif[1]={fbif1:#06x}");

            // Queue state
            let cmdq_h = bar0.read_u32(SEC2_BASE + 0xA00).unwrap_or(0);
            let cmdq_t = bar0.read_u32(SEC2_BASE + 0xA04).unwrap_or(0);
            let msgq_h = bar0.read_u32(SEC2_BASE + 0xA30).unwrap_or(0);
            let msgq_t = bar0.read_u32(SEC2_BASE + 0xA34).unwrap_or(0);
            if cmdq_h != 0 || cmdq_t != 0 || msgq_h != 0 || msgq_t != 0 {
                eprintln!("  Queues ALIVE: CMDQ h={cmdq_h:#x} t={cmdq_t:#x} MSGQ h={msgq_h:#x} t={msgq_t:#x}");
            }

            // FECS/GPCCS state
            let fecs = bar0.read_u32(FECS_BASE + 0x100).unwrap_or(0xDEAD);
            let gpccs = bar0.read_u32(GPCCS_BASE + 0x100).unwrap_or(0xDEAD);
            eprintln!("  FECS={fecs:#010x} GPCCS={gpccs:#010x}");
            if fecs & 0x10 == 0 && fecs & 0x20 == 0 {
                eprintln!("  *** FECS LEFT HRESET! ***");
            }
            if gpccs & 0x10 == 0 && gpccs & 0x20 == 0 {
                eprintln!("  *** GPCCS LEFT HRESET! ***");
            }
        };

        eprintln!("Strategy 1: sysmem_acr_boot (instance block + page tables in system memory)");
        let sysmem_result = dev.sysmem_acr_boot();
        eprintln!("  result: success={} strategy={}", sysmem_result.success, sysmem_result.strategy);
        for note in &sysmem_result.notes {
            eprintln!("  | {note}");
        }
        dump_sec2_post("post-sysmem");

        // PMC reset SEC2 to clear HS mode before next attempt
        {
            let pmc = bar0.read_u32(0x200).unwrap_or(0);
            let sec2_bit = 1u32 << 22;
            let _ = bar0.write_u32(0x200, pmc & !sec2_bit);
            std::thread::sleep(std::time::Duration::from_micros(50));
            let _ = bar0.write_u32(0x200, pmc | sec2_bit);
            for _ in 0..1000 {
                let d = bar0.read_u32(SEC2_BASE + 0x10C).unwrap_or(0xFF);
                if d & 0x06 == 0 { break; }
                std::thread::sleep(std::time::Duration::from_micros(100));
            }
            let sctl_post = bar0.read_u32(SEC2_BASE + 0x240).unwrap_or(0xDEAD);
            eprintln!("  PMC reset: sctl={sctl_post:#010x} (HS cleared={})", sctl_post & 0x02 == 0);
        }

        eprintln!("Strategy 2: sysmem_physical_boot (PHYS_SYS, no instance block)");
        let phys_result = dev.sysmem_physical_boot();
        eprintln!("  result: success={} strategy={}", phys_result.success, phys_result.strategy);
        for note in &phys_result.notes {
            eprintln!("  | {note}");
        }
        dump_sec2_post("post-phys");

        // PMC reset SEC2 to clear HS mode before Phase 3
        {
            let pmc = bar0.read_u32(0x200).unwrap_or(0);
            let sec2_bit = 1u32 << 22;
            let _ = bar0.write_u32(0x200, pmc & !sec2_bit);
            std::thread::sleep(std::time::Duration::from_micros(50));
            let _ = bar0.write_u32(0x200, pmc | sec2_bit);
            for _ in 0..1000 {
                let d = bar0.read_u32(SEC2_BASE + 0x10C).unwrap_or(0xFF);
                if d & 0x06 == 0 { break; }
                std::thread::sleep(std::time::Duration::from_micros(100));
            }
            let sctl_final = bar0.read_u32(SEC2_BASE + 0x240).unwrap_or(0xDEAD);
            eprintln!("  PMC reset for Phase 3: sctl={sctl_final:#010x}");
        }
    }
    } // end Phase 2.75

    // ── Phase 3: ACR boot (VRAM DMA, v2 desc) — DEFERRED ──
    // Phase 2.75 leaves SEC2 in HS mode, which PMC reset cannot clear.
    // Running Phase 3 on a post-HS SEC2 gives misleading results.
    #[allow(unreachable_code)]
    if false {
    eprintln!("\n── Phase 3: ACR boot (VRAM DMA, v2 desc) ──");

    let chip = "gv100";
    let fw = coral_driver::nv::vfio_compute::acr_boot::AcrFirmwareSet::load(chip)
        .expect("firmware load");
    let parsed = coral_driver::nv::vfio_compute::acr_boot::ParsedAcrFirmware::parse(&fw)
        .expect("firmware parse");
    eprintln!("FW: bl={}B acr={}B data_off={:#x} data_size={:#x} non_sec=[{:#x}+{:#x}] apps={:?}",
        parsed.bl_code.len(), parsed.acr_payload.len(),
        parsed.load_header.data_dma_base, parsed.load_header.data_size,
        parsed.load_header.non_sec_code_off, parsed.load_header.non_sec_code_size,
        parsed.load_header.apps);
    eprintln!("Sig: prod_off={:#x} prod_size={:#x} patch_loc={:#x} patch_sig={:#x}",
        parsed.hs_header.sig_prod_offset, parsed.hs_header.sig_prod_size,
        parsed.hs_header.patch_loc, parsed.hs_header.patch_sig);

    // WPR + ACR always at low VRAM (within 2MB identity-mapped page tables).
    // Nouveau allocates DOUBLE the WPR size: [shadow][WPR]. The ACR reads the
    // shadow for verification, then copies to the WPR. shadow != wpr is required.
    let acr_payload = &parsed.acr_payload;
    let acr_vram_base = 0x50000u64;
    let shadow_vram_base = 0x60000u64;
    let wpr_vram_base = 0x70000u64;

    let wpr_data = coral_driver::nv::vfio_compute::acr_boot::build_wpr(&fw, wpr_vram_base);
    let wpr_vram_end = wpr_vram_base + wpr_data.len() as u64;
    let mut payload_patched = acr_payload.to_vec();
    let data_off = parsed.load_header.data_dma_base as usize;
    coral_driver::nv::vfio_compute::acr_boot::patch_acr_desc(
        &mut payload_patched, data_off, wpr_vram_base, wpr_vram_end, shadow_vram_base,
    );
    // Skip WPR blob DMA: ACR won't DMA the blob from VRAM (we pre-populated WPR via PRAMIN).
    // blob_size=0 tells ACR the WPR is already in place.
    payload_patched[data_off + 0x258..data_off + 0x25C].copy_from_slice(&0u32.to_le_bytes());
    payload_patched[data_off + 0x260..data_off + 0x268].copy_from_slice(&0u64.to_le_bytes());
    eprintln!("ACR: {acr_vram_base:#x} Shadow: {shadow_vram_base:#x} WPR: {wpr_vram_base:#x}..{wpr_vram_end:#x} blob_size=0(skip DMA)");

    // Write ACR payload + WPR to VRAM via PRAMIN
    let write_to_vram = |vaddr: u64, data: &[u8], label: &str| -> bool {
        let mut off = 0usize;
        while off < data.len() {
            let chunk_vram = (vaddr + off as u64) as u32;
            let chunk_size = (data.len() - off).min(0xC000);
            match PraminRegion::new(bar0, chunk_vram, chunk_size) {
                Ok(mut region) => {
                    for w_off in (0..chunk_size).step_by(4) {
                        let src = off + w_off;
                        if src >= data.len() { break; }
                        let end = (src + 4).min(data.len());
                        let mut bytes = [0u8; 4];
                        bytes[..end - src].copy_from_slice(&data[src..end]);
                        if region.write_u32(w_off, u32::from_le_bytes(bytes)).is_err() {
                            eprintln!("  VRAM write failed: {label}@{chunk_vram:#x}+{w_off:#x}");
                            return false;
                        }
                    }
                    off += chunk_size;
                }
                Err(e) => {
                    eprintln!("  PRAMIN failed for {label}@{chunk_vram:#x}: {e}");
                    return false;
                }
            }
        }
        true
    };

    if !write_to_vram(acr_vram_base, &payload_patched, "ACR payload") {
        eprintln!("ACR payload write failed — aborting");
        eprintln!("\n=== End Exp 095 ===");
        return;
    }
    eprintln!("ACR payload: {}B → VRAM@{acr_vram_base:#x}", payload_patched.len());

    // Shadow WPR: identical copy at separate address (nouveau: first half of double allocation)
    if !write_to_vram(shadow_vram_base, &wpr_data, "Shadow WPR") {
        eprintln!("Shadow WPR write failed — aborting");
        eprintln!("\n=== End Exp 095 ===");
        return;
    }
    eprintln!("Shadow WPR: {}B → VRAM@{shadow_vram_base:#x}", wpr_data.len());

    if !write_to_vram(wpr_vram_base, &wpr_data, "WPR") {
        eprintln!("WPR write failed — aborting");
        eprintln!("\n=== End Exp 095 ===");
        return;
    }
    eprintln!("WPR: {}B → VRAM@{wpr_vram_base:#x}", wpr_data.len());

    use coral_driver::nv::vfio_compute::acr_boot::{
        falcon_dmem_upload, falcon_imem_upload_nouveau, falcon_start_cpu,
    };

    // ── Pre-configure WPR hardware registers ──
    // The ACR firmware may poll WPR registers to verify its WPR setup took effect.
    // With WPR disabled (all zeros after nouveau unbind), the ACR's poll would fail.
    // Try writing our WPR range to PFB WPR2 registers (0x100CEC/0x100CF0).
    //
    // Format: address >> 8 for GM200 indexed (0x100CD4), raw address for PFB direct.
    // We try BOTH PFB direct and GM200 indexed approaches.
    {
        let wpr_beg_val = wpr_vram_base as u32;         // 0x70000
        let wpr_end_val = (wpr_vram_base + wpr_data.len() as u64) as u32; // 0x7CD00

        // PFB direct: try raw address, address>>8, address>>12 with enable bits
        let formats: &[(&str, u32, u32)] = &[
            ("raw", wpr_beg_val, wpr_end_val),
            (">>8|1", (wpr_beg_val >> 8) | 1, wpr_end_val >> 8),
            (">>12|1", (wpr_beg_val >> 12) | 1, wpr_end_val >> 12),
        ];

        for (name, beg, end) in formats {
            let _ = bar0.write_u32(0x100CEC, *beg); // WPR2_BEG
            let _ = bar0.write_u32(0x100CF0, *end); // WPR2_END
            let rb_beg = bar0.read_u32(0x100CEC).unwrap_or(0xDEAD);
            let rb_end = bar0.read_u32(0x100CF0).unwrap_or(0xDEAD);
            let stuck = rb_beg == *beg && rb_end == *end;
            eprintln!("WPR2 write ({name}): beg={beg:#010x}→{rb_beg:#010x} end={end:#010x}→{rb_end:#010x} wrote={stuck}");
            if stuck { break; }
        }

        // Also try GM200 indexed write approach
        let gm200_lo = (wpr_beg_val >> 8) | 0x01; // enable bit
        let gm200_hi = wpr_end_val >> 8;
        let _ = bar0.write_u32(0x100CD4, gm200_lo);
        std::thread::sleep(std::time::Duration::from_micros(10));
        let rb_lo = bar0.read_u32(0x100CD4).unwrap_or(0xDEAD);
        eprintln!("GM200 indexed write: lo={gm200_lo:#010x}→{rb_lo:#010x}");

        // Final read of all WPR-related registers
        let wpr2_beg = bar0.read_u32(0x100CEC).unwrap_or(0xDEAD);
        let wpr2_end = bar0.read_u32(0x100CF0).unwrap_or(0xDEAD);
        eprintln!("WPR2 final: beg={wpr2_beg:#010x} end={wpr2_end:#010x}");
    }

    // ── Reconfigure FBHUB MMU fault buffers ──
    // Exp 076: FBHUB stalls ALL DMA (including SEC2 FBIF) without a valid fault
    // buffer drain target. After nouveau unbind, the old fault buffer DMA mapping
    // is invalid. VfioChannel::create configures FAULT_BUF0/1 at IOVA 0xA000,
    // but the write may not stick if PFB is in a degraded state. Force it here.
    {
        let fb_iova: u64 = 0xA000; // Same IOVA as VfioChannel's fault buffer
        let fb_lo = (fb_iova >> 12) as u32;
        let fb_entries: u32 = 64;

        // Reset GET pointers and disable before reconfiguring
        let _ = bar0.write_u32(0x100E34, 0); // FAULT_BUF0_PUT: disable
        let _ = bar0.write_u32(0x100E54, 0); // FAULT_BUF1_PUT: disable
        std::thread::sleep(std::time::Duration::from_micros(100));

        // Non-replayable fault buffer (BUF0)
        let _ = bar0.write_u32(0x100E24, fb_lo);     // LO
        let _ = bar0.write_u32(0x100E28, 0);          // HI
        let _ = bar0.write_u32(0x100E2C, fb_entries);  // SIZE
        let _ = bar0.write_u32(0x100E30, 0);           // GET
        let _ = bar0.write_u32(0x100E34, 0x8000_0000); // PUT: enable

        // Replayable fault buffer (BUF1) — same backing buffer
        let _ = bar0.write_u32(0x100E44, fb_lo);
        let _ = bar0.write_u32(0x100E48, 0);
        let _ = bar0.write_u32(0x100E4C, fb_entries);
        let _ = bar0.write_u32(0x100E50, 0);
        let _ = bar0.write_u32(0x100E54, 0x8000_0000);

        // Verify readback
        let rb_lo = bar0.read_u32(0x100E24).unwrap_or(0xDEAD);
        let rb_put = bar0.read_u32(0x100E34).unwrap_or(0xDEAD);
        let rb_enabled = rb_put & 0x8000_0000 != 0;
        eprintln!("Fault buffer reconfig: lo={rb_lo:#x} (expect {fb_lo:#x}) put={rb_put:#010x} enabled={rb_enabled}");
        if rb_lo != fb_lo {
            eprintln!("  *** FAULT_BUF0_LO write FAILED — FBHUB may be PRI-dead ***");
        }

        // Flush GPU MMU TLB (Exp 060: PAGE_ALL + HUB_ONLY via 0x100CBC)
        let _ = bar0.write_u32(0x100CBC, 0x0000_0001); // PRI_PFB_PRI_MMU_CTRL
        std::thread::sleep(std::time::Duration::from_micros(100));
        let mmu_ctrl_post = bar0.read_u32(0x100CBC).unwrap_or(0xDEAD);
        eprintln!("MMU TLB flush: ctrl={mmu_ctrl_post:#010x}");
    }

    // ── SEC2 engine reset (matching nouveau gm200_flcn_disable + gm200_flcn_enable) ──
    {
        // DISABLE: clear ITFEN, clear interrupts, PMC disable
        let _ = bar0.write_u32(SEC2_BASE + 0x048, r(0x048) & !0x03); // ITFEN clear bits 0:1
        let _ = bar0.write_u32(SEC2_BASE + 0x014, 0xFFFF_FFFF); // clear all interrupts

        let pmc = bar0.read_u32(0x200).unwrap_or(0);
        let sec2_bit = 1u32 << 22;
        let _ = bar0.write_u32(0x200, pmc & !sec2_bit); // PMC disable
        std::thread::sleep(std::time::Duration::from_micros(50));

        // ENABLE: PMC enable, wait scrub, write BOOT0
        let _ = bar0.write_u32(0x200, pmc | sec2_bit); // PMC enable
        for _ in 0..5000 {
            if r(0x10C) & 0x06 == 0 { break; }
            std::thread::sleep(std::time::Duration::from_micros(100));
        }
        let boot0 = bar0.read_u32(0x000).unwrap_or(0);
        let _ = bar0.write_u32(SEC2_BASE + 0x084, boot0);
        for _ in 0..5000 {
            if r(0x100) & 0x10 != 0 { break; }
            std::thread::sleep(std::time::Duration::from_micros(100));
        }
        eprintln!("Post-reset: cpuctl={:#010x} sctl={:#010x} FBIF={:#010x} DMACTL={:#010x}",
            r(0x100), r(0x240), r(0x624), r(0x10C));
    }

    // ── DMA config: HYBRID page tables (sysmem PTEs for ACR code, VRAM for WPR) ──
    // FBHUB is degraded after VFIO takeover: VRAM DMA reads corrupt data, breaking
    // the BL's signature verification and preventing HS mode entry. System memory
    // DMA bypasses FBHUB entirely. Allocate a sysmem DMA buffer for the ACR payload
    // and patch PT0 entries for those pages to SYS_MEM_COH aperture. WPR/shadow
    // pages stay as VRAM PTEs (WPR hardware only protects VRAM).
    let _acr_dma_guard; // DMA buffer must outlive the boot
    {
        use coral_driver::nv::vfio_compute::acr_boot::{
            build_vram_falcon_inst_block, encode_bind_inst, encode_sysmem_pte,
            falcon_bind_context, FALCON_INST_VRAM, FALCON_PT0_VRAM,
        };
        use coral_driver::vfio::dma::DmaBuffer;
        use coral_driver::vfio::memory::PraminRegion as PR2;

        // Build VRAM page tables (identity-map first 2MB — all VRAM aperture)
        let pt_ok = build_vram_falcon_inst_block(bar0);
        eprintln!("Instance block built: {pt_ok}");

        // Allocate sysmem DMA buffer for ACR payload at IOVA matching acr_vram_base
        let acr_buf_size = (payload_patched.len().div_ceil(4096)) * 4096;
        let container = dev.dma_backend();
        let mut acr_dma = DmaBuffer::new(container, acr_buf_size.max(4096), acr_vram_base)
            .expect("DMA alloc for ACR sysmem");
        acr_dma.as_mut_slice()[..payload_patched.len()].copy_from_slice(&payload_patched);
        eprintln!("ACR sysmem DMA: {}B at IOVA {acr_vram_base:#x}", payload_patched.len());

        // Overwrite PT0 entries for ACR payload pages: VRAM → SYS_MEM_COH
        let acr_page_start = (acr_vram_base / 4096) as usize;
        let acr_page_end = ((acr_vram_base + acr_buf_size as u64).div_ceil(4096)) as usize;
        let mut pt_patched = 0usize;
        for page in acr_page_start..acr_page_end {
            let iova = (page as u64) * 4096;
            let pte = encode_sysmem_pte(iova);
            let pte_lo = (pte & 0xFFFF_FFFF) as u32;
            let pte_hi = (pte >> 32) as u32;
            let off = page * 8;
            if let Ok(mut r) = PR2::new(bar0, FALCON_PT0_VRAM, off + 8) {
                let _ = r.write_u32(off, pte_lo);
                let _ = r.write_u32(off + 4, pte_hi);
                pt_patched += 1;
            }
        }
        eprintln!("PT0 hybrid: pages {acr_page_start}..{acr_page_end} → SYS_MEM_COH ({pt_patched} patched)");
        _acr_dma_guard = acr_dma;

        // Enable ITFEN for FBIF + ENGINE interfaces
        let _ = bar0.write_u32(SEC2_BASE + 0x048, r(0x048) | 0x03);

        // Bind instance block to SEC2 (full nouveau sequence)
        let bind_val = encode_bind_inst(FALCON_INST_VRAM as u64, 0); // target=0=VRAM
        let (bind_ok, bind_notes) = falcon_bind_context(
            &|off| bar0.read_u32(SEC2_BASE + off).unwrap_or(0xDEAD),
            &|off, val| { let _ = bar0.write_u32(SEC2_BASE + off, val); },
            bind_val,
        );
        for note in &bind_notes {
            eprintln!("  bind: {note}");
        }
        eprintln!("Instance block bind: ok={bind_ok}");

        // Set DMACTL for virtual DMA context (matching nouveau)
        let _ = bar0.write_u32(SEC2_BASE + 0x10C, 0x02); // DMACTL=2 (use bound ctx)

        eprintln!("TRANSCFG (no physical override):");
        for port in 0..8usize {
            let reg = 0x620 + port * 4;
            let val = r(reg);
            eprint!("  [{port}]={val:#06x}");
        }
        eprintln!();
        eprintln!("Virtual DMA: ITFEN={:#010x} DMACTL={:#010x} BIND={:#010x}",
            r(0x048), r(0x10C), r(0x054));
    }

    // ── Upload BL to IMEM ──
    let hwcfg = r(0x108);
    let code_limit = (hwcfg & 0x1FF) * 256;
    let boot_size = ((parsed.bl_desc.bl_code_off + parsed.bl_desc.bl_code_size + 0xFF) & !0xFF) as u32;
    let imem_addr = code_limit.saturating_sub(boot_size);
    let start_tag = parsed.bl_desc.bl_start_tag;
    let boot_addr = start_tag << 8;
    falcon_imem_upload_nouveau(bar0, SEC2_BASE, imem_addr, &parsed.bl_code, start_tag);
    eprintln!("BL: {}B → IMEM@{imem_addr:#x} tag={start_tag:#x} boot_addr={boot_addr:#x}",
        parsed.bl_code.len());

    // ── Pre-load data section → DMEM, then BL descriptor on top ──
    // The BL only loads code to IMEM; neither BL nor ACR successfully DMA-loads the
    // data section to DMEM (DMA transfer engine fails with our physical config).
    // Solution: pre-load the data section to DMEM[0..data_size] ourselves, then
    // write the BLD on top at DMEM[0..84]. The data section's first 512 bytes are
    // reserved_dmem (zeros), so the BLD overlaps only with don't-care bytes.
    // The ACR descriptor starts at DMEM[0x200], safely beyond the 84-byte BLD.
    let code_dma_base = acr_vram_base;
    let data_dma_base = acr_vram_base + parsed.load_header.data_dma_base as u64;
    let data_off = parsed.load_header.data_dma_base as usize;
    let data_size = parsed.load_header.data_size as usize;
    let data_section = &payload_patched[data_off..data_off + data_size];
    eprintln!("Pre-loading data section: {}B → DMEM[0..{data_size:#x}]", data_section.len());
    falcon_dmem_upload(bar0, SEC2_BASE, 0, data_section);

    let mut bl_desc = coral_driver::nv::vfio_compute::acr_boot::build_bl_dmem_desc(
        code_dma_base, data_dma_base, &parsed,
    );
    // ctx_dma=1 (FALCON_DMAIDX_VIRT): DMA through the bound instance block's page tables.
    bl_desc[32..36].copy_from_slice(&1u32.to_le_bytes());
    let ctx_dma_val = u32::from_le_bytes(bl_desc[32..36].try_into().unwrap());
    eprintln!("BL desc: {}B ctx_dma={ctx_dma_val} code={code_dma_base:#x} data={data_dma_base:#x}",
        bl_desc.len());
    let dmem_off = parsed.bl_desc.bl_desc_dmem_load_off;
    eprintln!("BL expects desc at DMEM offset {dmem_off:#x} (we write at 0x0)");
    falcon_dmem_upload(bar0, SEC2_BASE, 0, &bl_desc);

    // Verify IMEM upload: read back first 4 words of BL at imem_addr
    {
        let _ = bar0.write_u32(SEC2_BASE + 0x180, imem_addr | (start_tag << 24));
        let imem_w0 = bar0.read_u32(SEC2_BASE + 0x184).unwrap_or(0xDEAD);
        let imem_w1 = bar0.read_u32(SEC2_BASE + 0x184).unwrap_or(0xDEAD);
        let expect_w0 = u32::from_le_bytes(parsed.bl_code[..4].try_into().unwrap());
        let expect_w1 = u32::from_le_bytes(parsed.bl_code[4..8].try_into().unwrap());
        eprintln!("IMEM verify @{imem_addr:#x}: [{imem_w0:#010x} {imem_w1:#010x}] expect=[{expect_w0:#010x} {expect_w1:#010x}] match={}",
            imem_w0 == expect_w0 && imem_w1 == expect_w1);
    }

    // Verify DMEM upload: read back first 4 words of BL desc at offset 0
    {
        use coral_driver::nv::vfio_compute::acr_boot::sec2_emem_read;
        let _ = bar0.write_u32(SEC2_BASE + 0x1C0, 0); // DMEM index port 0
        let dm0 = bar0.read_u32(SEC2_BASE + 0x1C4).unwrap_or(0xDEAD); // word 0 (reserved)
        let _ = bar0.write_u32(SEC2_BASE + 0x1C0, 32); // offset 32 = ctx_dma
        let dm_ctx = bar0.read_u32(SEC2_BASE + 0x1C4).unwrap_or(0xDEAD);
        let _ = bar0.write_u32(SEC2_BASE + 0x1C0, 36); // offset 36 = code_dma_base lo
        let dm_code_lo = bar0.read_u32(SEC2_BASE + 0x1C4).unwrap_or(0xDEAD);
        eprintln!("DMEM verify @0: reserved={dm0:#010x} ctx_dma={dm_ctx:#010x} code_lo={dm_code_lo:#010x}");
    }

    // ── Boot SEC2 ──

    // NVIDIA RM sets TIMPRE before starting the falcon (kflcnableSetup_HAL).
    // TIMPRE = timer prescaler: falcon timer frequency = ref_clock / (TIMPRE + 1).
    // 0xE2 = 226, giving ~6.6μs ticks at 1.5GHz. Without this, firmware timeout
    // logic may behave incorrectly.
    let _ = bar0.write_u32(SEC2_BASE + 0x024, 0x0000_00E2); // TIMPRE
    let timpre_rb = r(0x024);
    eprintln!("TIMPRE={timpre_rb:#010x}");

    // IRQDEST routes each interrupt source to falcon CPU vs HOST.
    // Bit N set → source N goes to falcon CPU. Without this, the firmware
    // can't receive timer or software-generated interrupts.
    // Enable: bit 0 (EXT/GPTMR), bit 1 (WDTMR), bit 6 (SWGEN0), bit 7 (SWGEN1)
    let irqdest = (1u32 << 0) | (1u32 << 1) | (1u32 << 6) | (1u32 << 7);
    let _ = bar0.write_u32(SEC2_BASE + 0x01C, irqdest);
    let irqdest_rb = r(0x01C);
    eprintln!("IRQDEST={irqdest_rb:#010x} (expect {irqdest:#010x})");

    let _ = bar0.write_u32(SEC2_BASE + 0x040, 0xdead_a5a5); // sentinel
    let _ = bar0.write_u32(SEC2_BASE + 0x044, 0);
    let _ = bar0.write_u32(SEC2_BASE + 0x104, boot_addr); // BOOTVEC
    let bv_readback = r(0x104);
    eprintln!("Pre-start: BOOTVEC={bv_readback:#010x} cpuctl={:#010x}", r(0x100));
    eprintln!("STARTCPU: bootvec={boot_addr:#x} mb0=0xdeada5a5");
    // Full SEC2 register diff: snapshot before/after CPU start to see what firmware changed
    let reg_range: Vec<usize> = (0..0xD00usize).step_by(4).collect();
    let mut pre_regs: Vec<(usize, u32)> = Vec::new();
    for &off in &reg_range {
        let v = bar0.read_u32(SEC2_BASE + off).unwrap_or(0xBADF_1100);
        pre_regs.push((off, v));
    }

    falcon_start_cpu(bar0, SEC2_BASE);
    std::thread::sleep(std::time::Duration::from_millis(2));

    let mut post_regs: Vec<(usize, u32)> = Vec::new();
    for &off in &reg_range {
        let v = bar0.read_u32(SEC2_BASE + off).unwrap_or(0xBADF_1100);
        post_regs.push((off, v));
    }

    let mut diffs: Vec<(usize, u32, u32)> = Vec::new();
    for (i, &(off, pre)) in pre_regs.iter().enumerate() {
        let post = post_regs[i].1;
        if pre != post {
            diffs.push((off, pre, post));
        }
    }
    eprintln!("SEC2 register diff (pre vs post-boot, {} changed):", diffs.len());
    for &(off, pre, post) in &diffs {
        eprintln!("  [{off:#05x}] {pre:#010x} → {post:#010x}");
    }

    // Also check: PRIV ring, PFB WPR, PGRAPH engine after firmware ran
    let pri_mid = bar0.read_u32(0x120058).unwrap_or(0xDEAD);
    eprintln!("Priv ring 2ms after boot: {pri_mid:#010x}");

    // Continue with slower polling for remaining timeout
    let poll_start = std::time::Instant::now();
    let mut pc_trace: Vec<u32> = Vec::new();
    let timeout = std::time::Duration::from_secs(5);
    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let c = r(0x100);
        let p = r(0x030);
        let m0 = r(0x040);
        let m1 = r(0x044);
        if pc_trace.last() != Some(&p) { pc_trace.push(p); }

        let halted = c & 0x20 != 0;
        let mb_changed = m0 != 0xdead_a5a5 && m0 != 0;
        let hreset = c & 0x10 != 0;

        if halted || mb_changed || hreset {
            eprintln!("SEC2 event: cpuctl={c:#010x} pc={p:#06x} mb0={m0:#010x} mb1={m1:#010x} ({}ms)",
                poll_start.elapsed().as_millis());
            if mb_changed { eprintln!("  *** BL/ACR responded via MAILBOX! ***"); }
            break;
        }
        if poll_start.elapsed() > timeout {
            eprintln!("SEC2 timeout: cpuctl={c:#010x} pc={p:#06x} mb0={m0:#010x} mb1={m1:#010x}");
            break;
        }
    }
    let pcs: Vec<String> = pc_trace.iter().map(|p| format!("{p:#06x}")).collect();
    eprintln!("PC trace: [{}]", pcs.join(" → "));

    // Post-boot diagnostics
    // Register 0x148 is TRACE INDEX (not EXCI!). Nouveau gm200_flcn_tracepc():
    //   bits[23:16] = number of trace entries. Write index → 0x148, read PC → 0x14C.
    let tidx = r(0x148);
    let nr_traces = ((tidx & 0x00FF_0000) >> 16).min(32);
    let fbif_final = r(0x624);
    let dmactl_final = r(0x10C);
    eprintln!("Post-boot: TIDX={tidx:#010x} ({nr_traces} traces) FBIF={fbif_final:#010x} DMACTL={dmactl_final:#010x}");

    // Dump TRACEPC buffer — shows actual execution history
    if nr_traces > 0 {
        let mut traces = Vec::new();
        for i in 0..nr_traces {
            let _ = bar0.write_u32(SEC2_BASE + 0x148, i);
            let tpc = bar0.read_u32(SEC2_BASE + 0x14C).unwrap_or(0xDEAD);
            traces.push(format!("{tpc:#06x}"));
        }
        eprintln!("TRACEPC[0..{nr_traces}]: {}", traces.join(" "));
    }

    // DMA engine registers: if ACR is stuck polling a DMA completion, this reveals it
    eprintln!("DMA engine: TRFBASE={:#010x} TRFMOFFS={:#010x} TRFFBOFFS={:#010x} TRFCMD={:#010x}",
        r(0x110), r(0x114), r(0x118), r(0x11C));
    // Interrupt state: pending IRQs the ACR firmware might be waiting for
    eprintln!("IRQ: STAT={:#010x} MASK={:#010x} DEST={:#010x} SSET={:#010x} MSET={:#010x}",
        r(0x008), r(0x014), r(0x01C), r(0x010), r(0x018));
    eprintln!("SCTL={:#010x} CPUCTL={:#010x}", r(0x240), r(0x100));
    // Falcon exception/halt info: 0x18C is EXE_CTRL on some revisions
    eprintln!("Falcon debug: 0x18C={:#010x} 0x030(PC)={:#06x} 0x034(SP)={:#010x}",
        r(0x18C), r(0x030), r(0x034));
    // PMU state: ACR may require PMU to be alive for power/clock management
    let pmu_cpuctl = bar0.read_u32(0x10A100).unwrap_or(0xDEAD);
    let pmu_sctl = bar0.read_u32(0x10A240).unwrap_or(0xDEAD);
    eprintln!("PMU: cpuctl={pmu_cpuctl:#010x} sctl={pmu_sctl:#010x}");
    // Check PGRAPH engine state (FECS/GPCCS) — if ACR tried to bootstrap them
    let fecs_mid = bar0.read_u32(FECS_BASE + 0x100).unwrap_or(0xDEAD);
    let gpccs_mid = bar0.read_u32(GPCCS_BASE + 0x100).unwrap_or(0xDEAD);
    eprintln!("Mid-boot: FECS cpuctl={fecs_mid:#010x} GPCCS cpuctl={gpccs_mid:#010x}");

    // DMEM dump — see what the BL loaded and what the ACR wrote
    {
        let read_dmem = |off: u32| -> u32 {
            let _ = bar0.write_u32(SEC2_BASE + 0x1C0, (1u32 << 25) | off);
            bar0.read_u32(SEC2_BASE + 0x1C4).unwrap_or(0xDEAD)
        };

        // Dump ACR descriptor region (data_section[0x200..0x270] = DMEM[0x200..0x270])
        eprintln!("DMEM ACR descriptor after boot:");
        eprintln!("  [0x200] signatures: {:08x} {:08x} {:08x} {:08x}",
            read_dmem(0x200), read_dmem(0x204), read_dmem(0x208), read_dmem(0x20C));
        eprintln!("  [0x210] wpr_region_id={:#x} wpr_off={:#x} mmu_range={:#x} no_regions={:#x}",
            read_dmem(0x210), read_dmem(0x214), read_dmem(0x218), read_dmem(0x21C));
        eprintln!("  [0x220] r0: start={:#x} end={:#x} id={:#x} read={:#x}",
            read_dmem(0x220), read_dmem(0x224), read_dmem(0x228), read_dmem(0x22C));
        eprintln!("  [0x230] r0: write={:#x} client={:#x} shadow={:#x}",
            read_dmem(0x230), read_dmem(0x234), read_dmem(0x238));
        eprintln!("  [0x258] blob_size={:#x} blob_base={:#x}_{:08x}",
            read_dmem(0x258), read_dmem(0x264), read_dmem(0x260));

        // Scan DMEM for non-zero regions outside the descriptor
        // First 32 words (BL descriptor area at offset 0)
        eprintln!("DMEM BL desc area [0x00..0x54]:");
        for off in (0..0x54).step_by(16) {
            let w: Vec<String> = (0..4).map(|i| format!("{:08x}", read_dmem(off + i * 4))).collect();
            eprintln!("  [{off:#05x}] {}", w.join(" "));
        }

        // The BL may load data to DMEM at its ORIGINAL offset (data_dma_base=0x2F00),
        // not at offset 0. Check the ACR descriptor at DMEM[0x2F00 + 0x210] = DMEM[0x3110].
        eprintln!("DMEM ACR descriptor at data_off-relative (DMEM[0x2F00+]):");
        let d2 = 0x2F00u32;
        eprintln!("  [+0x210] wpr_region_id={:#x} wpr_off={:#x} no_regions={:#x}",
            read_dmem(d2 + 0x210), read_dmem(d2 + 0x214), read_dmem(d2 + 0x21C));
        eprintln!("  [+0x220] r0: start={:#x} end={:#x} shadow={:#x}",
            read_dmem(d2 + 0x220), read_dmem(d2 + 0x224), read_dmem(d2 + 0x238));
        eprintln!("  [+0x258] blob_size={:#x} blob_base={:#x}_{:08x}",
            read_dmem(d2 + 0x258), read_dmem(d2 + 0x264), read_dmem(d2 + 0x260));

        // Wide scan: every 256 bytes in 0..0x8000 (32KB of 64KB DMEM)
        let mut nz_regions = Vec::new();
        for off in (0u32..0x8000).step_by(256) {
            let v = read_dmem(off);
            if v != 0 { nz_regions.push((off, v)); }
        }
        eprintln!("DMEM non-zero samples (every 256B in 0..32K): {} hits", nz_regions.len());
        for (off, v) in &nz_regions {
            eprintln!("  [{off:#06x}] = {v:#010x}");
        }

        // Dense scan of data section area (0x2E00..0x4000) every 16B
        let mut nz_data = Vec::new();
        for off in (0x2E00u32..0x4000).step_by(16) {
            let v = read_dmem(off);
            if v != 0 { nz_data.push((off, v)); }
        }
        eprintln!("DMEM data section area (0x2E00..0x4000): {} non-zero", nz_data.len());
        for (off, v) in nz_data.iter().take(16) {
            eprintln!("  [{off:#06x}] = {v:#010x}");
        }
    }

    // EMEM dump after ACR boot — wide scan to find any firmware-written data
    {
        use coral_driver::nv::vfio_compute::acr_boot::sec2_emem_read;
        let emem_post3 = sec2_emem_read(bar0, 0, 256);
        let nz = emem_post3.iter().filter(|&&w| w != 0).count();
        eprintln!("EMEM after ACR boot: {nz}/256 non-zero");
        for (i, chunk) in emem_post3.chunks(8).enumerate() {
            let any_nz = chunk.iter().any(|&w| w != 0);
            if any_nz {
                let vals: Vec<String> = chunk.iter().map(|w| format!("{w:#010x}")).collect();
                eprintln!("  EMEM[{:3}..{:3}]: {}", i*8, i*8+8, vals.join(" "));
            }
        }
        let pri_p3 = bar0.read_u32(0x120058).unwrap_or(0xDEAD);
        eprintln!("Priv ring after ACR boot: {pri_p3:#010x}");
    }

    // Queue registers: CMDQ at 0xA00/0xA04, MSGQ at 0xA30/0xA34 (Exp 089b)
    let cmdq_h = bar0.read_u32(SEC2_BASE + 0xA00).unwrap_or(0xDEAD);
    let cmdq_t = bar0.read_u32(SEC2_BASE + 0xA04).unwrap_or(0xDEAD);
    let msgq_h = bar0.read_u32(SEC2_BASE + 0xA30).unwrap_or(0xDEAD);
    let msgq_t = bar0.read_u32(SEC2_BASE + 0xA34).unwrap_or(0xDEAD);
    let queues_alive = cmdq_h != 0 || cmdq_t != 0 || msgq_h != 0 || msgq_t != 0;
    eprintln!("Queues: CMDQ h={cmdq_h:#x} t={cmdq_t:#x} | MSGQ h={msgq_h:#x} t={msgq_t:#x} alive={queues_alive}");
    // Scan both queue register ranges for non-zero values
    {
        let mut q_nz = Vec::new();
        for off in (0xA00u32..=0xAFF).step_by(4) {
            let v = r(off as usize);
            if v != 0 { q_nz.push((off, v)); }
        }
        for off in (0xC00u32..=0xCFF).step_by(4) {
            let v = r(off as usize);
            if v != 0 { q_nz.push((off, v)); }
        }
        if !q_nz.is_empty() {
            eprintln!("Non-zero queue regs in 0xA00..0xAFF,0xC00..0xCFF: {:?}",
                q_nz.iter().map(|(o,v)| format!("{o:#05x}={v:#x}")).collect::<Vec<_>>());
        }
    }

    // Timer registers: if the firmware's polling loop waits on a timer
    eprintln!("Timers: GPTMR={:#010x} TIMPRE={:#010x} FTIMER={:#010x}",
        r(0x020), r(0x024), r(0x028));

    // FECS/GPCCS state after ACR
    let fecs_post = bar0.read_u32(FECS_BASE + 0x100).unwrap_or(0xDEAD);
    let gpccs_post = bar0.read_u32(GPCCS_BASE + 0x100).unwrap_or(0xDEAD);
    eprintln!("After ACR: FECS cpuctl={fecs_post:#010x} GPCCS cpuctl={gpccs_post:#010x}");

    if fecs_post & 0x10 == 0 && fecs_post & 0x20 == 0 {
        eprintln!("*** FECS LEFT HRESET! ***");
    }
    if gpccs_post & 0x10 == 0 && gpccs_post & 0x20 == 0 {
        eprintln!("*** GPCCS LEFT HRESET! ***");
    }

    } // end if false (Phase 3 deferred)

    // ── Phase 3.5: Firmware interaction experiments ──
    // ACR is running at PC≈0x1934 with ALL interrupts disabled. Try to
    // communicate with it: write mailboxes, enable + trigger interrupts.
    eprintln!("\n── Phase 3.5: Firmware interaction ──");
    {
        let pc_before = r(0x030);
        let cpu_before = r(0x100);
        eprintln!("Before interaction: PC={pc_before:#06x} CPUCTL={cpu_before:#010x}");

        // Experiment 1: Write MB0 = 0x00000001 (host acknowledgment)
        let _ = bar0.write_u32(SEC2_BASE + 0x040, 0x0000_0001);
        std::thread::sleep(std::time::Duration::from_millis(100));
        let pc_a = r(0x030);
        let mb0_a = bar0.read_u32(SEC2_BASE + 0x040).unwrap_or(0xDEAD);
        eprintln!("After MB0=1: PC={pc_a:#06x} MB0={mb0_a:#010x} (PC moved={})", pc_a != pc_before);

        // Experiment 2: Enable SWGEN0 interrupt and trigger it
        // Falcon IRQ layout: bit 6 = SWGEN0, bit 7 = SWGEN1
        let _ = bar0.write_u32(SEC2_BASE + 0x010, 1u32 << 6); // IRQMSET: enable SWGEN0
        let irqmask = r(0x018); // read back IRQMASK
        eprintln!("IRQMASK after IRQMSET(SWGEN0): {irqmask:#010x}");

        let _ = bar0.write_u32(SEC2_BASE + 0x000, 1u32 << 6); // IRQSSET: trigger SWGEN0
        std::thread::sleep(std::time::Duration::from_millis(200));
        let pc_b = r(0x030);
        let irqstat_b = r(0x008);
        let mb0_b = bar0.read_u32(SEC2_BASE + 0x040).unwrap_or(0xDEAD);
        eprintln!("After SWGEN0: PC={pc_b:#06x} IRQSTAT={irqstat_b:#010x} MB0={mb0_b:#010x} (PC moved={})", pc_b != pc_a);

        // Experiment 3: Also enable + trigger SWGEN1 (bit 7)
        let _ = bar0.write_u32(SEC2_BASE + 0x010, 1u32 << 7); // IRQMSET: enable SWGEN1
        let _ = bar0.write_u32(SEC2_BASE + 0x000, 1u32 << 7); // IRQSSET: trigger SWGEN1
        std::thread::sleep(std::time::Duration::from_millis(200));
        let pc_c = r(0x030);
        let irqstat_c = r(0x008);
        let mb0_c = bar0.read_u32(SEC2_BASE + 0x040).unwrap_or(0xDEAD);
        eprintln!("After SWGEN1: PC={pc_c:#06x} IRQSTAT={irqstat_c:#010x} MB0={mb0_c:#010x} (PC moved={})", pc_c != pc_b);

        // Experiment 4: Enable EXT interrupt (bit 0, external/engine interrupt)
        let _ = bar0.write_u32(SEC2_BASE + 0x010, 1u32 << 0); // IRQMSET: enable EXT(0)
        let _ = bar0.write_u32(SEC2_BASE + 0x000, 1u32 << 0); // IRQSSET: trigger EXT(0)
        std::thread::sleep(std::time::Duration::from_millis(200));
        let pc_d = r(0x030);
        let irqstat_d = r(0x008);
        let mb0_d = bar0.read_u32(SEC2_BASE + 0x040).unwrap_or(0xDEAD);
        eprintln!("After EXT(0): PC={pc_d:#06x} IRQSTAT={irqstat_d:#010x} MB0={mb0_d:#010x} (PC moved={})", pc_d != pc_c);

        // Final state
        let irqmask_fin = r(0x018);
        let irqstat_fin = r(0x008);
        let cpu_fin = r(0x100);
        let pc_fin = r(0x030);
        eprintln!("Final: PC={pc_fin:#06x} CPUCTL={cpu_fin:#010x} IRQMASK={irqmask_fin:#010x} IRQSTAT={irqstat_fin:#010x}");

        // Check if queues came alive after interaction (Exp 089b: CMDQ=0xA00, MSGQ=0xA30)
        let cmdq_h2 = bar0.read_u32(SEC2_BASE + 0xA00).unwrap_or(0);
        let cmdq_t2 = bar0.read_u32(SEC2_BASE + 0xA04).unwrap_or(0);
        let msgq_h2 = bar0.read_u32(SEC2_BASE + 0xA30).unwrap_or(0);
        let msgq_t2 = bar0.read_u32(SEC2_BASE + 0xA34).unwrap_or(0);
        let q_alive2 = cmdq_h2 != 0 || cmdq_t2 != 0 || msgq_h2 != 0 || msgq_t2 != 0;
        eprintln!("Queues after interaction: CMDQ h={cmdq_h2:#x} t={cmdq_t2:#x} | MSGQ h={msgq_h2:#x} t={msgq_t2:#x} alive={q_alive2}");

        // Re-read EMEM to see if firmware wrote anything new
        {
            use coral_driver::nv::vfio_compute::acr_boot::sec2_emem_read;
            let emem_int = sec2_emem_read(bar0, 0, 64);
            let nz_int = emem_int.iter().filter(|&&w| w != 0).count();
            if nz_int > 0 {
                eprintln!("EMEM after interaction: {nz_int}/64 non-zero");
                for (i, chunk) in emem_int.chunks(8).enumerate() {
                    let any_nz = chunk.iter().any(|&w| w != 0);
                    if any_nz {
                        let vals: Vec<String> = chunk.iter().map(|w| format!("{w:#010x}")).collect();
                        eprintln!("  EMEM[{:3}..{:3}]: {}", i*8, i*8+8, vals.join(" "));
                    }
                }
            }
        }
    }

    // ── Phase 4: BOOTSTRAP_FALCON (mailbox + CMDQ, try both) ──
    let sec2_c = r(0x100);
    let sec2_running = sec2_c & 0x30 == 0;
    if sec2_running {
        eprintln!("\n── Phase 4: BOOTSTRAP_FALCON commands ──");

        // IMEM probe: Falcon IMEM uses block addressing (off>>6 per 64-byte block)
        // Read mode: write IMEMC = (block_addr >> 6) | (1<<25), then read 16 words
        let imem_read_block = |pc: u32| -> Vec<u32> {
            let block = pc & !0x3F;
            let _ = bar0.write_u32(SEC2_BASE + 0x180, (block >> 6) | (1 << 25));
            let mut words = Vec::new();
            for _ in 0..16 {
                words.push(bar0.read_u32(SEC2_BASE + 0x184).unwrap_or(0xDEAD));
            }
            words
        };

        // Read the current idle loop PC and key trace PCs
        let idle_pc = r(0x030);
        eprintln!("IMEM probe (block-addressed):");
        for &(label, pc) in &[
            ("idle", idle_pc),
            ("0x2d07", 0x2d07),
            ("0x1c32", 0x1c32),
            ("0x4e5a", 0x4e5a),
        ] {
            let block = imem_read_block(pc);
            let word_idx = ((pc & 0x3F) >> 2) as usize;
            let w0 = block.get(word_idx).copied().unwrap_or(0);
            let w1 = block.get(word_idx + 1).copied().unwrap_or(0);
            let all_zero = block.iter().all(|&w| w == 0);
            eprintln!("  IMEM[{label}@{pc:#06x}]: [{word_idx}]={w0:#010x} [{next}]={w1:#010x} block_zero={all_zero}",
                next = word_idx + 1);
        }

        // Scan IMEM to find ANY non-zero region (is IMEM populated at all?)
        let mut nz_blocks = 0u32;
        let mut first_nz = 0u32;
        for blk in 0..1024u32 {
            let _ = bar0.write_u32(SEC2_BASE + 0x180, blk | (1 << 25));
            let w = bar0.read_u32(SEC2_BASE + 0x184).unwrap_or(0);
            if w != 0 {
                nz_blocks += 1;
                if first_nz == 0 { first_nz = blk * 64; }
            }
        }
        eprintln!("  IMEM scan: {nz_blocks}/1024 non-zero blocks, first_nz_addr={first_nz:#x}");

        // Strategy A: Mailbox BOOTSTRAP_FALCON (from strategy_mailbox.rs)
        eprintln!("Strategy A: Mailbox BOOTSTRAP_FALCON");
        let falcon_mask = (1u32 << 2) | (1u32 << 3); // FECS=2, GPCCS=3
        let _ = bar0.write_u32(SEC2_BASE + 0x044, falcon_mask); // MB1 = falcon mask
        let _ = bar0.write_u32(SEC2_BASE + 0x040, 1); // MB0 = BOOTSTRAP_FALCON cmd
        let _ = bar0.write_u32(SEC2_BASE + 0x000, 1u32 << 6); // IRQSSET SWGEN0
        std::thread::sleep(std::time::Duration::from_millis(500));
        let mb0_a = bar0.read_u32(SEC2_BASE + 0x040).unwrap_or(0);
        let mb1_a = bar0.read_u32(SEC2_BASE + 0x044).unwrap_or(0);
        let pc_a = r(0x030);
        let fecs_a = bar0.read_u32(FECS_BASE + 0x100).unwrap_or(0xDEAD);
        let gpccs_a = bar0.read_u32(GPCCS_BASE + 0x100).unwrap_or(0xDEAD);
        eprintln!("  After 500ms: MB0={mb0_a:#x} MB1={mb1_a:#x} PC={pc_a:#06x}");
        eprintln!("  FECS={fecs_a:#010x} GPCCS={gpccs_a:#010x}");
        if fecs_a & 0x10 == 0 { eprintln!("  *** FECS LEFT HRESET! ***"); }

        // Strategy B: CMDQ push (force even if h==t==0)
        eprintln!("Strategy B: CMDQ BOOTSTRAP_FALCON (forced)");
        const NV_SEC2_UNIT_ACR: u8 = 0x07;
        let send_bootstrap = |falcon_id: u32, seq: u8, label: &str| {
            let tail = bar0.read_u32(SEC2_BASE + 0xA04).unwrap_or(0);
            let cmd: [u32; 4] = [
                u32::from_le_bytes([NV_SEC2_UNIT_ACR, 16, 0x00, seq]),
                0x01, // BOOTSTRAP_FALCON
                0x01, // RESET_YES
                falcon_id,
            ];
            // Write cmd to EMEM at tail offset
            let _ = bar0.write_u32(SEC2_BASE + 0xAC0, tail | (1 << 24));
            for &w in &cmd {
                let _ = bar0.write_u32(SEC2_BASE + 0xAC4, w);
            }
            let new_tail = tail + 16;
            let _ = bar0.write_u32(SEC2_BASE + 0xA04, new_tail);
            let _ = bar0.write_u32(SEC2_BASE + 0x000, 1u32 << 6); // IRQSSET SWGEN0
            eprintln!("  {label}: falcon_id={falcon_id} @tail={tail:#x}→{new_tail:#x}");
        };
        send_bootstrap(3, 1, "GPCCS");
        std::thread::sleep(std::time::Duration::from_secs(1));
        let fecs_b = bar0.read_u32(FECS_BASE + 0x100).unwrap_or(0xDEAD);
        let gpccs_b = bar0.read_u32(GPCCS_BASE + 0x100).unwrap_or(0xDEAD);
        eprintln!("  After GPCCS: FECS={fecs_b:#010x} GPCCS={gpccs_b:#010x}");

        send_bootstrap(2, 2, "FECS");
        std::thread::sleep(std::time::Duration::from_secs(1));

        // Strategy C: Also try unit_id=0x08 (seen in sec2_cmdq.rs)
        eprintln!("Strategy C: CMDQ with unit_id=0x08");
        {
            let tail = bar0.read_u32(SEC2_BASE + 0xA04).unwrap_or(0);
            let cmd: [u32; 4] = [
                0x0003_1008, // seq=0, ctrl=0x03, size=0x10, unit=0x08
                0x0000_0000, // cmd_type=0x00(BOOTSTRAP)
                0x0000_0000, // flags=RESET_YES
                0x0000_0003, // falcon_id=GPCCS
            ];
            let _ = bar0.write_u32(SEC2_BASE + 0xAC0, tail | (1 << 24));
            for &w in &cmd {
                let _ = bar0.write_u32(SEC2_BASE + 0xAC4, w);
            }
            let _ = bar0.write_u32(SEC2_BASE + 0xA04, tail + 16);
            let _ = bar0.write_u32(SEC2_BASE + 0x000, 1u32 << 6);
        }
        std::thread::sleep(std::time::Duration::from_secs(1));

        // Final check
        let fecs_fin = bar0.read_u32(FECS_BASE + 0x100).unwrap_or(0xDEAD);
        let gpccs_fin = bar0.read_u32(GPCCS_BASE + 0x100).unwrap_or(0xDEAD);
        let pc_fin = r(0x030);
        let mb0_fin = bar0.read_u32(SEC2_BASE + 0x040).unwrap_or(0);
        let cpu_fin = r(0x100);
        eprintln!("Phase 4 final: FECS={fecs_fin:#010x} GPCCS={gpccs_fin:#010x} PC={pc_fin:#06x} MB0={mb0_fin:#x} cpuctl={cpu_fin:#010x}");
        if fecs_fin & 0x10 == 0 { eprintln!("*** FECS LEFT HRESET! ***"); }
        if gpccs_fin & 0x10 == 0 { eprintln!("*** GPCCS LEFT HRESET! ***"); }

        // TRACEPC after Phase 4
        let tidx_p4 = r(0x148);
        let nr_p4 = ((tidx_p4 & 0x00FF_0000) >> 16).min(32);
        if nr_p4 > 0 {
            let mut traces = Vec::new();
            for i in 0..nr_p4 {
                let _ = bar0.write_u32(SEC2_BASE + 0x148, i);
                let tpc = bar0.read_u32(SEC2_BASE + 0x14C).unwrap_or(0xDEAD);
                traces.push(format!("{tpc:#06x}"));
            }
            eprintln!("TRACEPC after Phase 4: {}", traces.join(" "));
        }
    } else {
        eprintln!("\nSEC2 not running (cpuctl={sec2_c:#010x})");
    }

    // ── Final state ──
    eprintln!("\n── Final state ──");
    let probe = dev.falcon_probe();
    eprintln!("{probe}");

    eprintln!("\n=== End Exp 095 ===");
}
