// SPDX-License-Identifier: AGPL-3.0-or-later
//! PRI ring management — init, fault clearing, topology, and diagnostics.

use crate::vfio::device::MappedBar;

/// GK210B VBIOS DEVINIT hub station register table.
///
/// Written BEFORE ring INIT. Extracted from GK210 VBIOS opcode trace;
/// other Kepler chips (GK104, GK106, GK208) may need different values.
static GK210_HUB_STATION_PARAMS: &[(u32, u32)] = &[
    (0x12_2400, 0x0011_CE20), // hub station PRIV_MASTER timeout
    (0x12_2480, 0xFE00_3000), // hub station PRIV_CG_IDLE_CG
    (0x12_2600, 0x0000_0800), // hub station PRIV_CG_STATUS
    (0x12_00A0, 0x0000_0001), // PRI ring master enable
    (0x12_231C, 0x0000_F000), // hub station timeout value 2
    (0x12_2204, 0x0000_0001), // hub station config
    (0x12_0060, 0x0000_0000), // PRI ring master reset
];

/// K80 VBIOS DEVINIT hub station parameters — must be written BEFORE ring INIT.
pub(super) fn write_kepler_hub_station_params(w: &dyn Fn(u32, u32)) {
    for &(reg, val) in GK210_HUB_STATION_PARAMS {
        w(reg, val);
    }
}

/// PRI ring timing mask table — nouveau `gk104_privring_init()`.
///
/// Each entry: (register, clear_mask, set_mask). Applied as read-modify-write
/// to configure station timeouts before ring enumeration.
static GK104_PRIVRING_TIMING: &[(u32, u32, u32)] = &[
    (0x12_2318, 0x0003_FFFF, 0x0000_1000),
    (0x12_231C, 0x0003_FFFF, 0x0000_0200),
    (0x12_2310, 0x0003_FFFF, 0x0000_0800),
    (0x12_2348, 0x0003_FFFF, 0x0000_0100),
    (0x12_23B0, 0x0003_FFFF, 0x0000_0FFF),
    (0x12_2348, 0x0003_FFFF, 0x0000_0200),
    (0x12_2358, 0x0003_FFFF, 0x0000_2880),
];

/// Configure PRI ring hub station timing parameters.
///
/// Matches nouveau's `gk104_privring_init()` — must run before any PRI
/// ring enumerate commands. Without these, station timeouts may be too
/// aggressive for GK210B's dual-die topology.
pub(super) fn gk104_privring_timing(r: &dyn Fn(u32) -> u32, w: &dyn Fn(u32, u32)) {
    for &(reg, clr, set) in GK104_PRIVRING_TIMING {
        let cur = r(reg);
        w(reg, (cur & !clr) | set);
    }
}

