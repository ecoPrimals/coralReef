// SPDX-License-Identifier: AGPL-3.0-or-later
//! K80 VFIO sovereign compute dispatch — write_42 with readback verification.
//!
//! Opens the K80 via `NvVfioComputeDevice::open` (which auto-selects the
//! Kepler channel path for SM35), compiles a write_42 WGSL shader targeting
//! `NvArch::Sm35`, dispatches via QMD V21 + Kepler pushbuf, reads back, and
//! asserts every element equals 42.
//!
//! Prerequisites:
//!   - K80 (`10de:102d`) bound to vfio-pci
//!   - Run as root or with VFIO group access
//!   - `CORALREEF_VFIO_BDF` env var or auto-scan finds the device

use coral_driver::nv::vfio_compute::NvVfioComputeDevice;
use coral_driver::{ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

use super::helpers::find_k80_devices;

fn open_k80_vfio() -> NvVfioComputeDevice {
    open_k80_vfio_warm()
}

fn open_k80_vfio_warm() -> NvVfioComputeDevice {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .try_init();

    let devices = find_k80_devices();
    assert!(!devices.is_empty(), "No K80 devices found on vfio-pci");

    let dev_path = &devices[0];
    let bdf = dev_path.rsplit('/').next().expect("BDF from sysfs path");

    eprintln!("Opening K80 via VFIO (warm legacy): {bdf}");
    NvVfioComputeDevice::open_warm_legacy(bdf, 0, 0)
        .expect("NvVfioComputeDevice::open_warm_legacy for K80")
}

fn open_k80_vfio_cold() -> NvVfioComputeDevice {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .try_init();

    let devices = find_k80_devices();
    assert!(!devices.is_empty(), "No K80 devices found on vfio-pci");

    let dev_path = &devices[0];
    let bdf = dev_path.rsplit('/').next().expect("BDF from sysfs path");

    eprintln!("Opening K80 via VFIO (cold boot): {bdf}");
    NvVfioComputeDevice::open_legacy(bdf, 0, 0).expect("NvVfioComputeDevice::open_legacy for K80")
}

#[test]
#[ignore = "requires K80 on vfio-pci — cold boot VFIO dispatch"]
fn k80_vfio_write_42_readback() {
    eprintln!("\n=== K80 VFIO Write-42 Readback (Cold Boot, SM35) ===\n");

    let mut dev = open_k80_vfio();
    let sm = dev.sm_version();
    eprintln!("K80 SM version: {sm}");
    assert!(sm <= 37, "Expected SM35/37 for K80, got SM{sm}");

    let wgsl = r"
@group(0) @binding(0)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = 42u;
}
";
    let opts = coral_reef::CompileOptions {
        target: coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm35),
        ..coral_reef::CompileOptions::default()
    };
    let compiled = coral_reef::compile_wgsl_full(wgsl, &opts).expect("compile write_42 for SM35");
    eprintln!(
        "Compiled: {} bytes, {} GPRs, format={:?}",
        compiled.binary.len(),
        compiled.info.gpr_count,
        compiled.format
    );

    let info = ShaderInfo {
        gpr_count: compiled.info.gpr_count,
        shared_mem_bytes: compiled.info.shared_mem_bytes,
        barrier_count: compiled.info.barrier_count,
        workgroup: compiled.info.local_size,
        wave_size: 32,
        local_mem_bytes: None,
    };

    let n = 64u64;
    let byte_size = n * 4;

    let buf = dev.alloc(byte_size, MemoryDomain::Vram).expect("alloc");
    dev.upload(buf, 0, &vec![0u8; byte_size as usize])
        .expect("zero buffer");

    // Check FECS state before dispatch
    let gr = dev.gr_engine_status();
    eprintln!("Pre-dispatch GR status: {gr}");
    if gr.fecs_halted() {
        eprintln!("FECS is HALTED — compute dispatch will fence-timeout");
        eprintln!("K80 FECS firmware may need additional GR init or VBIOS DEVINIT.");
    }

    eprintln!("Dispatching write_42 (64 elements, 1 workgroup)...");
    dev.dispatch(&compiled.binary, &[buf], DispatchDims::linear(1), &info)
        .expect("dispatch");
    dev.sync().expect("sync");
    eprintln!("Dispatch + sync succeeded");

    let data = dev.readback(buf, 0, byte_size as usize).expect("readback");
    let mut pass_count = 0u64;
    for i in 0..n as usize {
        let val = u32::from_le_bytes(data[i * 4..(i + 1) * 4].try_into().unwrap());
        assert_eq!(val, 42, "element {i}: expected 42, got {val}");
        pass_count += 1;
    }

    dev.free(buf).expect("free");

    eprintln!("****************************************************");
    eprintln!("*  ALL {pass_count} ELEMENTS = 42 — READBACK VERIFIED!     *");
    eprintln!("*  Sovereign VFIO compute on K80 (Kepler) PROVEN!  *");
    eprintln!("****************************************************");
    eprintln!("\n=== End K80 Dispatch ===");
}

