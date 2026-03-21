// SPDX-License-Identifier: AGPL-3.0-only
#![expect(
    missing_docs,
    reason = "PMU/FALCON registers mirror hardware; full docs planned"
)]
//! PMU FALCON registers, DevinitStatus, FalconDiagnostic, and execution.

use std::fmt::Write as FmtWrite;

use crate::vfio::device::MappedBar;

use super::script::interpret_boot_scripts;
use super::vbios::{
    BitTable, PROM_BASE, PROM_ENABLE_REG, read_vbios_file, read_vbios_prom, read_vbios_sysfs,
};

mod pmu_reg {
    pub const FALCON_CTRL: usize = 0x0010_A100;
    pub const FALCON_PC: usize = 0x0010_A104;
    pub const FALCON_TRIG: usize = 0x0010_A10C;
    pub const FALCON_MBOX0: usize = 0x0010_A040;
    pub const FALCON_MBOX1: usize = 0x0010_A044;
    pub const IMEM_PORT: usize = 0x0010_A180;
    pub const IMEM_DATA: usize = 0x0010_A184;
    pub const IMEM_TAG: usize = 0x0010_A188;
    pub const DMEM_PORT: usize = 0x0010_A1C0;
    pub const DMEM_DATA: usize = 0x0010_A1C4;

    pub const DEVINIT_STATUS: usize = 0x0002_240C;
    pub const FALCON_HWCFG: usize = 0x0010_A108;
    pub const FALCON_CPUCTL: usize = 0x0010_A100;
    pub const FALCON_ID: usize = 0x0010_A12C;
}

/// Devinit status check result.
#[derive(Debug, Clone)]
pub struct DevinitStatus {
    pub needs_post: bool,
    pub devinit_reg: u32,
    pub pmu_id: u32,
    pub pmu_hwcfg: u32,
    pub pmu_ctrl: u32,
    pub pmu_mbox0: u32,
}

impl DevinitStatus {
    /// Check the GPU's devinit status and PMU FALCON health.
    pub fn probe(bar0: &MappedBar) -> Self {
        let r = |reg| bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD);

        let devinit_reg = r(pmu_reg::DEVINIT_STATUS);
        let needs_post = (devinit_reg & 2) == 0;

        Self {
            needs_post,
            devinit_reg,
            pmu_id: r(pmu_reg::FALCON_ID),
            pmu_hwcfg: r(pmu_reg::FALCON_HWCFG),
            pmu_ctrl: r(pmu_reg::FALCON_CPUCTL),
            pmu_mbox0: r(pmu_reg::FALCON_MBOX0),
        }
    }

    /// Append devinit status lines (shared with [`FalconDiagnostic::print_report`]).
    pub(crate) fn write_summary_lines(&self, s: &mut String) {
        writeln!(
            s,
            "╠══ DEVINIT STATUS ══════════════════════════════════════════╣"
        )
        .expect("writing to String is infallible");
        writeln!(s, "║ devinit_reg[0x2240c]  = {:#010x}", self.devinit_reg)
            .expect("writing to String is infallible");
        writeln!(s, "║ needs_post (bit1==0)  = {}", self.needs_post)
            .expect("writing to String is infallible");
        writeln!(s, "║ PMU FALCON ID         = {:#010x}", self.pmu_id)
            .expect("writing to String is infallible");
        writeln!(s, "║ PMU FALCON HWCFG      = {:#010x}", self.pmu_hwcfg)
            .expect("writing to String is infallible");
        writeln!(s, "║ PMU FALCON CTRL       = {:#010x}", self.pmu_ctrl)
            .expect("writing to String is infallible");
        writeln!(s, "║ PMU MBOX0             = {:#010x}", self.pmu_mbox0)
            .expect("writing to String is infallible");
        if self.needs_post {
            writeln!(
                s,
                "║ *** GPU REQUIRES DEVINIT POST (HBM2 training not done) ***"
            )
            .expect("writing to String is infallible");
        } else {
            writeln!(
                s,
                "║ GPU devinit already complete — HBM2 should be trained."
            )
            .expect("writing to String is infallible");
        }
    }

    pub fn print_summary(&self) {
        let mut s = String::new();
        self.write_summary_lines(&mut s);
        tracing::info!(summary = %s, "devinit status");
    }

    /// Check if FALCON security bits indicate signed-only firmware is required.
    pub fn requires_signed_firmware(&self) -> bool {
        self.pmu_hwcfg & (1 << 8) != 0
    }

    /// Check if the PMU FALCON is halted (vs running).
    pub fn is_falcon_halted(&self) -> bool {
        self.pmu_ctrl & 0x10 != 0
    }
}

