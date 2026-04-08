// SPDX-License-Identifier: AGPL-3.0-only
//! Falcon capability discovery — runtime bit solver for PIO register layouts.
//!
//! Instead of hardcoding bit positions (which vary across falcon versions and
//! caused the IMEMC BIT(24) vs BIT(6) bug), this module probes actual hardware
//! to discover the correct register format for each falcon instance.
//!
//! Each falcon self-describes: version, PIO protocol, CPUCTL layout, security
//! state, and memory sizes are all discovered at runtime. No global tables of
//! "GV100 uses this, Blackwell uses that" — the hardware tells us.

use std::fmt;

use crate::error::{DriverError, DriverResult};
use crate::vfio::channel::registers::falcon;
use crate::vfio::device::MappedBar;

/// Opaque IMEMC/DMEMC/EMEMC control word — can only be constructed through
/// [`FalconCapabilities`], preventing wrong-format bugs at compile time.
#[derive(Clone, Copy)]
pub struct PioCtrl(u32);

impl PioCtrl {
    /// Raw u32 value for writing to the hardware control register.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

impl fmt::Debug for PioCtrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PioCtrl({:#010x})", self.0)
    }
}

/// Falcon security mode as decoded from SCTL/SEC_MODE register.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityMode {
    /// Non-secure: host has full control, unsigned firmware accepted.
    NonSecure,
    /// Light Secure: fuse-enforced, PIO works but firmware auth required for HS ops.
    LightSecure,
    /// Heavy Secure: ACR-managed, host PIO may have restrictions.
    HeavySecure,
    /// Unknown security level — SCTL bits decode to an undocumented value.
    Unknown(u32),
}

impl fmt::Display for SecurityMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonSecure => write!(f, "NS"),
            Self::LightSecure => write!(f, "LS"),
            Self::HeavySecure => write!(f, "HS"),
            Self::Unknown(v) => write!(f, "UNKNOWN({v:#x})"),
        }
    }
}

/// Discovered falcon version from HWCFG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FalconVersion {
    /// Major version (v0..v5+). Determines CPUCTL layout and PIO protocol.
    pub major: u8,
    /// Raw HWCFG register value for reference.
    pub hwcfg_raw: u32,
}

/// CPUCTL bit layout — varies between falcon versions.
#[derive(Debug, Clone, Copy)]
pub struct CpuCtlLayout {
    /// Bit mask for STARTCPU (release from HRESET).
    pub startcpu: u32,
    /// Bit mask for IINVAL (instruction cache invalidate). Zero if not available.
    pub iinval: u32,
    /// Bit mask for HRESET (hard reset state indicator).
    pub hreset: u32,
    /// Bit mask for HALTED state indicator.
    pub halted: u32,
}

/// PIO (Programmed I/O) bit layout for IMEM/DMEM/EMEM control registers.
#[derive(Debug, Clone, Copy)]
pub struct PioLayout {
    /// Bit for auto-increment on write. Expected: BIT(24) = 0x0100_0000.
    pub write_autoinc: u32,
    /// Bit for auto-increment on read. Expected: BIT(25) = 0x0200_0000.
    pub read_autoinc: u32,
    /// Bit for marking IMEM pages as secure. Expected: BIT(28) = 0x1000_0000.
    pub secure_flag: u32,
    /// Whether PIO write+readback was validated on hardware.
    pub write_validated: bool,
    /// Whether PIO read mode was validated on hardware.
    pub read_validated: bool,
}

/// Complete discovered capabilities for a falcon instance.
#[derive(Debug, Clone)]
pub struct FalconCapabilities {
    /// Human-readable falcon name (FECS, GPCCS, SEC2, PMU).
    pub name: String,
    /// BAR0 base address of this falcon.
    pub base: usize,
    /// Falcon version.
    pub version: FalconVersion,
    /// CPUCTL register bit layout.
    pub cpuctl: CpuCtlLayout,
    /// PIO register bit layout.
    pub pio: PioLayout,
    /// Current security mode from SCTL.
    pub security: SecurityMode,
    /// Raw SCTL register value.
    pub sctl_raw: u32,
    /// IMEM size in bytes (from HWCFG).
    pub imem_size: u32,
    /// DMEM size in bytes (from HWCFG).
    pub dmem_size: u32,
    /// Whether HWCFG indicates signed firmware is required.
    pub requires_signed_fw: bool,
    /// Diagnostics: any anomalies found during probing.
    pub anomalies: Vec<String>,
}

impl FalconCapabilities {
    /// Construct a PIO control word for IMEM/DMEM **write** at `addr` with auto-increment.
    #[must_use]
    pub const fn imem_write_ctrl(&self, addr: u32) -> PioCtrl {
        PioCtrl(self.pio.write_autoinc | addr)
    }