#[test]
#[ignore = "requires K80 on vfio-pci — warm handoff device open"]
fn k80_vfio_device_opens() {
    let dev = open_k80_vfio();
    let sm = dev.sm_version();
    eprintln!("K80 VFIO device: SM{sm}");
    assert!(sm <= 37, "K80 should be SM35/37");
}

#[test]
#[ignore = "requires K80 on vfio-pci — cold boot with full PRI ring + FECS init"]
fn k80_vfio_cold_boot_device_opens() {
    let dev = open_k80_vfio_cold();
    let sm = dev.sm_version();
    eprintln!("K80 VFIO cold boot device: SM{sm}");
    assert!(sm <= 37, "K80 should be SM35/37");
}

#[test]
#[ignore = "requires K80 on vfio-pci — FLR diagnostic for PCLOCK"]
fn k80_vfio_flr_pclock_diagnostic() {
    use coral_driver::nv::vfio_compute::RawVfioDevice;

    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .try_init();

    let devices = find_k80_devices();
    assert!(!devices.is_empty(), "No K80 devices found on vfio-pci");

    let dev_path = &devices[0];
    let bdf = dev_path.rsplit('/').next().expect("BDF from sysfs path");
    eprintln!("\n=== K80 FLR PCLOCK Diagnostic: {bdf} ===\n");

    let raw = RawVfioDevice::open_legacy(bdf).expect("open raw device");
    let bar0 = &raw.bar0;
    let rd = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
    let wr = |reg: u32, val: u32| {
        let _ = bar0.write_u32(reg as usize, val);
    };

    // Pre-FLR state
    let boot0_pre = rd(0x000);
    let pmc_pre = rd(0x200);
    let pll0_pre = rd(0x13_0000);
    let seq_pre = rd(0x13_8000);
    let ref_pll_pre = rd(0xe800);
    eprintln!(
        "PRE-FLR: boot0={boot0_pre:#010x} pmc={pmc_pre:#010x} pll0={pll0_pre:#010x} seq={seq_pre:#010x} ref_pll={ref_pll_pre:#010x}"
    );

    // Try writing PLL0 before FLR
    wr(0x13_0000, 0x8000_0101);
    let pll0_test_pre = rd(0x13_0000);
    wr(0x13_0000, 0);
    eprintln!(
        "PRE-FLR PLL0 write test: readback={pll0_test_pre:#010x} writable={}",
        pll0_test_pre != 0
    );

    // Try FLR first, fall back to SBR
    eprintln!("\n--- Attempting device reset ---\n");
    let reset_ok = match raw.reset() {
        Ok(()) => {
            eprintln!("FLR succeeded");
            true
        }
        Err(e) => {
            eprintln!("FLR failed ({e}), trying PCI hot reset (SBR)...");
            match raw.pci_hot_reset() {
                Ok(()) => {
                    eprintln!("SBR succeeded");
                    true
                }
                Err(e2) => {
                    eprintln!("SBR also failed: {e2}");
                    false
                }
            }
        }
    };

    if !reset_ok {
        eprintln!("No reset method available. Aborting.");
        raw.leak();
        return;
    }

    std::thread::sleep(std::time::Duration::from_millis(500));

    // Re-enable bus master (reset clears PCI config)
    if let Err(e) = raw.enable_bus_master() {
        eprintln!("enable_bus_master failed after reset: {e}");
    }

    // Post-FLR state
    let boot0_post = rd(0x000);
    let pmc_post = rd(0x200);
    let pll0_post = rd(0x13_0000);
    let seq_post = rd(0x13_8000);
    let ref_pll_post = rd(0xe800);
    eprintln!(
        "POST-FLR: boot0={boot0_post:#010x} pmc={pmc_post:#010x} pll0={pll0_post:#010x} seq={seq_post:#010x} ref_pll={ref_pll_post:#010x}"
    );

    if boot0_post == 0xFFFF_FFFF || boot0_post == 0xDEAD_DEAD {
        eprintln!("!!! GPU DEAD after FLR — PCIe link lost !!!");
        raw.leak();
        return;
    }

    // Minimal PMC + PRI ring bootstrap
    wr(0x200, 0x0000_2020); // PDAEMON + PRING
    std::thread::sleep(std::time::Duration::from_millis(50));

    // PRI ring init hub station params
    wr(0x12_2400, 0x0011_CE20);
    wr(0x12_2480, 0xFE00_3000);
    wr(0x12_2600, 0x0000_0800);
    wr(0x12_00A0, 0x0000_0001);
    wr(0x12_231C, 0x0000_F000);
    wr(0x12_2204, 0x0000_0001);
    wr(0x12_0060, 0x0000_0000);

    // PRI ring init command (0x03)
    wr(0x12_004c, 0x0000_0003);
    std::thread::sleep(std::time::Duration::from_millis(50));
    let ring_status = rd(0x12_004c);
    eprintln!("POST-FLR PRI ring: status={ring_status:#010x}");

    // Enable full PMC
    wr(0x200, 0xe011_312c);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let pmc_full = rd(0x200);
    let pll0_full = rd(0x13_0000);
    let seq_full = rd(0x13_8000);
    let ref_pll_full = rd(0xe800);
    eprintln!(
        "POST-FLR + full PMC: pmc={pmc_full:#010x} pll0={pll0_full:#010x} seq={seq_full:#010x} ref_pll={ref_pll_full:#010x}"
    );

    // Critical test: can we write to PCLOCK PLLs after FLR?
    wr(0x13_0000, 0x8000_0101);
    let pll0_test_post = rd(0x13_0000);
    wr(0x13_0000, 0);
    eprintln!(
        "POST-FLR PLL0 write test: readback={pll0_test_post:#010x} writable={}",
        pll0_test_post != 0
    );

    // Also test a few other PCLOCK registers
    wr(0x13_7000, 0x0001_0000);
    let route_test = rd(0x13_7000);
    wr(0x13_7000, 0);
    eprintln!(
        "POST-FLR CLK_ROUTE write test: readback={route_test:#010x} writable={}",
        route_test != 0
    );

    wr(0x13_2000, 0x0001_0000);
    let dom_test = rd(0x13_2000);
    wr(0x13_2000, 0);
    eprintln!(
        "POST-FLR CLK_DOM write test: readback={dom_test:#010x} writable={}",
        dom_test != 0
    );

    // SEQ_CTRL write test
    wr(0x13_8000, 0x0000_0001);
    let seq_test = rd(0x13_8000);
    wr(0x13_8000, 0);
    eprintln!(
        "POST-FLR SEQ_CTRL write test: readback={seq_test:#010x} writable={}",
        seq_test != 0
    );

    eprintln!("\n=== End FLR PCLOCK Diagnostic ===");
    raw.leak();
}