/// Comprehensive PMU FALCON diagnostic report.
#[derive(Debug, Clone)]
pub struct FalconDiagnostic {
    pub status: DevinitStatus,
    pub prom_accessible: bool,
    pub prom_signature: u32,
    pub prom_enable_reg: u32,
    pub secure_boot: bool,
    pub falcon_halted: bool,
    pub falcon_pc: u32,
    pub falcon_mbox1: u32,
    pub imem_size_kb: u32,
    pub dmem_size_kb: u32,
    pub vbios_sources: Vec<(String, bool, String)>,
}

impl FalconDiagnostic {
    /// Run comprehensive FALCON diagnostics.
    pub fn probe(bar0: &MappedBar, bdf: Option<&str>) -> Self {
        let r = |reg| bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD);

        let status = DevinitStatus::probe(bar0);

        // PROM accessibility
        let prom_enable_reg = r(PROM_ENABLE_REG);
        let _ = bar0.write_u32(PROM_ENABLE_REG, prom_enable_reg & !1);
        let prom_signature = r(PROM_BASE);
        let prom_accessible = (prom_signature & 0xFFFF) == 0xAA55;
        let _ = bar0.write_u32(PROM_ENABLE_REG, prom_enable_reg);

        // FALCON hardware config
        let hwcfg = status.pmu_hwcfg;
        let secure_boot = hwcfg & (1 << 8) != 0;
        let falcon_halted = status.pmu_ctrl & 0x10 != 0;
        let falcon_pc = r(pmu_reg::FALCON_PC);
        let falcon_mbox1 = r(pmu_reg::FALCON_MBOX1);

        // IMEM/DMEM sizes from HWCFG
        let imem_size_kb = ((hwcfg >> 16) & 0x1FF) * 256 / 1024;
        let dmem_size_kb = (hwcfg & 0x1FF) * 256 / 1024;

        // Check available VBIOS sources
        let mut vbios_sources = Vec::new();

        vbios_sources.push((
            "PROM (BAR0+0x300000)".into(),
            prom_accessible,
            if prom_accessible {
                format!("signature {prom_signature:#010x}")
            } else {
                format!("signature mismatch: {prom_signature:#010x}")
            },
        ));

        if let Some(bdf) = bdf {
            let rom_path = crate::linux_paths::sysfs_pci_device_file(bdf, "rom");
            let sysfs_ok = std::fs::metadata(&rom_path).is_ok();
            vbios_sources.push((
                format!("sysfs ({rom_path})"),
                sysfs_ok,
                if sysfs_ok {
                    "file exists".into()
                } else {
                    "not available".into()
                },
            ));
        }

        if let Ok(data_dir) = std::env::var("HOTSPRING_DATA_DIR") {
            let dump_names = ["vbios_0000_4a_00_0.bin", "vbios_0000_03_00_0.bin"];
            for name in &dump_names {
                let path = format!("{data_dir}/{name}");
                let exists = std::fs::metadata(&path).is_ok();
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                vbios_sources.push((
                    format!("file ({path})"),
                    exists,
                    if exists {
                        format!("{} KB", size / 1024)
                    } else {
                        "not found".into()
                    },
                ));
            }
        }