    /// Construct a PIO control word for secure IMEM **write** at `addr`.
    #[must_use]
    pub const fn imem_write_secure_ctrl(&self, addr: u32) -> PioCtrl {
        PioCtrl(self.pio.write_autoinc | self.pio.secure_flag | addr)
    }

    /// Construct a PIO control word for IMEM/DMEM **read** at `addr`.
    #[must_use]
    pub const fn imem_read_ctrl(&self, addr: u32) -> PioCtrl {
        PioCtrl(self.pio.read_autoinc | addr)
    }

    /// Construct DMEMC control for write at `addr`.
    #[must_use]
    pub const fn dmem_write_ctrl(&self, addr: u32) -> PioCtrl {
        PioCtrl(self.pio.write_autoinc | addr)
    }

    /// Construct DMEMC control for read at `addr`.
    #[must_use]
    pub const fn dmem_read_ctrl(&self, addr: u32) -> PioCtrl {
        PioCtrl(self.pio.read_autoinc | addr)
    }

    /// Construct EMEMC control for write at `offset`.
    #[must_use]
    pub const fn emem_write_ctrl(&self, offset: u32) -> PioCtrl {
        PioCtrl(self.pio.write_autoinc | offset)
    }

    /// Construct EMEMC control for read at `offset`.
    #[must_use]
    pub const fn emem_read_ctrl(&self, offset: u32) -> PioCtrl {
        PioCtrl(self.pio.read_autoinc | offset)
    }

    /// CPUCTL value to start the falcon CPU.
    #[must_use]
    pub const fn startcpu_value(&self) -> u32 {
        self.cpuctl.startcpu
    }

    /// CPUCTL value to invalidate the instruction cache.
    #[must_use]
    pub const fn iinval_value(&self) -> u32 {
        self.cpuctl.iinval
    }

    /// Whether the falcon is in a state where host PIO is expected to work.
    #[must_use]
    pub const fn pio_accessible(&self) -> bool {
        self.pio.write_validated || self.pio.read_validated
    }

    /// Whether any anomalies were detected during probing.
    #[must_use]
    pub fn has_anomalies(&self) -> bool {
        !self.anomalies.is_empty()
    }
}

impl fmt::Display for FalconCapabilities {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} @ {:#08x}: v{} {} imem={}B dmem={}B signed={} \
             pio_w={:#010x}{} pio_r={:#010x}{} cpuctl_start={:#x}",
            self.name,
            self.base,
            self.version.major,
            self.security,
            self.imem_size,
            self.dmem_size,
            self.requires_signed_fw,
            self.pio.write_autoinc,
            if self.pio.write_validated {
                "(OK)"
            } else {
                "(?)"
            },
            self.pio.read_autoinc,
            if self.pio.read_validated {
                "(OK)"
            } else {
                "(?)"
            },
            self.cpuctl.startcpu,
        )?;
        if !self.anomalies.is_empty() {
            write!(f, " anomalies={}", self.anomalies.join("; "))?;
        }
        Ok(())
    }
}

// ── Runtime state (firmware boundary) ─────────────────────────────────────

/// Runtime state of a falcon processor — the firmware boundary probe.
///
/// This is the GPU equivalent of checking whether a motherboard's UEFI/BIOS
/// is running. The distinction between `Waiting` and `Halted` is critical:
/// under nouveau, PMU shows `Waiting` (firmware loaded, processing commands)
/// while after teardown it shows `Halted` (firmware dead, needs ACR reboot).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FalconState {
    /// Falcon is actively executing code (CPUCTL bits 4,5 both clear).
    Running,
    /// Falcon firmware is loaded and waiting for commands (CPUCTL bit 5 set,
    /// bit 4 clear). This is the PMU's normal operational state under nouveau.
    /// The falcon is alive and its services (PRI gates, etc.) are available.
    Waiting,
    /// Falcon has halted (CPUCTL bit 4 set). Firmware executed a HALT
    /// instruction or completed its boot sequence. Context-dependent:
    /// - Under nouveau: FECS normally halts between scheduling events (alive)
    /// - After teardown: falcon is dead, needs ACR boot to restart
    Halted,
    /// Both CPUCTL bits 4 and 5 set — hard reset or fully stopped state.
    Reset,
    /// Falcon registers are inaccessible (PRI-gated, returns 0xBADxxxxx).
    /// The engine hosting this falcon may be clock-gated or PMC-disabled.
    Inaccessible,
}

impl FalconState {
    /// Whether the falcon's firmware is loaded and functional (Running or Waiting).
    #[must_use]
    pub const fn is_alive(self) -> bool {
        matches!(self, Self::Running | Self::Waiting)
    }
}

