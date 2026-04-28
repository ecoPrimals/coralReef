// SPDX-License-Identifier: AGPL-3.0-or-later
//! PRI ring management — init, fault clearing, topology, and diagnostics.

use crate::vfio::device::MappedBar;

/// K80 VBIOS DEVINIT hub station parameters — must be written BEFORE ring INIT.
pub(super) fn write_kepler_hub_station_params(w: &dyn Fn(u32, u32)) {
    w(0x12_2400, 0x0011_CE20);
    w(0x12_2480, 0xFE00_3000);
    w(0x12_2600, 0x0000_0800);
    w(0x12_00A0, 0x0000_0001);
    w(0x12_231C, 0x0000_F000);
    w(0x12_2204, 0x0000_0001);
    w(0x12_0060, 0x0000_0000);
}

/// PRI ring INIT using VBIOS command 0x03.
///
/// GK210 VBIOS uses command 0x03 (not 0x04 as nouveau). After a cold FLR,
/// command 0x04 never completes because the INIT token requires the bus
/// interface that only 0x03 activates. Returns true if topology was
/// discovered (hub station count > 0).
/// PRI ring init matching nouveau's `gf100_bus_init()`.
///
/// Sends command `0x04` (ENUMERATE_STATIONS_BC) and waits for bit 31 of
/// `PRI_RINGMASTER_INTSTAT0` to clear, indicating all stations have been
/// enumerated and are online.
pub(super) fn nouveau_pri_ring_init(r: &dyn Fn(u32) -> u32, w: &dyn Fn(u32, u32)) -> bool {
    const PRI_RINGMASTER_COMMAND: u32 = 0x12_004C;
    const PRI_RING_INTR_STATUS: u32 = 0x12_0058;

    // Match nouveau's gf100_bus_init: reset PBUS (PMC bit 1) to reinitialize
    // the PRI ring controller before sending the enumerate command.
    let pmc = r(0x200);
    if pmc != 0xDEAD_DEAD && pmc & 0x2 != 0 {
        w(0x200, pmc & !0x2); // clear bit 1 = PBUS off
        r(0x200);
        std::thread::sleep(std::time::Duration::from_millis(5));
        w(0x200, pmc | 0x2); // set bit 1 = PBUS on
        r(0x200);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    // ACK any pre-existing faults first
    let pre = r(PRI_RING_INTR_STATUS);
    if pre != 0 && pre != 0xDEAD_DEAD {
        w(PRI_RINGMASTER_COMMAND, 0x02);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    // nouveau: nvkm_wr32(device, 0x12004c, 0x00000004)
    w(PRI_RINGMASTER_COMMAND, 0x0000_0004);

    // nouveau: nvkm_msec(device, 2000, if (!(nvkm_rd32(device, 0x120058) & 0x80000000)) break)
    let mut ok = false;
    for _ in 0..200 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        let status = r(PRI_RING_INTR_STATUS);
        if status & 0x8000_0000 == 0 {
            ok = true;
            break;
        }
    }

    if !ok {
        tracing::warn!("nouveau PRI ring init: timeout waiting for bit 31 to clear");
    }

    let hub = r(0x12_0070);
    let gpc = r(0x12_0074);
    tracing::info!(
        hub_count = format_args!("{hub:#010x}"),
        gpc_count = format_args!("{gpc:#010x}"),
        ok,
        "nouveau PRI ring init complete"
    );

    ok && hub > 0 && hub < 0xBAD0_0000
}

pub(super) fn vbios_pri_ring_init(r: &dyn Fn(u32) -> u32, w: &dyn Fn(u32, u32)) -> bool {
    const PRI_RINGMASTER_COMMAND: u32 = 0x12_004C;
    const PRI_RING_INTR_STATUS: u32 = 0x12_0058;
    const VBIOS_INIT_CMD: u32 = 0x03;
    const ACK_CMD: u32 = 0x02;

    // Clear any pre-existing faults
    let pre = r(PRI_RING_INTR_STATUS);
    if pre != 0 && pre != 0xDEAD_DEAD {
        w(PRI_RINGMASTER_COMMAND, ACK_CMD);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    w(PRI_RINGMASTER_COMMAND, VBIOS_INIT_CMD);
    std::thread::sleep(std::time::Duration::from_millis(200));

    // ACK any faults from the init traversal
    let intr = r(PRI_RING_INTR_STATUS);
    if intr != 0 && intr != 0xDEAD_DEAD {
        w(PRI_RINGMASTER_COMMAND, ACK_CMD);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    // Topology check: hub station count > 0 means ring is alive
    let hub = r(0x12_0070);
    hub > 0 && hub < 0xBAD0_0000
}

/// Diagnostic dump: probe PRI ring master, GPC topology, PLL lock, and PBUS state.
pub(super) fn kepler_pri_ring_diag(bar0: &MappedBar, r: &dyn Fn(u32) -> u32) {
    // PRI ring master registers
    let rm_cmd = r(0x12004C);
    let rm_intr = r(0x120058);
    let rm_intr0 = r(0x120060);
    let rm_gpc_err = r(0x120078);
    let rm_fbp_err = r(0x120070);

    // GPC topology fuses
    let fuse_gpc = r(0x022430);
    let fuse_tpc_gpc0 = r(0x022438);

    // PGRAPH GPC counts
    let gr_gpc_count = r(0x409604);
    let gr_tpc_in_gpc = r(0x409614);

    // PBUS status
    let pbus_intr = r(0x001100);
    let pbus_bar0_window = r(0x001700);

    // Additional PLL status readback
    let pll0_ctrl = r(0x130000);
    let pll0_stat = r(0x130014);
    let clk_master_0 = r(0x137300);
    let clk_source = r(0x137100);

    tracing::warn!(
        rm_cmd = format_args!("{rm_cmd:#010x}"),
        rm_intr = format_args!("{rm_intr:#010x}"),
        rm_intr0 = format_args!("{rm_intr0:#010x}"),
        rm_gpc_err = format_args!("{rm_gpc_err:#010x}"),
        rm_fbp_err = format_args!("{rm_fbp_err:#010x}"),
        "PRI ring master state"
    );
    tracing::warn!(
        fuse_gpc = format_args!("{fuse_gpc:#010x}"),
        fuse_tpc0 = format_args!("{fuse_tpc_gpc0:#010x}"),
        gr_gpc_count = format_args!("{gr_gpc_count:#010x}"),
        gr_tpc = format_args!("{gr_tpc_in_gpc:#010x}"),
        "GPC topology"
    );
    tracing::warn!(
        pbus_intr = format_args!("{pbus_intr:#010x}"),
        bar0_win = format_args!("{pbus_bar0_window:#010x}"),
        pll0_ctrl = format_args!("{pll0_ctrl:#010x}"),
        pll0_stat = format_args!("{pll0_stat:#010x}"),
        clk_master = format_args!("{clk_master_0:#010x}"),
        clk_source = format_args!("{clk_source:#010x}"),
        "PLL/clock diagnostics"
    );
}

/// Read GPC0 TPC count via sysfs resource0 (independent of VFIO BAR mapping).
/// Returns `None` if sysfs BAR0 is unavailable.
pub(super) fn scan_gpc_topology(
    guard: &super::hardware_guard::GuardedBar<'_>,
) -> (u32, u32, [(u32, u32); 8]) {
    let r = guard.read_fn();
    let mut counts: [(u32, u32); 8] = [(0, 0); 8];
    let mut gpc_count = 0u32;
    let mut tpc_total = 0u32;
    for gpc in 0..8u32 {
        let tpc_reg = r(0x50_0000 + gpc * 0x8000 + 0x2608);
        let alive = tpc_reg != 0xDEAD_DEAD && tpc_reg & 0xBAD0_0000 != 0xBAD0_0000;
        if alive {
            let tpc_nr = tpc_reg & 0x1F;
            counts[gpc_count as usize] = (gpc, tpc_nr);
            gpc_count += 1;
            tpc_total += tpc_nr;
        }
    }
    (gpc_count, tpc_total, counts)
}

pub(super) fn sysfs_bar0_read_gpc0() -> Option<u32> {
    use std::fs::File;
    use std::os::fd::AsFd;

    let file = File::open("/sys/bus/pci/devices/0000:4c:00.0/resource0").ok()?;
    let map = unsafe {
        rustix::mm::mmap(
            std::ptr::null_mut(),
            0x80_0000,
            rustix::mm::ProtFlags::READ,
            rustix::mm::MapFlags::SHARED,
            file.as_fd(),
            0,
        )
        .ok()?
    };
    let val = unsafe { std::ptr::read_volatile(map.cast::<u8>().add(0x50_2608).cast::<u32>()) };
    unsafe {
        let _ = rustix::mm::munmap(map, 0x80_0000);
    }
    Some(val)
}

/// Apply `sw_nonctx.bin` register writes for a given chip (e.g. "gk210").
///
/// The file contains packed LE u32 pairs `(BAR0_addr, value)` that configure
/// GR engine registers to the state FECS firmware expects. Returns
/// `(applied, skipped)` counts.
pub(super) fn apply_sw_nonctx(
    guard: &super::hardware_guard::GuardedBar<'_>,
    chip: &str,
) -> (u32, u32) {
    let local_prefixed = format!(
        "{}/firmware/{chip}/{chip}_sw_nonctx.bin",
        env!("CARGO_MANIFEST_DIR")
    );
    let local_plain = format!(
        "{}/firmware/{chip}/sw_nonctx.bin",
        env!("CARGO_MANIFEST_DIR")
    );
    let system_path = format!("/lib/firmware/nvidia/{chip}/sw_nonctx.bin");
    let fw_paths: &[&str] = &[&local_prefixed, &local_plain, &system_path];

    let mut data = None;
    for path in fw_paths {
        if let Ok(d) = std::fs::read(path) {
            tracing::info!(path, bytes = d.len(), "loaded sw_nonctx.bin");
            data = Some(d);
            break;
        }
    }

    let Some(data) = data else {
        tracing::warn!(chip, "sw_nonctx.bin not found — FECS may fail to boot");
        return (0, 0);
    };

    let mut applied = 0u32;
    let mut skipped = 0u32;

    for chunk in data.chunks_exact(8) {
        let addr = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let value = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);

        if addr % 4 != 0 {
            skipped += 1;
            continue;
        }

        if let Err(e) = guard.write_u32(addr, value) {
            tracing::trace!(
                addr = format_args!("{addr:#010x}"),
                value = format_args!("{value:#010x}"),
                %e,
                "sw_nonctx write blocked by guard"
            );
            skipped += 1;
        } else {
            applied += 1;
        }
    }

    (applied, skipped)
}

pub(super) fn clear_pri_ring_faults(
    _bar0: &MappedBar,
    r: &dyn Fn(u32) -> u32,
    w: &dyn Fn(u32, u32),
) {
    const PRIV_RING_INTR_STATUS: u32 = 0x120058;
    const PRIV_RING_COMMAND: u32 = 0x12004C;
    const PRIV_RING_CMD_ACK: u32 = 0x2;

    let status = r(PRIV_RING_INTR_STATUS);
    if status == 0 || status == 0xDEAD_DEAD {
        return;
    }

    // Match nouveau gk104_privring_intr: clear per-station errors for ALL
    // station types (hub, GPC, FBP) BEFORE the master ACK.
    let hub_count = r(0x12_0070) & 0xFF;
    for i in 0..hub_count {
        let stat_reg = 0x12_2120 + i * 0x800;
        let stat = r(stat_reg);
        if stat != 0 && stat != 0xDEAD_DEAD {
            w(stat_reg + 4, 0x2);
        }
    }

    let gpc_count = r(0x12_0074) & 0xFF;
    for i in 0..gpc_count {
        let stat_reg = 0x12_8120 + i * 0x800;
        let stat = r(stat_reg);
        if stat != 0 && stat != 0xDEAD_DEAD {
            w(stat_reg + 4, 0x2);
        }
    }

    let fbp_count = r(0x12_0078) & 0xFF;
    for i in 0..fbp_count {
        let stat_reg = 0x13_0120 + i * 0x800;
        let stat = r(stat_reg);
        if stat != 0 && stat != 0xDEAD_DEAD {
            w(stat_reg + 4, 0x2);
        }
    }

    for attempt in 0..5 {
        w(PRIV_RING_COMMAND, PRIV_RING_CMD_ACK);
        std::thread::sleep(std::time::Duration::from_millis(20));
        let after = r(PRIV_RING_INTR_STATUS);
        if after == 0 {
            tracing::info!(
                attempt,
                before = format_args!("{status:#010x}"),
                "PRI ring faults cleared"
            );
            return;
        }
    }

    let after = r(PRIV_RING_INTR_STATUS);
    tracing::warn!(
        before = format_args!("{status:#010x}"),
        after = format_args!("{after:#010x}"),
        "PRI ring faults persist after 5 ACK attempts"
    );
}