        Self {
            status,
            prom_accessible,
            prom_signature,
            prom_enable_reg,
            secure_boot,
            falcon_halted,
            falcon_pc,
            falcon_mbox1,
            imem_size_kb,
            dmem_size_kb,
            vbios_sources,
        }
    }

    /// Print a human-readable diagnostic report.
    pub fn print_report(&self) {
        let mut s = String::new();
        writeln!(
            &mut s,
            "╠══ PMU FALCON DIAGNOSTIC ═══════════════════════════════════╣"
        )
        .expect("writing to String is infallible");
        self.status.write_summary_lines(&mut s);
        writeln!(&mut s, "║").expect("writing to String is infallible");
        writeln!(&mut s, "║ FALCON Security:").expect("writing to String is infallible");
        writeln!(&mut s, "║   Secure boot required: {}", self.secure_boot)
            .expect("writing to String is infallible");
        writeln!(&mut s, "║   FALCON halted: {}", self.falcon_halted)
            .expect("writing to String is infallible");
        writeln!(&mut s, "║   FALCON PC: {:#010x}", self.falcon_pc)
            .expect("writing to String is infallible");
        writeln!(&mut s, "║   FALCON MBOX1: {:#010x}", self.falcon_mbox1)
            .expect("writing to String is infallible");
        writeln!(
            &mut s,
            "║   IMEM: {} KB, DMEM: {} KB",
            self.imem_size_kb, self.dmem_size_kb
        )
        .expect("writing to String is infallible");
        writeln!(&mut s, "║").expect("writing to String is infallible");
        writeln!(&mut s, "║ PROM Access:").expect("writing to String is infallible");
        writeln!(
            &mut s,
            "║   Enable reg (0x1854): {:#010x}",
            self.prom_enable_reg
        )
        .expect("writing to String is infallible");
        writeln!(
            &mut s,
            "║   PROM signature: {:#010x} ({})",
            self.prom_signature,
            if self.prom_accessible { "OK" } else { "FAIL" }
        )
        .expect("writing to String is infallible");
        writeln!(&mut s, "║").expect("writing to String is infallible");
        writeln!(&mut s, "║ VBIOS Sources:").expect("writing to String is infallible");
        for (name, ok, detail) in &self.vbios_sources {
            writeln!(
                &mut s,
                "║   {} {} — {}",
                if *ok { "✓" } else { "✗" },
                name,
                detail
            )
            .expect("writing to String is infallible");
        }
        writeln!(&mut s, "║").expect("writing to String is infallible");

        if self.status.needs_post {
            if self.secure_boot {
                writeln!(&mut s, "║ RECOMMENDATION: PMU requires signed firmware.")
                    .expect("writing to String is infallible");
                writeln!(
                    &mut s,
                    "║   → Use host-side VBIOS interpreter (interpret_boot_scripts)"
                )
                .expect("writing to String is infallible");
                writeln!(&mut s, "║   → Or use differential replay from oracle card")
                    .expect("writing to String is infallible");
            } else if self.prom_accessible {
                writeln!(&mut s, "║ RECOMMENDATION: FALCON upload should work.")
                    .expect("writing to String is infallible");
                writeln!(&mut s, "║   → Try execute_devinit() with PROM-read VBIOS")
                    .expect("writing to String is infallible");
            } else {
                writeln!(
                    &mut s,
                    "║ RECOMMENDATION: PROM inaccessible, FALCON unsigned."
                )
                .expect("writing to String is infallible");
                if self.vbios_sources.iter().any(|(_, ok, _)| *ok) {
                    writeln!(&mut s, "║   → Try execute_devinit() with file-based VBIOS")
                        .expect("writing to String is infallible");
                } else {
                    writeln!(
                        &mut s,
                        "║   → No VBIOS source available — try oracle replay"
                    )
                    .expect("writing to String is infallible");
                }
            }
        } else {
            writeln!(
                &mut s,
                "║ RECOMMENDATION: Devinit already complete, no action needed."
            )
            .expect("writing to String is infallible");
        }
        writeln!(
            &mut s,
            "╚═══════════════════════════════════════════════════════════╝"
        )
        .expect("writing to String is infallible");
        tracing::info!(summary = %s, "PMU FALCON diagnostic");
    }

    /// Find the best available VBIOS ROM, trying all sources.
    pub fn best_vbios(&self, bar0: &MappedBar, bdf: Option<&str>) -> Result<Vec<u8>, String> {
        if self.prom_accessible
            && let Ok(rom) = read_vbios_prom(bar0)
        {
            return Ok(rom);
        }

        if let Some(bdf) = bdf
            && let Ok(rom) = read_vbios_sysfs(bdf)
        {
            return Ok(rom);
        }

        for (name, ok, _) in &self.vbios_sources {
            if !ok {
                continue;
            }
            if let Some(path) = name
                .strip_prefix("file (")
                .and_then(|s| s.strip_suffix(')'))
                && let Ok(rom) = read_vbios_file(path)
            {
                return Ok(rom);
            }
        }

        Err("No VBIOS source available".into())
    }
}