impl fmt::Display for FalconState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Running => write!(f, "RUNNING"),
            Self::Waiting => write!(f, "WAITING"),
            Self::Halted => write!(f, "HALTED"),
            Self::Reset => write!(f, "RESET"),
            Self::Inaccessible => write!(f, "INACCESSIBLE"),
        }
    }
}

/// Firmware boundary probe result for a single falcon.
#[derive(Debug, Clone)]
pub struct FalconStatus {
    /// Falcon name (PMU, FECS, GPCCS, SEC2).
    pub name: String,
    /// BAR0 base address.
    pub base: usize,
    /// Runtime state.
    pub state: FalconState,
    /// Raw CPUCTL register value.
    pub cpuctl_raw: u32,
    /// Mailbox0 value (for communication probing).
    pub mailbox0: u32,
    /// Mailbox1 value.
    pub mailbox1: u32,
    /// Boot vector address.
    pub bootvec: u32,
}

impl fmt::Display for FalconStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:5} @ {:#08x}: {} (cpuctl={:#010x} mbox0={:#010x} bootvec={:#010x})",
            self.name, self.base, self.state, self.cpuctl_raw, self.mailbox0, self.bootvec
        )
    }
}

/// Complete firmware boundary probe — discovers all falcon states and
/// determines whether the GPU's firmware layer supports channel dispatch.
///
/// Follows the toadStool pattern: discover firmware state, report viability,
/// never try to replace the firmware.
#[derive(Debug, Clone)]
pub struct FalconProbe {
    /// Per-falcon status.
    pub falcons: Vec<FalconStatus>,
    /// PMC_ENABLE register value.
    pub pmc_enable: u32,
    /// PMC_UNK260 (PRI gate control) register value.
    pub pmc_unk260: u32,
    /// Whether PRI gates appear open (UNK260 is not 0xBADxxxxx).
    pub pri_gates_open: bool,
}

impl FalconProbe {
    /// Probe all standard GV100 falcons and PMC state.
    pub fn discover(bar0: &MappedBar) -> Self {
        let r = |reg: usize| bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD);

        let pmc_enable = r(0x200);
        let pmc_unk260 = r(0x260);
        let pri_gates_open = pmc_unk260 & 0xBAD0_0000 != 0xBAD0_0000;

        let targets: &[(&str, usize)] = &[
            ("PMU", falcon::PMU_BASE),
            ("SEC2", falcon::SEC2_BASE),
            ("FECS", falcon::FECS_BASE),
            ("GPCCS", falcon::GPCCS_BASE),
        ];

        let falcons = targets
            .iter()
            .map(|&(name, base)| {
                let cpuctl_raw = r(base + falcon::CPUCTL);
                let mailbox0 = r(base + falcon::MAILBOX0);
                let mailbox1 = r(base + falcon::MAILBOX1);
                let bootvec = r(base + falcon::BOOTVEC);

                let state = if cpuctl_raw & 0xBAD0_0000 == 0xBAD0_0000 {
                    FalconState::Inaccessible
                } else {
                    let bit4 = cpuctl_raw & (1 << 4) != 0;
                    let bit5 = cpuctl_raw & (1 << 5) != 0;
                    match (bit4, bit5) {
                        (false, false) => FalconState::Running,
                        (false, true) => FalconState::Waiting,
                        (true, false) => FalconState::Halted,
                        (true, true) => FalconState::Reset,
                    }
                };

                FalconStatus {
                    name: name.to_string(),
                    base,
                    state,
                    cpuctl_raw,
                    mailbox0,
                    mailbox1,
                    bootvec,
                }
            })
            .collect();

        Self {
            falcons,
            pmc_enable,
            pmc_unk260,
            pri_gates_open,
        }
    }

    /// Is the PMU falcon alive (Running or Waiting)? Without a functional PMU,
    /// PRI gates are closed and most GPU register writes are silently dropped.
    ///
    /// PMU `Waiting` (CPUCTL bit 5) is its normal operational state under
    /// nouveau — firmware is loaded and processing mailbox commands.
    #[must_use]
    pub fn pmu_alive(&self) -> bool {
        self.falcon_state("PMU").is_some_and(FalconState::is_alive)
    }

    /// Is the FECS falcon alive? Without FECS, GR engine context switches
    /// cannot complete and channels on GR runlists will hang.
    ///
    /// Note: FECS `Halted` is normal when no GR work is pending — it sleeps
    /// between scheduling events. What matters is whether PMU is alive to
    /// wake FECS when needed.
    #[must_use]
    pub fn fecs_alive(&self) -> bool {
        self.falcon_state("FECS").is_some_and(FalconState::is_alive)
    }

    /// Can we dispatch compute work through the PFIFO scheduler?
    ///
    /// The PMU must be alive (Running or Waiting) to keep PRI gates open.
    /// FECS being `Halted` is acceptable if PMU is alive — the scheduler
    /// can wake FECS for context switches.
    #[must_use]
    pub fn dispatch_viable(&self) -> bool {
        self.pmu_alive()
    }

    /// Get the state of a named falcon.
    #[must_use]
    pub fn falcon_state(&self, name: &str) -> Option<FalconState> {
        self.falcons.iter().find(|f| f.name == name).map(|f| f.state)
    }

    /// Summary of what blocks dispatch, if anything.
    #[must_use]
    pub fn dispatch_blockers(&self) -> Vec<String> {
        let mut blockers = Vec::new();
        if !self.pmu_alive() {
            let pmu_state = self.falcon_state("PMU").unwrap_or(FalconState::Inaccessible);
            blockers.push(format!(
                "PMU {pmu_state} — PRI gates closed, PFIFO register writes are dead. \
                 Needs ACR boot chain (SEC2 -> PMU firmware load) to restart."
            ));
        }
        if !self.pri_gates_open {
            blockers.push(format!(
                "PRI gates closed (UNK260={:#010x}) — most engine registers inaccessible",
                self.pmc_unk260
            ));
        }
        blockers
    }
}

