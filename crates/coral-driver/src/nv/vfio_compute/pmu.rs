// SPDX-License-Identifier: AGPL-3.0-or-later
//! GK110 PMU falcon firmware boot.

/// This sequence powers on the PGRAPH GR hub frontend (0x400xxx) by:
/// Boot the GK110 PMU falcon with firmware extracted from nouveau.ko.
///
/// The PMU firmware manages GPU power domains. Without it running, the PGOB
/// power-on sequence writes to hardware registers but GPC power gates don't
/// actually open. The firmware processes PGOB requests through its internal
/// state machine.
///
/// Steps: hardware reset → upload IMEM (code) → upload DMEM (data) →
/// set BOOTVEC=0 → STARTCPU → poll for firmware init complete.
pub(super) fn gk110_pmu_boot(guard: &super::hardware_guard::GuardedBar<'_>) -> bool {
    const PMU_BASE: u32 = 0x10_A000;
    const PMU_IMEM_TAG: u32 = PMU_BASE + 0x188;
    const PMU_IMEM_CTRL: u32 = PMU_BASE + 0x180;
    const PMU_IMEM_DATA: u32 = PMU_BASE + 0x184;
    const PMU_DMEM_CTRL: u32 = PMU_BASE + 0x1C0;
    const PMU_DMEM_DATA: u32 = PMU_BASE + 0x1C4;
    const PMU_CPUCTL: u32 = PMU_BASE + 0x100;
    const PMU_BOOTVEC: u32 = PMU_BASE + 0x104;
    const PMU_DMACTL: u32 = PMU_BASE + 0x10C;
    const PMU_IRQMASK: u32 = PMU_BASE + 0x014;

    static PMU_CODE: &[u8] = include_bytes!("../firmware/gk110_pmu_code.bin");
    static PMU_DATA: &[u8] = include_bytes!("../firmware/gk110_pmu_data.bin");

    let bar0 = guard.inner();
    let rd = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
    let wr = |reg: u32, val: u32| {
        let _ = bar0.write_u32(reg as usize, val);
    };

    tracing::info!(
        code_size = PMU_CODE.len(),
        data_size = PMU_DATA.len(),
        "PMU firmware boot: starting"
    );

    // Step 1: Reset PMU via PMC bit 13 (PDAEMON), matching the kernel's
    // nvkm_falcon_reset path in gt215_pmu_init. Previously used PTOP 0x022210
    // which left the falcon in HRESET where STARTCPU was silently ignored.
    {
        let pmc = rd(0x200);
        wr(0x200, pmc & !0x0000_2000); // clear bit 13 = PDAEMON off
        rd(0x200);
        std::thread::sleep(std::time::Duration::from_millis(20));
        wr(0x200, pmc | 0x0000_2000); // set bit 13 = PDAEMON on
        rd(0x200);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    // Inhibit interrupts during upload
    wr(PMU_IRQMASK, 0x0000_FFFF);

    // Wait for DMA idle (kernel: nvkm_rd32(device, 0x10a10c) & 0x06 == 0)
    let mut scrub_ok = false;
    for _ in 0..200 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        let dmactl = rd(PMU_DMACTL);
        if dmactl & 0x06 == 0 {
            scrub_ok = true;
            break;
        }
    }

    let cpuctl = rd(PMU_CPUCTL);
    tracing::info!(
        cpuctl = format_args!("{cpuctl:#010x}"),
        scrub_ok,
        halted = cpuctl & 0x10 != 0,
        hreset = cpuctl & 0x20 != 0,
        "PMU after PMC reset + DMA idle"
    );

    if !scrub_ok {
        tracing::warn!("PMU DMA idle wait timed out");
        return false;
    }

    // Step 2: Upload DMEM (data) first (kernel order: data before code)
    wr(PMU_DMEM_CTRL, 0x0100_0000); // address 0, auto-increment

    let data_words = PMU_DATA.len() / 4;
    for i in 0..data_words {
        let byte_addr = i * 4;
        let word = u32::from_le_bytes([
            PMU_DATA[byte_addr],
            PMU_DATA[byte_addr + 1],
            PMU_DATA[byte_addr + 2],
            PMU_DATA[byte_addr + 3],
        ]);
        wr(PMU_DMEM_DATA, word);
    }

    tracing::info!(words = data_words, "PMU DMEM uploaded");

    // Step 3: Upload IMEM (code) via PIO
    wr(PMU_IMEM_CTRL, 0x0100_0000); // address 0, auto-increment

    let code_words = PMU_CODE.len() / 4;
    for i in 0..code_words {
        // Update tag at every 64-word (256-byte) boundary
        if i & 0x3f == 0 {
            wr(PMU_IMEM_TAG, (i >> 6) as u32);
        }
        let byte_addr = i * 4;
        let word = u32::from_le_bytes([
            PMU_CODE[byte_addr],
            PMU_CODE[byte_addr + 1],
            PMU_CODE[byte_addr + 2],
            PMU_CODE[byte_addr + 3],
        ]);
        wr(PMU_IMEM_DATA, word);
    }

    tracing::info!(words = code_words, "PMU IMEM uploaded");

    // Step 4: Clear stale ring config (may be left from previous PMU run)
    wr(PMU_BASE + 0x4D0, 0x0000_0000);
    wr(PMU_BASE + 0x4DC, 0x0000_0000);

    // Step 5: Start CPU (matching kernel gt215_pmu_init exactly)
    wr(PMU_DMACTL, 0x0000_0000); // DMACTL = DMA disabled
    wr(PMU_BOOTVEC, 0x0000_0000); // BOOTVEC = 0
    wr(PMU_CPUCTL, 0x0000_0002); // STARTCPU

    let cpuctl_post = rd(PMU_CPUCTL);
    tracing::info!(
        cpuctl = format_args!("{cpuctl_post:#010x}"),
        running = cpuctl_post & 0x30 == 0,
        "PMU after STARTCPU"
    );

    // Step 6: Wait for firmware ring configuration at 0x10a4d0 (host→pmu queue)
    let mut booted = false;
    for i in 0..200 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        let ring_cfg = rd(PMU_BASE + 0x4D0);
        let cpuctl = rd(PMU_CPUCTL);

        if i % 20 == 0 || ring_cfg != 0 || cpuctl & 0x30 != 0 {
            tracing::info!(
                poll = i,
                cpuctl = format_args!("{cpuctl:#010x}"),
                ring_cfg = format_args!("{ring_cfg:#010x}"),
                "PMU boot poll"
            );
        }

        if ring_cfg != 0 && ring_cfg != 0xDEAD_DEAD {
            booted = true;
            break;
        }
        if cpuctl & 0x20 != 0 {
            tracing::warn!(
                cpuctl = format_args!("{cpuctl:#010x}"),
                "PMU firmware stopped (HRESET — STARTCPU failed)"
            );
            break;
        }
        if cpuctl & 0x10 != 0 {
            tracing::warn!(
                cpuctl = format_args!("{cpuctl:#010x}"),
                "PMU firmware halted"
            );
            break;
        }
    }

    if booted {
        let send_cfg = rd(PMU_BASE + 0x4D0);
        let recv_cfg = rd(PMU_BASE + 0x4DC);
        let cpuctl_final = rd(PMU_CPUCTL);
        let pc = rd(PMU_BASE + 0x110);
        let trap = rd(PMU_BASE + 0x028);
        let exci = rd(PMU_BASE + 0x04C);
        tracing::info!(
            send_cfg = format_args!("{send_cfg:#010x}"),
            recv_cfg = format_args!("{recv_cfg:#010x}"),
            cpuctl = format_args!("{cpuctl_final:#010x}"),
            pc = format_args!("{pc:#010x}"),
            trap = format_args!("{trap:#010x}"),
            exci = format_args!("{exci:#010x}"),
            still_running = cpuctl_final & 0x30 == 0,
            "PMU firmware initialized — ring queues configured"
        );
        // Enable interrupts (kernel: nvkm_wr32(device, 0x10a010, 0x000000e0))
        wr(PMU_BASE + 0x010, 0x0000_00E0);
    } else {
        let cpuctl_final = rd(PMU_CPUCTL);
        let pc = rd(PMU_BASE + 0x110);     // UC_PC (microcode PC)
        let epc = rd(PMU_BASE + 0x030);    // TRACEPC[0]
        let epc1 = rd(PMU_BASE + 0x034);   // TRACEPC[1]
        let exci = rd(PMU_BASE + 0x04C);   // EXC_INTR (exception info)
        let trap = rd(PMU_BASE + 0x028);    // EXCP_CAUSE
        let sctl = rd(PMU_BASE + 0x240);    // SCTL (engine specific)
        let fbif_ctrl = rd(PMU_BASE + 0x600); // FBIF control
        let fbif_stat = rd(PMU_BASE + 0x604); // FBIF status
        tracing::warn!(
            cpuctl = format_args!("{cpuctl_final:#010x}"),
            pc = format_args!("{pc:#010x}"),
            epc0 = format_args!("{epc:#010x}"),
            epc1 = format_args!("{epc1:#010x}"),
            exci = format_args!("{exci:#010x}"),
            trap = format_args!("{trap:#010x}"),
            sctl = format_args!("{sctl:#010x}"),
            fbif_ctrl = format_args!("{fbif_ctrl:#010x}"),
            fbif_stat = format_args!("{fbif_stat:#010x}"),
            "PMU firmware boot failed — crash diagnostics"
        );
    }

    booted
}