/// Quick VRAM check via PRAMIN sentinel.
fn check_vram_via_pramin(bar0: &MappedBar) -> bool {
    use crate::vfio::memory::{MemoryRegion, PraminRegion};
    if let Ok(mut region) = PraminRegion::new(bar0, 0x0002_6000, 8) {
        region.probe_sentinel(0, 0xCAFE_DEAD).is_working()
    } else {
        false
    }
}

/// Execute devinit with enhanced diagnostics and automatic VBIOS source selection.
pub fn execute_devinit_with_diagnostics(
    bar0: &MappedBar,
    bdf: Option<&str>,
) -> Result<bool, String> {
    let diag = FalconDiagnostic::probe(bar0, bdf);
    diag.print_report();

    if !diag.status.needs_post {
        return Ok(false);
    }

    let rom = diag.best_vbios(bar0, bdf)?;

    if diag.secure_boot {
        tracing::info!("secure boot detected — using host-side VBIOS interpreter");
        let stats = interpret_boot_scripts(bar0, &rom)?;
        let vram_ok = check_vram_via_pramin(bar0);
        tracing::info!(
            writes = stats.writes_applied,
            vram = if vram_ok { "ALIVE" } else { "still dead" },
            "VBIOS interpreter result"
        );
        return Ok(vram_ok);
    }

    tracing::info!("attempting PMU FALCON devinit");
    match execute_devinit(bar0, &rom) {
        Ok(true) => {
            let vram_ok = check_vram_via_pramin(bar0);
            if vram_ok {
                tracing::info!("FALCON devinit succeeded + VRAM alive");
                return Ok(true);
            }
            tracing::warn!("FALCON devinit completed but VRAM still dead");
        }
        Ok(false) => {
            tracing::info!("FALCON reports devinit not needed");
            return Ok(false);
        }
        Err(e) => {
            tracing::error!(error = %e, "FALCON devinit failed");
        }
    }

    tracing::info!("falling back to host-side VBIOS interpreter");
    let stats = interpret_boot_scripts(bar0, &rom)?;
    let vram_ok = check_vram_via_pramin(bar0);
    tracing::info!(
        writes = stats.writes_applied,
        vram = if vram_ok { "ALIVE" } else { "still dead" },
        "VBIOS interpreter fallback result"
    );
    Ok(vram_ok)
}

// ── PMU FALCON execution ────────────────────────────────────────────────

/// Reset the PMU FALCON microcontroller.
pub fn pmu_falcon_reset(bar0: &MappedBar) {
    let r = |reg| bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD);
    let w = |reg, val| {
        let _ = bar0.write_u32(reg, val);
    };

    w(pmu_reg::FALCON_CTRL, 0x02);
    std::thread::sleep(std::time::Duration::from_millis(5));

    let ctrl = r(pmu_reg::FALCON_CTRL);
    tracing::debug!(ctrl = format!("{ctrl:#010x}"), "PMU FALCON CTRL after halt");
}