impl fmt::Display for FalconProbe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Firmware Boundary Probe:")?;
        writeln!(
            f,
            "  PMC_ENABLE={:#010x}  UNK260={:#010x}  PRI_GATES={}",
            self.pmc_enable,
            self.pmc_unk260,
            if self.pri_gates_open { "OPEN" } else { "CLOSED" }
        )?;
        for falcon in &self.falcons {
            writeln!(f, "  {falcon}")?;
        }
        let viable = self.dispatch_viable();
        write!(f, "  dispatch_viable={viable}")?;
        if !viable {
            let blockers = self.dispatch_blockers();
            for b in &blockers {
                write!(f, "\n    BLOCKED: {b}")?;
            }
        }
        Ok(())
    }
}

// ── Probing ──────────────────────────────────────────────────────────────

const TEST_PATTERN: u32 = 0xCAFE_BABE;
const DEAD_READ: u32 = 0xDEAD_DEAD;

/// Probe a single falcon instance and discover its capabilities.
///
/// Safe to call on any falcon (FECS, GPCCS, SEC2, PMU) — the probe reads
/// HWCFG to determine version, then performs non-destructive PIO validation
/// if the falcon appears accessible (not in DEAD state).
pub fn probe_falcon(bar0: &MappedBar, name: &str, base: usize) -> DriverResult<FalconCapabilities> {
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(DEAD_READ);

    let hwcfg_raw = r(falcon::HWCFG);
    if hwcfg_raw == DEAD_READ {
        return Err(DriverError::DeviceNotFound(
            format!("{name} @ {base:#08x}: HWCFG reads as DEAD — falcon not accessible").into(),
        ));
    }

    let mut anomalies = Vec::new();

    let version = detect_version(hwcfg_raw, &mut anomalies);
    let cpuctl = detect_cpuctl_layout(&version);
    let security = decode_security_mode(r(falcon::SCTL), &mut anomalies);
    let sctl_raw = r(falcon::SCTL);
    let imem_size = falcon::imem_size_bytes(hwcfg_raw);
    let dmem_size = falcon::dmem_size_bytes(hwcfg_raw);
    let requires_signed_fw = hwcfg_raw & falcon::HWCFG_SECURITY_MODE != 0;

    if imem_size == 0 {
        anomalies.push(format!("IMEM size=0 from HWCFG {hwcfg_raw:#010x}"));
    }
    if dmem_size == 0 {
        anomalies.push(format!("DMEM size=0 from HWCFG {hwcfg_raw:#010x}"));
    }

    let pio = probe_pio_layout(bar0, base, dmem_size, &mut anomalies);

    Ok(FalconCapabilities {
        name: name.to_string(),
        base,
        version,
        cpuctl,
        pio,
        security,
        sctl_raw,
        imem_size,
        dmem_size,
        requires_signed_fw,
        anomalies,
    })
}

/// Probe all standard GV100 falcon instances.
pub fn probe_all_falcons(bar0: &MappedBar) -> Vec<FalconCapabilities> {
    let targets = [
        ("FECS", falcon::FECS_BASE),
        ("GPCCS", falcon::GPCCS_BASE),
        ("SEC2", falcon::SEC2_BASE),
        ("PMU", falcon::PMU_BASE),
    ];

    targets
        .iter()
        .filter_map(|&(name, base)| match probe_falcon(bar0, name, base) {
            Ok(caps) => Some(caps),
            Err(e) => {
                tracing::warn!(name, base = format!("{base:#08x}"), error = %e, "falcon probe failed");
                None
            }
        })
        .collect()
}

// ── Version detection ────────────────────────────────────────────────────

