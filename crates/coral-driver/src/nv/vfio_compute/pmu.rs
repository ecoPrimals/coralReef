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

    // Step 1: Reset PMU via PTOP register 0x022210 (gf100_pmu_reset).
    // NOT via PMC bit 13 — the kernel uses 0x022210 bit 0 exclusively.
    {
        let cur = rd(0x02_2210);
        wr(0x02_2210, cur & !0x01); // clear bit 0 = PMU disable
        rd(0x02_2210);
        std::thread::sleep(std::time::Duration::from_millis(5));
        wr(0x02_2210, cur | 0x01); // set bit 0 = PMU enable
        rd(0x02_2210);
    }

    // Inhibit interrupts if PMU was previously running
    wr(PMU_IRQMASK, 0x0000_FFFF);

    // Wait for IMEM/DMEM scrubbing to complete (kernel: 0x10a10c & 0x06 == 0)
    let mut scrub_ok = false;
    for _ in 0..200 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        let hwcfg = rd(PMU_DMACTL); // 0x10a10c
        if hwcfg & 0x06 == 0 {
            scrub_ok = true;
            break;
        }
    }

    let cpuctl = rd(PMU_CPUCTL);
    tracing::info!(
        cpuctl = format_args!("{cpuctl:#010x}"),
        scrub_ok,
        "PMU after PTOP reset + scrub"
    );

    if !scrub_ok {
        tracing::warn!("PMU IMEM/DMEM scrub timed out");
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

    // Step 4: Start CPU (matching kernel gt215_pmu_init exactly)
    wr(PMU_DMACTL, 0x0000_0000); // DMACTL = DMA disabled
    wr(PMU_BOOTVEC, 0x0000_0000); // BOOTVEC = 0
    wr(PMU_CPUCTL, 0x0000_0002); // STARTCPU

    // Step 5: Wait for firmware ring configuration at 0x10a4d0 (host→pmu queue)
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
                "PMU firmware stopped"
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
        tracing::info!(
            send_cfg = format_args!("{send_cfg:#010x}"),
            recv_cfg = format_args!("{recv_cfg:#010x}"),
            "PMU firmware initialized — ring queues configured"
        );
        // Enable interrupts (kernel: nvkm_wr32(device, 0x10a010, 0x000000e0))
        wr(PMU_BASE + 0x010, 0x0000_00E0);
    } else {
        let cpuctl_final = rd(PMU_CPUCTL);
        let pc = rd(PMU_BASE + 0x030);
        tracing::warn!(
            cpuctl = format_args!("{cpuctl_final:#010x}"),
            pc = format_args!("{pc:#010x}"),
            "PMU firmware boot failed"
        );
    }

    booted
}