#[test]
#[ignore = "requires K80 on vfio-pci — state-zero PCLOCK diagnostic"]
fn k80_vfio_state_zero_pclock() {
    use coral_driver::nv::vfio_compute::RawVfioDevice;

    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .try_init();

    let devices = find_k80_devices();
    assert!(!devices.is_empty(), "No K80 devices found on vfio-pci");

    let dev_path = &devices[0];
    let bdf = dev_path.rsplit('/').next().expect("BDF from sysfs path");
    eprintln!("\n=== K80 State-Zero PCLOCK Diagnostic: {bdf} ===\n");

    let raw = RawVfioDevice::open_legacy(bdf).expect("open raw device");
    let bar0 = &raw.bar0;
    let rd = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
    let wr = |reg: u32, val: u32| {
        let _ = bar0.write_u32(reg as usize, val);
    };

    // ── Phase 0: Read baseline (no writes) ──
    eprintln!("── Phase 0: Baseline state (read-only) ──");
    let boot0 = rd(0x000);
    let pmc = rd(0x200);
    let pmc2 = rd(0x640);
    let devinit_post = rd(0x02_240C);
    let devinit_1540 = rd(0x001540);
    eprintln!("BOOT0={boot0:#010x} PMC={pmc:#010x} PMC2={pmc2:#010x}");
    eprintln!(
        "DEVINIT_POST(0x2240c)={devinit_post:#010x} needs_post={} devinit_1540={devinit_1540:#010x}",
        devinit_post & 0x2 == 0
    );

    // PMU state
    let pmu_cpuctl = rd(0x10_a100);
    let pmu_engctl = rd(0x10_a200);
    let pmu_pgob = rd(0x10_a78c);
    eprintln!(
        "PMU: cpuctl={pmu_cpuctl:#010x} running={} halted={} engctl={pmu_engctl:#010x} pgob_ctrl={pmu_pgob:#010x}",
        pmu_cpuctl & 0x20 != 0,
        pmu_cpuctl & 0x10 != 0
    );

    // PGOB registers
    let pgob_520 = rd(0x02_0520);
    let pgob_524 = rd(0x02_0524);
    let pgob_528 = rd(0x02_0528);
    let pgob_52c = rd(0x02_052C);
    let pgob_530 = rd(0x02_0530);
    eprintln!(
        "PGOB: 520={pgob_520:#010x} 524={pgob_524:#010x} 528={pgob_528:#010x} 52c={pgob_52c:#010x} 530={pgob_530:#010x}"
    );

    // Reference PLLs (PNVIO domain — should be alive)
    let ref0 = rd(0xe800);
    let ref0_coef = rd(0xe804);
    let ref1 = rd(0xe820);
    let ref1_coef = rd(0xe824);
    eprintln!("REF_PLL0: ctrl={ref0:#010x} coef={ref0_coef:#010x}");
    eprintln!("REF_PLL1: ctrl={ref1:#010x} coef={ref1_coef:#010x}");

    // PCLOCK sequencer / master
    let seq_ctrl = rd(0x13_8000);
    let seq_cfg0 = rd(0x13_8004);
    let seq_cfg1 = rd(0x13_8008);
    let cg0 = rd(0x13_9000);
    let cg1 = rd(0x13_9004);
    eprintln!(
        "SEQ: ctrl={seq_ctrl:#010x} cfg0={seq_cfg0:#010x} cfg1={seq_cfg1:#010x} CG0={cg0:#010x} CG1={cg1:#010x}"
    );

    // PCLOCK core PLLs (0x130000 range — previously dead)
    let pll0_ctrl = rd(0x13_0000);
    let pll0_coef = rd(0x13_0004);
    let pll0_lock = rd(0x13_0014);
    eprintln!(
        "PLL0(0x130000): ctrl={pll0_ctrl:#010x} coef={pll0_coef:#010x} lock={pll0_lock:#010x}"
    );

    // Nouveau's actual engine PLLs: 0x137000 + idx * 0x20
    let epll_names = [
        (0x13_7000, "GPC_PLL"),
        (0x13_7020, "ROP_PLL"),
        (0x13_7040, "HUB07_PLL"),
        (0x13_70E0, "HUB06_PLL"),
    ];
    for &(addr, name) in &epll_names {
        let ctrl = rd(addr);
        let coef = rd(addr + 4);
        let lock = rd(addr + 0x14);
        eprintln!("{name}({addr:#010x}): ctrl={ctrl:#010x} coef={coef:#010x} lock={lock:#010x}");
    }

    // Clock routing/dividers/selectors (Nouveau gk104_clk paths)
    let clk_sel = rd(0x13_7100);
    let clk_div0 = rd(0x13_7250);
    let clk_src0 = rd(0x13_7160);
    let clk_ddiv0 = rd(0x13_71D0);
    let clk_src_div0 = rd(0x13_7120);
    let clk_src_div1 = rd(0x13_7140);
    eprintln!("CLK_SEL(0x137100)={clk_sel:#010x} DIV0(0x137250)={clk_div0:#010x}");
    eprintln!("SRC0(0x137160)={clk_src0:#010x} DDIV0(0x1371D0)={clk_ddiv0:#010x}");
    eprintln!("SRC_DIV0(0x137120)={clk_src_div0:#010x} SRC_DIV1(0x137140)={clk_src_div1:#010x}");

    // Memory PLLs
    let mclk_sel = rd(0x13_73f4);
    let mpll0 = rd(0x13_2000);
    let mpll1 = rd(0x13_2020);
    eprintln!(
        "MCLK_SEL(0x1373f4)={mclk_sel:#010x} MPLL0(0x132000)={mpll0:#010x} MPLL1(0x132020)={mpll1:#010x}"
    );

    // PRI ring status
    let pri_ring = rd(0x12_004c);
    let pri_intr = rd(0x12_0058);
    let pri_hubs = rd(0x12_0070);
    eprintln!("PRI: ring_cmd={pri_ring:#010x} intr={pri_intr:#010x} hubs={pri_hubs:#010x}");

    // PBUS
    let pbus_debug0 = rd(0x001084);
    let pbus_debug1 = rd(0x001098);
    eprintln!("PBUS: debug0={pbus_debug0:#010x} debug1={pbus_debug1:#010x}");

    // ── Phase 1: Write tests at Nouveau's actual PLL addresses ──
    eprintln!("\n── Phase 1: PCLOCK write tests (at Nouveau's PLL addresses) ──");

    let test_regs: &[(u32, &str)] = &[
        (0x13_0000, "PLL0_CTRL"),
        (0x13_7000, "GPC_PLL_CTRL"),
        (0x13_7004, "GPC_PLL_COEF"),
        (0x13_7020, "ROP_PLL_CTRL"),
        (0x13_7100, "CLK_SEL"),
        (0x13_7160, "CLK_SRC0"),
        (0x13_71D0, "CLK_DDIV0"),
        (0x13_7250, "CLK_DIV0"),
        (0x13_2000, "MPLL0"),
        (0x13_2020, "MPLL1"),
        (0x13_73F4, "MCLK_SEL"),
        (0x13_8000, "SEQ_CTRL"),
    ];

    for &(reg, name) in test_regs {
        let before = rd(reg);
        let test_val = 0x0000_0001_u32;
        wr(reg, test_val);
        let after = rd(reg);
        wr(reg, before); // restore
        eprintln!(
            "{name}({reg:#010x}): before={before:#010x} wrote=0x00000001 readback={after:#010x} writable={}",
            after != before || after == test_val
        );
    }

    // ── Phase 2: Try Nouveau's actual init ordering ──
    // Nouveau does: devinit → pmu boot → fb → clk → pgob (in gr)
    // Our key question: does PMU boot enable PCLOCK?
    eprintln!("\n── Phase 2: Check if PMU boot unlocks PCLOCK ──");

    // First, check if PMU is running Nouveau firmware
    let pmu_falcon_ver = rd(0x10_a12c);
    let pmu_fbif = rd(0x10_a600);
    let pmu_dscratch0 = rd(0x10_a040);
    let pmu_dscratch1 = rd(0x10_a044);
    eprintln!("PMU: falcon_ver={pmu_falcon_ver:#010x} fbif={pmu_fbif:#010x}");
    eprintln!("PMU: scratch0={pmu_dscratch0:#010x} scratch1={pmu_dscratch1:#010x}");

    // If PMU is halted, try booting it
    if pmu_cpuctl & 0x10 != 0 {
        eprintln!("PMU is HALTED — attempting to start...");
        wr(0x10_a104, 0x0000_0002); // STARTCPU
        std::thread::sleep(std::time::Duration::from_millis(100));
        let pmu_after = rd(0x10_a100);
        eprintln!(
            "PMU after STARTCPU: cpuctl={pmu_after:#010x} running={}",
            pmu_after & 0x20 != 0
        );
    }

    // Test PCLOCK after PMU state check
    wr(0x13_7000, 0x0000_0001);
    let gpc_pll_test = rd(0x13_7000);
    wr(0x13_7000, 0);
    eprintln!(
        "After PMU check: GPC_PLL write test readback={gpc_pll_test:#010x} writable={}",
        gpc_pll_test != 0
    );

    // ── Phase 3: Try PGOB-first approach ──
    // What if PGOB controls not just PGRAPH but also PCLOCK access?
    eprintln!("\n── Phase 3: PGOB state and PMC bit 27 experiment ──");
    let pmc_now = rd(0x200);
    eprintln!("PMC before PGOB experiment: {pmc_now:#010x}");
    eprintln!("PMC bit 12 (PGRAPH): {}", pmc_now & 0x1000 != 0);
    eprintln!("PMC bit 27 (PGOB gate): {}", pmc_now & 0x0800_0000 != 0);
    eprintln!("PMC bit 13 (PDAEMON): {}", pmc_now & 0x2000 != 0);

    eprintln!("\n=== End State-Zero PCLOCK Diagnostic ===");
    raw.leak();
}