fn detect_version(hwcfg: u32, anomalies: &mut Vec<String>) -> FalconVersion {
    // Falcon version is encoded in HWCFG bits [27:24] on GM200+ (envytools: CORE_REV).
    // On older falcons, these bits may be zero or encode differently.
    let core_rev = (hwcfg >> 24) & 0xF;

    // Heuristic: falcon v0 has core_rev=0, v1-v5 have nonzero core_rev.
    // GV100 falcons are v4/v5 (core_rev >= 4).
    let major = match core_rev {
        0 => {
            // Could be v0 (pre-GM200) or just unset. Use IMEM size heuristic:
            // v0 falcons typically have smaller IMEM (<=16KB).
            let imem = falcon::imem_size_bytes(hwcfg);
            if imem > 0 && imem <= 16384 {
                0
            } else {
                anomalies.push(format!(
                    "HWCFG core_rev=0 with large IMEM ({imem}B) — defaulting to v4"
                ));
                4
            }
        }
        1..=3 => {
            // GM200 era: falcon v1-v3
            core_rev as u8
        }
        4..=5 => core_rev as u8,
        _ => {
            anomalies.push(format!("unexpected HWCFG core_rev={core_rev}"));
            core_rev as u8
        }
    };

    FalconVersion {
        major,
        hwcfg_raw: hwcfg,
    }
}

fn detect_cpuctl_layout(version: &FalconVersion) -> CpuCtlLayout {
    if version.major >= 4 {
        // Falcon v4+ (GM200+): IINVAL=BIT(0), STARTCPU=BIT(1)
        CpuCtlLayout {
            startcpu: 1 << 1,
            iinval: 1 << 0,
            hreset: 1 << 4,
            halted: 1 << 5,
        }
    } else {
        // Falcon v0-v3: STARTCPU=BIT(0), no separate IINVAL
        CpuCtlLayout {
            startcpu: 1 << 0,
            iinval: 0,
            hreset: 1 << 4,
            halted: 1 << 5,
        }
    }
}

// ── Security mode ────────────────────────────────────────────────────────

fn decode_security_mode(sctl: u32, anomalies: &mut Vec<String>) -> SecurityMode {
    // envytools SEC_MODE at offset 0x240: bits[13:12] encode security level.
    // 0=NS, 1=LS, 2=HS. Value 3 is undocumented.
    //
    // NVIDIA internal "SCTL" terminology uses a wider field.
    // We decode both the 2-bit SEC_MODE and flag any extra bits.
    let sec_level = (sctl >> 12) & 0x3;
    let mode = match sec_level {
        0 => SecurityMode::NonSecure,
        1 => SecurityMode::LightSecure,
        2 => SecurityMode::HeavySecure,
        _ => {
            anomalies.push(format!(
                "SCTL={sctl:#010x}: sec_level={sec_level} is undocumented (bits[13:12]=0b11)"
            ));
            SecurityMode::Unknown(sec_level)
        }
    };

    // Flag additional set bits outside the known SEC_MODE field
    let known_mask = 0x3000; // bits [13:12]
    let extra = sctl & !known_mask;
    if extra != 0 {
        anomalies.push(format!(
            "SCTL={sctl:#010x}: extra bits outside SEC_MODE: {extra:#010x}"
        ));
    }

    mode
}

// ── PIO layout probing ──────────────────────────────────────────────────

/// Candidate bit positions to try for PIO auto-increment.
const WRITE_CANDIDATES: &[(u32, &str)] = &[
    (1 << 24, "BIT(24) — GM200+/nouveau"),
    (1 << 6, "BIT(6) — legacy/envytools v0"),
    (1 << 8, "BIT(8) — speculative"),
];

const READ_CANDIDATES: &[(u32, &str)] = &[
    (1 << 25, "BIT(25) — GM200+/nouveau"),
    (1 << 7, "BIT(7) — legacy/envytools v0"),
];