/// PRI ring INIT matching nouveau's sequence.
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
    let rop = r(0x12_0074);
    let gpc = r(0x12_0078);
    tracing::info!(
        hub_stations = format_args!("{hub:#010x}"),
        rop_stations = format_args!("{rop:#010x}"),
        gpc_stations = format_args!("{gpc:#010x}"),
        ok,
        "nouveau PRI ring init complete (0x70=hub, 0x74=rop, 0x78=gpc)"
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
#[expect(dead_code, reason = "diagnostic function for Kepler cold-boot debugging")]
pub(super) fn kepler_pri_ring_diag(_bar0: &MappedBar, r: &dyn Fn(u32) -> u32) {
    // PRI ring master topology (nouveau gk104_privring_intr):
    //   0x120070 = hub station count
    //   0x120074 = ROP/FBP station count
    //   0x120078 = GPC station count
    let rm_cmd = r(0x12004C);
    let rm_intr = r(0x120058);
    let rm_intr1 = r(0x12005C);
    let hub_count = r(0x120070);
    let rop_count = r(0x120074);
    let gpc_count = r(0x120078);

    // GPC topology fuses
    let fuse_gpc = r(0x022430);
    let fuse_tpc_gpc0 = r(0x022438);

    // PGRAPH GPC counts (GR HUB — only valid after PGOB disable)
    let gr_gpc_count = r(0x409604);
    let gr_tpc_in_gpc = r(0x409614);

    // PBUS status
    let pbus_intr = r(0x001100);
    let pbus_bar0_window = r(0x001700);

    // PLL status
    let pll0_ctrl = r(0x130000);
    let pll0_stat = r(0x130014);
    let clk_master_0 = r(0x137300);
    let clk_source = r(0x137100);

    tracing::warn!(
        rm_cmd = format_args!("{rm_cmd:#010x}"),
        rm_intr0 = format_args!("{rm_intr:#010x}"),
        rm_intr1 = format_args!("{rm_intr1:#010x}"),
        hub_stations = format_args!("{hub_count:#010x}"),
        rop_stations = format_args!("{rop_count:#010x}"),
        gpc_stations = format_args!("{gpc_count:#010x}"),
        "PRI ring master state (hub=0x70, rop=0x74, gpc=0x78)"
    );
    tracing::warn!(
        fuse_gpc = format_args!("{fuse_gpc:#010x}"),
        fuse_tpc0 = format_args!("{fuse_tpc_gpc0:#010x}"),
        gr_gpc_count = format_args!("{gr_gpc_count:#010x}"),
        gr_tpc = format_args!("{gr_tpc_in_gpc:#010x}"),
        "GPC topology (fuse + GR HUB)"
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

/// Scan GPC/TPC topology using the guarded BAR0 read callback.
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

/// Read GPC0 TPC count via sysfs `resource0` (independent of VFIO BAR mapping).
///
/// `bdf` must be the sysfs PCI id (e.g. `0000:01:00.0`). When `CORALREEF_PRI_DEBUG_BDF` is set,
/// that value overrides `bdf` for cross-checking a specific device.
/// Returns `None` if sysfs BAR0 is unavailable.
pub(super) fn sysfs_bar0_read_gpc0(bdf: &str) -> Option<u32> {
    use std::fs::File;
    use std::os::fd::AsFd;

    let bdf_resolved = std::env::var("CORALREEF_PRI_DEBUG_BDF").unwrap_or_else(|_| bdf.to_string());
    let sysfs = crate::linux_paths::sysfs_root();
    let path = format!("{sysfs}/bus/pci/devices/{bdf_resolved}/resource0");
    let file = File::open(&path).ok()?;

    const MAP_LEN: usize = 0x80_0000;
    const REG_OFF: usize = 0x50_2608;

    // SAFETY: `path` selects this GPU's sysfs `resource0`; map the first `MAP_LEN` bytes
    // read-only MAP_SHARED from offset 0 (normal BAR0 window).
    let map = unsafe {
        rustix::mm::mmap(
            std::ptr::null_mut(),
            MAP_LEN,
            rustix::mm::ProtFlags::READ,
            rustix::mm::MapFlags::SHARED,
            file.as_fd(),
            0,
        )
        .ok()?
    };

    // SAFETY: `REG_OFF + 4 <= MAP_LEN`; `map` spans `MAP_LEN` bytes from a valid mmap.
    let val = unsafe { std::ptr::read_volatile(map.cast::<u8>().add(REG_OFF).cast::<u32>()) };

    // SAFETY: unmmap paired with successful mmap above, same length.
    unsafe {
        let _ = rustix::mm::munmap(map, MAP_LEN);
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
    let system_path = crate::linux_paths::nvidia_firmware_path(chip, "sw_nonctx.bin");
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
    // station types (hub, ROP, GPC) BEFORE the master ACK.
    //
    // PRI ring master topology registers (from nouveau gk104_privring_intr):
    //   0x120070 = hub station count
    //   0x120074 = ROP/FBP station count
    //   0x120078 = GPC station count
    let hub_count = r(0x12_0070) & 0xFF;
    for i in 0..hub_count {
        let stat_reg = 0x12_2120 + i * 0x800;
        let stat = r(stat_reg);
        if stat != 0 && stat != 0xDEAD_DEAD {
            w(stat_reg + 4, 0x2);
        }
    }

    let rop_count = r(0x12_0074) & 0xFF;
    for i in 0..rop_count {
        let stat_reg = 0x12_4120 + i * 0x800;
        let stat = r(stat_reg);
        if stat != 0 && stat != 0xDEAD_DEAD {
            w(stat_reg + 4, 0x2);
        }
    }

    let gpc_count = r(0x12_0078) & 0xFF;
    for i in 0..gpc_count {
        let stat_reg = 0x12_8120 + i * 0x800;
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

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;

    struct MockRegs {
        regs: RefCell<HashMap<u32, u32>>,
        writes: RefCell<Vec<(u32, u32)>>,
    }

    impl MockRegs {
        fn new() -> Self {
            Self {
                regs: RefCell::new(HashMap::new()),
                writes: RefCell::new(Vec::new()),
            }
        }

        fn set(&self, addr: u32, val: u32) {
            self.regs.borrow_mut().insert(addr, val);
        }

        fn read_fn(&self) -> impl Fn(u32) -> u32 + '_ {
            |addr| self.regs.borrow().get(&addr).copied().unwrap_or(0xDEAD_DEAD)
        }

        fn write_fn(&self) -> impl Fn(u32, u32) + '_ {
            |addr, val| {
                self.regs.borrow_mut().insert(addr, val);
                self.writes.borrow_mut().push((addr, val));
            }
        }

        fn write_log(&self) -> Vec<(u32, u32)> {
            self.writes.borrow().clone()
        }
    }

    #[test]
    fn hub_station_params_writes_all_entries() {
        let mock = MockRegs::new();
        let w = mock.write_fn();
        write_kepler_hub_station_params(&w);
        let log = mock.write_log();
        assert_eq!(log.len(), GK210_HUB_STATION_PARAMS.len());
        for (i, &(reg, val)) in GK210_HUB_STATION_PARAMS.iter().enumerate() {
            assert_eq!(log[i], (reg, val));
        }
    }

    #[test]
    fn privring_timing_applies_masks_correctly() {
        let mock = MockRegs::new();
        for &(reg, _, _) in GK104_PRIVRING_TIMING {
            mock.set(reg, 0xFFFF_FFFF);
        }
        let r = mock.read_fn();
        let w = mock.write_fn();
        gk104_privring_timing(&r, &w);
        let log = mock.write_log();
        assert_eq!(log.len(), GK104_PRIVRING_TIMING.len());
        for (i, &(reg, clr, set)) in GK104_PRIVRING_TIMING.iter().enumerate() {
            let expected = (0xFFFF_FFFF & !clr) | set;
            assert_eq!(log[i], (reg, expected), "mismatch at step {i} reg {reg:#010x}");
        }
    }

    #[test]
    fn vbios_ring_init_returns_false_when_hub_dead() {
        let mock = MockRegs::new();
        mock.set(0x12_0058, 0); // no faults
        mock.set(0x12_0070, 0xDEAD_DEAD); // hub dead
        let r = mock.read_fn();
        let w = mock.write_fn();
        assert!(!vbios_pri_ring_init(&r, &w));
    }

    #[test]
    fn vbios_ring_init_returns_true_when_hub_alive() {
        let mock = MockRegs::new();
        mock.set(0x12_0058, 0);
        mock.set(0x12_0070, 3); // 3 hub stations = alive
        let r = mock.read_fn();
        let w = mock.write_fn();
        assert!(vbios_pri_ring_init(&r, &w));
        let log = mock.write_log();
        // Should have sent init command 0x03
        assert!(log.contains(&(0x12_004C, 0x03)));
    }
}