/// Upload code to PMU FALCON IMEM.
pub fn pmu_upload_code(
    bar0: &MappedBar,
    rom: &[u8],
    pmu_addr: u32,
    rom_offset: u32,
    size: u32,
    secure: bool,
) {
    let w = |reg, val: u32| {
        let _ = bar0.write_u32(reg, val);
    };

    let sec_flag: u32 = if secure { 0x1000_0000 } else { 0 };
    w(pmu_reg::IMEM_PORT, 0x0100_0000 | sec_flag | pmu_addr);

    let data = &rom[rom_offset as usize..(rom_offset + size) as usize];
    for (i, chunk) in data.chunks(4).enumerate() {
        let byte_offset = (i * 4) as u32;
        if byte_offset & 0xFF == 0 {
            w(pmu_reg::IMEM_TAG, (pmu_addr + byte_offset) >> 8);
        }

        let word = match chunk.len() {
            4 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            3 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]),
            2 => u32::from_le_bytes([chunk[0], chunk[1], 0, 0]),
            1 => u32::from_le_bytes([chunk[0], 0, 0, 0]),
            _ => 0,
        };
        w(pmu_reg::IMEM_DATA, word);
    }

    let total_words = (size as usize).div_ceil(4);
    let remainder = (total_words * 4) & 0xFF;
    if remainder != 0 {
        let padding_words = (256 - remainder) / 4;
        for _ in 0..padding_words {
            w(pmu_reg::IMEM_DATA, 0);
        }
    }
}

/// Upload data to PMU FALCON DMEM.
pub fn pmu_upload_data(bar0: &MappedBar, rom: &[u8], pmu_addr: u32, rom_offset: u32, size: u32) {
    let w = |reg, val: u32| {
        let _ = bar0.write_u32(reg, val);
    };

    w(pmu_reg::DMEM_PORT, 0x0100_0000 | pmu_addr);

    let data = &rom[rom_offset as usize..(rom_offset + size) as usize];
    for chunk in data.chunks(4) {
        let word = match chunk.len() {
            4 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            3 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]),
            2 => u32::from_le_bytes([chunk[0], chunk[1], 0, 0]),
            1 => u32::from_le_bytes([chunk[0], 0, 0, 0]),
            _ => 0,
        };
        w(pmu_reg::DMEM_DATA, word);
    }
}

/// Read a DMEM argument pointer.
pub fn pmu_read_args(bar0: &MappedBar, argp: u32, argi: u32) -> u32 {
    let r = |reg| bar0.read_u32(reg).unwrap_or(0);
    let w = |reg, val: u32| {
        let _ = bar0.write_u32(reg, val);
    };

    w(pmu_reg::DMEM_PORT, argp);
    let indirect = r(pmu_reg::DMEM_DATA);
    w(pmu_reg::DMEM_PORT, indirect + argi);
    r(pmu_reg::DMEM_DATA)
}

/// Start PMU FALCON execution at the given address.
pub fn pmu_exec(bar0: &MappedBar, init_addr: u32) {
    let w = |reg, val: u32| {
        let _ = bar0.write_u32(reg, val);
    };
    w(pmu_reg::FALCON_PC, init_addr);
    w(pmu_reg::FALCON_TRIG, 0);
    w(pmu_reg::FALCON_CTRL, 0x02);
}