fn probe_pio_layout(
    bar0: &MappedBar,
    base: usize,
    dmem_size: u32,
    anomalies: &mut Vec<String>,
) -> PioLayout {
    // Default: the GM200+ layout (BIT(24)/BIT(25)/BIT(28)) which is correct
    // for all falcon instances we've validated on GV100.
    let mut layout = PioLayout {
        write_autoinc: 1 << 24,
        read_autoinc: 1 << 25,
        secure_flag: 1 << 28,
        write_validated: false,
        read_validated: false,
    };

    // Only attempt PIO validation if DMEM exists and is large enough for a test word.
    if dmem_size < 8 {
        anomalies.push("DMEM too small for PIO validation".to_string());
        return layout;
    }

    // Use DMEM for probing because it's simpler (no tags, no secure page complexity).
    // Write a test pattern using each candidate bit, then read it back.
    let test_addr: u32 = 0;

    // Save original DMEM[0] before we clobber it.
    let original = read_dmem_word(bar0, base, test_addr, layout.read_autoinc);

    // Try write candidates
    for &(write_bit, label) in WRITE_CANDIDATES {
        write_dmem_word(bar0, base, test_addr, write_bit, TEST_PATTERN);

        // Read back with each read candidate to find the working combo
        for &(read_bit, _) in READ_CANDIDATES {
            let readback = read_dmem_word(bar0, base, test_addr, read_bit);
            if readback == TEST_PATTERN {
                layout.write_autoinc = write_bit;
                layout.read_autoinc = read_bit;
                layout.write_validated = true;
                layout.read_validated = true;

                if write_bit != (1 << 24) {
                    anomalies.push(format!(
                        "PIO write uses {label} ({write_bit:#010x}), not BIT(24)"
                    ));
                }

                // Restore original value
                write_dmem_word(bar0, base, test_addr, write_bit, original);
                return probe_secure_flag(bar0, base, &mut layout, anomalies);
            }
        }
    }

    // None of the candidates produced a valid readback.
    // This can happen if the falcon is in a state that blocks DMEM PIO
    // (e.g., running firmware occupying DMEM, or truly inaccessible).
    anomalies.push(
        "PIO validation failed: no write+read candidate produced correct readback. \
                    Falcon may be running or DMEM inaccessible. Using default GM200+ layout."
            .to_string(),
    );

    layout
}

fn probe_secure_flag(
    bar0: &MappedBar,
    base: usize,
    layout: &mut PioLayout,
    anomalies: &mut Vec<String>,
) -> PioLayout {
    // The secure flag (expected BIT(28)) marks IMEM pages as secure.
    // When read back, secure pages return 0xdead5ec1 instead of actual content.
    // We can detect this by writing to IMEM with the flag and checking the sentinel.
    //
    // This test is only safe if the falcon is halted (not executing from IMEM).
    // For now, we trust the default BIT(28) since it matches nouveau source
    // for all falcon versions we've seen.
    //
    // A full validation would: write IMEM[high_addr] with BIT(28), read back,
    // check for 0xdead5ec1. But IMEM writes clobber firmware, so we skip this
    // on running falcons.
    let cpuctl = bar0.read_u32(base + falcon::CPUCTL).unwrap_or(DEAD_READ);
    let halted_or_hreset = cpuctl & ((1 << 4) | (1 << 5)) != 0;

    if !halted_or_hreset && cpuctl != DEAD_READ {
        anomalies.push(
            "skipped IMEM secure-flag probe: falcon appears running (CPUCTL indicates active)"
                .to_string(),
        );
    }

    *layout
}

// ── Low-level PIO helpers ────────────────────────────────────────────────

fn write_dmem_word(bar0: &MappedBar, base: usize, addr: u32, write_bit: u32, value: u32) {
    let _ = bar0.write_u32(base + falcon::DMEMC, write_bit | addr);
    let _ = bar0.write_u32(base + falcon::DMEMD, value);
}

fn read_dmem_word(bar0: &MappedBar, base: usize, addr: u32, read_bit: u32) -> u32 {
    let _ = bar0.write_u32(base + falcon::DMEMC, read_bit | addr);
    bar0.read_u32(base + falcon::DMEMD).unwrap_or(DEAD_READ)
}

// ── Safe PIO interface ──────────────────────────────────────────────────

/// Safe PIO interface for a specific falcon, backed by discovered capabilities.
///
/// All control words are constructed from validated bit layouts. Provides
/// upload, readback, and verification methods that cannot use the wrong format.
pub struct FalconPio<'a> {
    bar0: &'a MappedBar,
    caps: &'a FalconCapabilities,
}

impl<'a> FalconPio<'a> {
    /// Create a new PIO interface for a probed falcon.
    #[must_use]
    pub const fn new(bar0: &'a MappedBar, caps: &'a FalconCapabilities) -> Self {
        Self { bar0, caps }
    }

    /// Upload data to IMEM at `addr`, with optional secure page marking.
    pub fn upload_imem(&self, addr: u32, data: &[u8], secure: bool) {
        let ctrl = if secure {
            self.caps.imem_write_secure_ctrl(addr)
        } else {
            self.caps.imem_write_ctrl(addr)
        };
        let _ = self
            .bar0
            .write_u32(self.caps.base + falcon::IMEMC, ctrl.raw());

        for (i, chunk) in data.chunks(4).enumerate() {
            let byte_offset = (i * 4) as u32;
            if byte_offset & 0xFF == 0 {
                let _ = self
                    .bar0
                    .write_u32(self.caps.base + falcon::IMEMT, (addr + byte_offset) >> 8);
            }
            let _ = self
                .bar0
                .write_u32(self.caps.base + falcon::IMEMD, le_word(chunk));
        }

        // Pad to 256-byte boundary
        let total_bytes = (data.len().div_ceil(4)) * 4;
        let remainder = total_bytes & 0xFF;
        if remainder != 0 {
            let padding_words = (256 - remainder) / 4;
            for _ in 0..padding_words {
                let _ = self.bar0.write_u32(self.caps.base + falcon::IMEMD, 0);
            }
        }
    }

    /// Upload data to DMEM at `addr`.
    pub fn upload_dmem(&self, addr: u32, data: &[u8]) {
        let ctrl = self.caps.dmem_write_ctrl(addr);
        let _ = self
            .bar0
            .write_u32(self.caps.base + falcon::DMEMC, ctrl.raw());

        for chunk in data.chunks(4) {
            let _ = self
                .bar0
                .write_u32(self.caps.base + falcon::DMEMD, le_word(chunk));
        }
    }

    /// Read `count` 32-bit words from IMEM starting at `addr`.
    pub fn read_imem(&self, addr: u32, count: usize) -> Vec<u32> {
        let ctrl = self.caps.imem_read_ctrl(addr);
        let _ = self
            .bar0
            .write_u32(self.caps.base + falcon::IMEMC, ctrl.raw());

        (0..count)
            .map(|_| {
                self.bar0
                    .read_u32(self.caps.base + falcon::IMEMD)
                    .unwrap_or(DEAD_READ)
            })
            .collect()
    }

    /// Read `count` 32-bit words from DMEM starting at `addr`.
    pub fn read_dmem(&self, addr: u32, count: usize) -> Vec<u32> {
        let ctrl = self.caps.dmem_read_ctrl(addr);
        let _ = self
            .bar0
            .write_u32(self.caps.base + falcon::DMEMC, ctrl.raw());

        (0..count)
            .map(|_| {
                self.bar0
                    .read_u32(self.caps.base + falcon::DMEMD)
                    .unwrap_or(DEAD_READ)
            })
            .collect()
    }

    /// Upload IMEM data and verify by readback. Returns number of mismatched words.
    pub fn upload_imem_verified(&self, addr: u32, data: &[u8], secure: bool) -> usize {
        self.upload_imem(addr, data, secure);

        if secure {
            // Secure pages return sentinel on readback — skip verification
            return 0;
        }

        let word_count = data.len().div_ceil(4);
        let readback = self.read_imem(addr, word_count);

        let mut mismatches = 0;
        for (i, chunk) in data.chunks(4).enumerate() {
            let expected = le_word(chunk);
            if i < readback.len() && readback[i] != expected {
                if mismatches < 4 {
                    tracing::warn!(
                        falcon = %self.caps.name,
                        offset = i * 4 + addr as usize,
                        expected = format!("{expected:#010x}"),
                        got = format!("{:#010x}", readback[i]),
                        "IMEM verify mismatch"
                    );
                }
                mismatches += 1;
            }
        }
        mismatches
    }

    /// Upload DMEM data and verify by readback. Returns number of mismatched words.
    pub fn upload_dmem_verified(&self, addr: u32, data: &[u8]) -> usize {
        self.upload_dmem(addr, data);

        let word_count = data.len().div_ceil(4);
        let readback = self.read_dmem(addr, word_count);

        let mut mismatches = 0;
        for (i, chunk) in data.chunks(4).enumerate() {
            let expected = le_word(chunk);
            if i < readback.len() && readback[i] != expected {
                if mismatches < 4 {
                    tracing::warn!(
                        falcon = %self.caps.name,
                        offset = i * 4 + addr as usize,
                        expected = format!("{expected:#010x}"),
                        got = format!("{:#010x}", readback[i]),
                        "DMEM verify mismatch"
                    );
                }
                mismatches += 1;
            }
        }
        mismatches
    }
}