/// Execute the full devinit sequence via PMU FALCON.
///
/// Returns Ok(true) if devinit completed, Ok(false) if it wasn't needed,
/// or Err on failure.
pub fn execute_devinit(bar0: &MappedBar, rom: &[u8]) -> Result<bool, String> {
    use super::vbios::parse_pmu_table;

    let status = DevinitStatus::probe(bar0);
    status.print_summary();

    if !status.needs_post {
        tracing::info!("devinit already complete — skipping PMU upload");
        return Ok(false);
    }

    let bit = BitTable::parse(rom)?;
    tracing::debug!(entries = bit.entries.len(), "BIT table");
    for entry in &bit.entries {
        tracing::trace!(
            bit_id = entry.id,
            version = entry.version,
            data_offset = format!("{:#06x}", entry.data_offset),
            data_size = entry.data_size,
            "BIT entry"
        );
    }

    let bit_i = bit
        .find(b'I')
        .ok_or("BIT 'I' entry not found — cannot locate devinit scripts")?;

    if bit_i.version != 1 || bit_i.data_size < 0x1c {
        return Err(format!(
            "BIT 'I' entry: unexpected version {} or size {} (need ver=1, size>=0x1c)",
            bit_i.version, bit_i.data_size
        ));
    }

    let pmu_fws = parse_pmu_table(rom, &bit)?;
    tracing::debug!(count = pmu_fws.len(), "PMU firmware entries");
    for fw in &pmu_fws {
        tracing::trace!(
            app_type = format!("{:#04x}", fw.app_type),
            boot = format!(
                "{:#x}+{:#x}({})",
                fw.boot_addr_pmu, fw.boot_addr, fw.boot_size
            ),
            code = format!(
                "{:#x}+{:#x}({})",
                fw.code_addr_pmu, fw.code_addr, fw.code_size
            ),
            data = format!(
                "{:#x}+{:#x}({})",
                fw.data_addr_pmu, fw.data_addr, fw.data_size
            ),
            init = format!("{:#x}", fw.init_addr_pmu),
            args = format!("{:#x}", fw.args_addr_pmu),
            "PMU firmware section"
        );
    }

    let devinit_fw = pmu_fws
        .iter()
        .find(|fw| fw.app_type == 0x04)
        .ok_or("PMU DEVINIT firmware (type 0x04) not found in VBIOS")?;

    let rom_len = rom.len() as u32;
    if devinit_fw.boot_addr + devinit_fw.boot_size > rom_len
        || devinit_fw.code_addr + devinit_fw.code_size > rom_len
        || devinit_fw.data_addr + devinit_fw.data_size > rom_len
    {
        return Err("DEVINIT firmware sections extend beyond ROM".into());
    }

    tracing::info!("PMU FALCON devinit upload starting");

    pmu_falcon_reset(bar0);

    tracing::debug!(
        bytes = devinit_fw.boot_size,
        addr = format!("{:#x}", devinit_fw.boot_addr_pmu),
        "uploading boot code to PMU IMEM"
    );
    pmu_upload_code(
        bar0,
        rom,
        devinit_fw.boot_addr_pmu,
        devinit_fw.boot_addr,
        devinit_fw.boot_size,
        false,
    );

    tracing::debug!(
        bytes = devinit_fw.code_size,
        addr = format!("{:#x}", devinit_fw.code_addr_pmu),
        "uploading main code to PMU IMEM"
    );
    pmu_upload_code(
        bar0,
        rom,
        devinit_fw.code_addr_pmu,
        devinit_fw.code_addr,
        devinit_fw.code_size,
        true,
    );

    tracing::debug!(
        bytes = devinit_fw.data_size,
        addr = format!("{:#x}", devinit_fw.data_addr_pmu),
        "uploading data to PMU DMEM"
    );
    pmu_upload_data(
        bar0,
        rom,
        devinit_fw.data_addr_pmu,
        devinit_fw.data_addr,
        devinit_fw.data_size,
    );

    let i_data_off = bit_i.data_offset as usize;
    let opcode_img = u16::from_le_bytes([
        rom.get(i_data_off + 0x14).copied().unwrap_or(0),
        rom.get(i_data_off + 0x15).copied().unwrap_or(0),
    ]) as u32;
    let opcode_len = u16::from_le_bytes([
        rom.get(i_data_off + 0x16).copied().unwrap_or(0),
        rom.get(i_data_off + 0x17).copied().unwrap_or(0),
    ]) as u32;

    if opcode_len > 0 && opcode_img + opcode_len <= rom_len {
        let pmu_opcode_addr = pmu_read_args(bar0, devinit_fw.args_addr_pmu + 0x08, 0x08);
        tracing::trace!(
            bytes = opcode_len,
            rom_offset = format!("{:#x}", opcode_img),
            dmem = format!("{:#x}", pmu_opcode_addr),
            "uploading opcode tables"
        );
        pmu_upload_data(bar0, rom, pmu_opcode_addr, opcode_img, opcode_len);
    } else {
        tracing::debug!(
            img = format!("{opcode_img:#x}"),
            len = opcode_len,
            "no opcode table found"
        );
    }

    let script_img = u16::from_le_bytes([
        rom.get(i_data_off + 0x18).copied().unwrap_or(0),
        rom.get(i_data_off + 0x19).copied().unwrap_or(0),
    ]) as u32;
    let script_len = u16::from_le_bytes([
        rom.get(i_data_off + 0x1a).copied().unwrap_or(0),
        rom.get(i_data_off + 0x1b).copied().unwrap_or(0),
    ]) as u32;

    if script_len > 0 && script_img + script_len <= rom_len {
        let pmu_script_addr = pmu_read_args(bar0, devinit_fw.args_addr_pmu + 0x08, 0x10);
        tracing::trace!(
            bytes = script_len,
            rom_offset = format!("{:#x}", script_img),
            dmem = format!("{:#x}", pmu_script_addr),
            "uploading boot scripts"
        );
        pmu_upload_data(bar0, rom, pmu_script_addr, script_img, script_len);
    } else {
        tracing::debug!(
            img = format!("{script_img:#x}"),
            len = script_len,
            "no boot script found"
        );
    }

    tracing::info!("PMU devinit execution");
    let w = |reg, val: u32| {
        let _ = bar0.write_u32(reg, val);
    };
    let r = |reg| bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD);

    w(pmu_reg::FALCON_MBOX0, 0x0000_5000);
    pmu_exec(bar0, devinit_fw.init_addr_pmu);

    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(2);
    let mut completed = false;

    while start.elapsed() < timeout {
        let mbox = r(pmu_reg::FALCON_MBOX0);
        if mbox & 0x2000 != 0 {
            completed = true;
            tracing::info!(
                mbox0 = format!("{mbox:#010x}"),
                elapsed_ms = start.elapsed().as_millis(),
                "DEVINIT complete"
            );
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    if !completed {
        let mbox = r(pmu_reg::FALCON_MBOX0);
        let ctrl = r(pmu_reg::FALCON_CTRL);
        tracing::error!(
            mbox0 = format!("{mbox:#010x}"),
            ctrl = format!("{ctrl:#010x}"),
            "DEVINIT timeout"
        );
        return Err(format!(
            "PMU DEVINIT timed out after 2s (MBOX0={mbox:#010x})"
        ));
    }

    // Run PRE_OS app (type 0x01) — for fan control
    if let Some(preos_fw) = pmu_fws.iter().find(|fw| fw.app_type == 0x01) {
        tracing::info!("loading PRE_OS app (fan control)");
        if preos_fw.boot_addr + preos_fw.boot_size <= rom_len
            && preos_fw.code_addr + preos_fw.code_size <= rom_len
            && preos_fw.data_addr + preos_fw.data_size <= rom_len
        {
            pmu_falcon_reset(bar0);
            pmu_upload_code(
                bar0,
                rom,
                preos_fw.boot_addr_pmu,
                preos_fw.boot_addr,
                preos_fw.boot_size,
                false,
            );
            pmu_upload_code(
                bar0,
                rom,
                preos_fw.code_addr_pmu,
                preos_fw.code_addr,
                preos_fw.code_size,
                true,
            );
            pmu_upload_data(
                bar0,
                rom,
                preos_fw.data_addr_pmu,
                preos_fw.data_addr,
                preos_fw.data_size,
            );
            pmu_exec(bar0, preos_fw.init_addr_pmu);
            tracing::info!("PRE_OS app launched on PMU");
        }
    }

    let post_status = DevinitStatus::probe(bar0);
    if !post_status.needs_post {
        tracing::info!("devinit status register shows COMPLETE");
    } else {
        tracing::warn!("devinit status register still shows needs_post");
    }

    Ok(true)
}