fn le_word(chunk: &[u8]) -> u32 {
    match chunk.len() {
        4 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
        3 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]),
        2 => u32::from_le_bytes([chunk[0], chunk[1], 0, 0]),
        1 => u32::from_le_bytes([chunk[0], 0, 0, 0]),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_mode_decode_ns() {
        let mut a = Vec::new();
        assert_eq!(
            decode_security_mode(0x0000, &mut a),
            SecurityMode::NonSecure
        );
        assert!(a.is_empty());
    }

    #[test]
    fn security_mode_decode_ls() {
        let mut a = Vec::new();
        assert_eq!(
            decode_security_mode(0x1000, &mut a),
            SecurityMode::LightSecure
        );
        assert!(a.is_empty());
    }

    #[test]
    fn security_mode_decode_hs() {
        let mut a = Vec::new();
        assert_eq!(
            decode_security_mode(0x2000, &mut a),
            SecurityMode::HeavySecure
        );
        assert!(a.is_empty());
    }

    #[test]
    fn security_mode_decode_unknown_3() {
        let mut a = Vec::new();
        let mode = decode_security_mode(0x3000, &mut a);
        assert!(matches!(mode, SecurityMode::Unknown(3)));
        assert_eq!(a.len(), 1, "should flag undocumented sec_level=3");
    }

    #[test]
    fn security_mode_extra_bits_flagged() {
        let mut a = Vec::new();
        decode_security_mode(0x3001, &mut a);
        assert!(
            a.len() >= 2,
            "should flag both unknown level and extra bit 0"
        );
    }

    #[test]
    fn cpuctl_layout_v4_plus() {
        let v = FalconVersion {
            major: 4,
            hwcfg_raw: 0,
        };
        let layout = detect_cpuctl_layout(&v);
        assert_eq!(layout.startcpu, 1 << 1);
        assert_eq!(layout.iinval, 1 << 0);
        assert_eq!(layout.hreset, 1 << 4);
        assert_eq!(layout.halted, 1 << 5);
    }

    #[test]
    fn cpuctl_layout_v0() {
        let v = FalconVersion {
            major: 0,
            hwcfg_raw: 0,
        };
        let layout = detect_cpuctl_layout(&v);
        assert_eq!(layout.startcpu, 1 << 0);
        assert_eq!(layout.iinval, 0);
    }

    #[test]
    fn pio_ctrl_raw_roundtrip() {
        let caps = FalconCapabilities {
            name: "test".into(),
            base: 0,
            version: FalconVersion {
                major: 4,
                hwcfg_raw: 0,
            },
            cpuctl: detect_cpuctl_layout(&FalconVersion {
                major: 4,
                hwcfg_raw: 0,
            }),
            pio: PioLayout {
                write_autoinc: 1 << 24,
                read_autoinc: 1 << 25,
                secure_flag: 1 << 28,
                write_validated: true,
                read_validated: true,
            },
            security: SecurityMode::NonSecure,
            sctl_raw: 0,
            imem_size: 65536,
            dmem_size: 5120,
            requires_signed_fw: false,
            anomalies: vec![],
        };

        assert_eq!(caps.imem_write_ctrl(0x3400).raw(), 0x0100_3400);
        assert_eq!(caps.imem_write_secure_ctrl(0).raw(), 0x1100_0000);
        assert_eq!(caps.imem_read_ctrl(0x3400).raw(), 0x0200_3400);
        assert_eq!(caps.dmem_write_ctrl(0).raw(), 0x0100_0000);
        assert_eq!(caps.dmem_read_ctrl(0x100).raw(), 0x0200_0100);
    }

    #[test]
    fn version_detect_high_core_rev() {
        let mut a = Vec::new();
        let v = detect_version(0x0400_0000 | 0x140, &mut a); // core_rev=4, some IMEM
        assert_eq!(v.major, 4);
        assert!(a.is_empty());
    }

    #[test]
    fn version_detect_zero_core_rev_small_imem() {
        let mut a = Vec::new();
        let hwcfg = 0x0000_0040; // 64 * 256 = 16384B IMEM, core_rev=0
        let v = detect_version(hwcfg, &mut a);
        assert_eq!(v.major, 0);
    }

    #[test]
    fn display_capabilities() {
        let caps = FalconCapabilities {
            name: "FECS".into(),
            base: 0x409000,
            version: FalconVersion {
                major: 4,
                hwcfg_raw: 0x04000140,
            },
            cpuctl: detect_cpuctl_layout(&FalconVersion {
                major: 4,
                hwcfg_raw: 0,
            }),
            pio: PioLayout {
                write_autoinc: 1 << 24,
                read_autoinc: 1 << 25,
                secure_flag: 1 << 28,
                write_validated: true,
                read_validated: true,
            },
            security: SecurityMode::LightSecure,
            sctl_raw: 0x1000,
            imem_size: 65536,
            dmem_size: 5120,
            requires_signed_fw: false,
            anomalies: vec![],
        };
        let s = caps.to_string();
        assert!(s.contains("FECS"));
        assert!(s.contains("LS"));
        assert!(s.contains("(OK)"));
    }

    #[test]
    fn le_word_full() {
        assert_eq!(le_word(&[0xd0, 0x00, 0x14, 0x00]), 0x001400d0);
    }

    #[test]
    fn le_word_partial() {
        assert_eq!(le_word(&[0x01, 0x02, 0x03]), 0x00030201);
        assert_eq!(le_word(&[0xFF, 0x00]), 0x000000FF);
        assert_eq!(le_word(&[0x42]), 0x00000042);
        assert_eq!(le_word(&[]), 0);
    }
}
