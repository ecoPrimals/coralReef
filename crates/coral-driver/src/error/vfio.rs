// SPDX-License-Identifier: AGPL-3.0-or-later
//! VFIO-path error types: PCI discovery, VBIOS/devinit, channel oracle, sovereign stages.

use super::DriverError;

/// Errors from PCI sysfs/config-space discovery and power management (VFIO path).
#[derive(Debug, thiserror::Error)]
pub enum PciDiscoveryError {
    /// The BDF string does not match `domain:bus:dev.fn` hex segments.
    #[error("invalid PCI BDF: {bdf}")]
    InvalidBdf {
        /// Raw BDF input.
        bdf: String,
    },

    /// Config space snapshot is shorter than required for standard PCI headers / caps.
    #[error("PCI config too short: {len} bytes (need at least {need})")]
    ConfigTooShort {
        /// Bytes available.
        len: usize,
        /// Minimum bytes required for the operation.
        need: usize,
    },

    /// Status register reports no capability list.
    #[error("PCI config has no capabilities list")]
    NoPciCapabilitiesList,

    /// Capability chain walk did not find a power-management capability.
    #[error("PM capability not found in PCI config space")]
    PmCapabilityNotFound,

    /// PMCSR lies outside the config buffer (truncated sysfs read or corrupt image).
    #[error("PMCSR offset {pmcsr_off:#x} is beyond PCI config space ({config_len} bytes)")]
    PmcsrBeyondConfig {
        /// Byte offset of PMCSR in config space.
        pmcsr_off: usize,
        /// Length of the config buffer.
        config_len: usize,
    },

    /// Sysfs file read/write for PCI discovery failed.
    #[error("{operation} {path}: {source}")]
    SysfsIo {
        /// Short verb for logs (`read`, `open for write`, etc.).
        operation: &'static str,
        /// Full sysfs path.
        path: String,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },

    /// Power cycle refused: kernel still has a driver bound.
    #[error("device has a driver bound — unbind before power cycle")]
    DriverBoundForPowerCycle,

    /// Device path missing after `remove` + bus rescan.
    #[error("PCI device not found after bus rescan")]
    DeviceMissingAfterRescan,
}

#[cfg(feature = "vfio")]
impl PciDiscoveryError {
    /// Wrap an [`std::io::Error`] with the sysfs path and operation label.
    pub(crate) fn sysfs_io(
        operation: &'static str,
        path: impl Into<String>,
        source: std::io::Error,
    ) -> Self {
        Self::SysfsIo {
            operation,
            path: path.into(),
            source,
        }
    }
}

/// Errors from VBIOS parsing, PROM/sysfs ROM reads, and host-side devinit (interpreter / PMU).
#[derive(Debug, thiserror::Error)]
pub enum DevinitError {
    /// BIT table signature `\xFF\xB8BIT` not found in ROM.
    #[error("BIT signature (\\xFF\\xB8BIT) not found in VBIOS")]
    BitSignatureNotFound,

    /// BIT header ends before required fields.
    #[error("BIT header truncated")]
    BitHeaderTruncated,

    /// BIT header lists impossible entry size or count.
    #[error("BIT header invalid: entry_size={entry_size} count={entry_count}")]
    BitHeaderInvalid {
        /// Declared entry size in bytes.
        entry_size: usize,
        /// Declared entry count.
        entry_count: usize,
    },

    /// BIT `'p'` (PMU) sub-table not present.
    #[error("BIT 'p' (PMU) entry not found")]
    PmuBitEntryNotFound,

    /// Pointer to PMU firmware table lies outside ROM.
    #[error("PMU table pointer out of bounds")]
    PmuTablePointerOutOfBounds,

    /// Resolved PMU table start lies outside ROM.
    #[error("PMU table at {offset:#x} out of bounds")]
    PmuTableOutOfBounds {
        /// Byte offset of the PMU table header.
        offset: usize,
    },

    /// PMU table header does not match expected layout.
    #[error(
        "unexpected PMU table format: ver={version} hdr={header_size} entries={entry_count} entry_size={entry_size}"
    )]
    PmuTableUnexpectedFormat {
        /// Table version byte.
        version: u8,
        /// Header size in bytes.
        header_size: usize,
        /// Number of entries.
        entry_count: usize,
        /// Bytes per entry.
        entry_size: usize,
    },

    /// PROM window does not start with a valid option-ROM signature.
    #[error("PROM signature mismatch: got {got:#010x} (expected 0x????AA55)")]
    PromSignatureMismatch {
        /// First 32-bit word read from PROM base.
        got: u32,
    },

    /// PROM read produced fewer bytes than the minimum valid ROM.
    #[error("PROM too small: {len} bytes")]
    PromTooSmall {
        /// Bytes read.
        len: usize,
    },

    /// ROM buffer shorter than a minimal PCI option ROM.
    #[error("ROM too small: {len} bytes")]
    RomTooSmall {
        /// Bytes available.
        len: usize,
    },

    /// PCI ROM signature bytes at offset 0–1 are not `0x55 0xAA`.
    #[error("bad ROM signature: {byte0:#04x} {byte1:#04x} (expected 0x55 0xAA)")]
    RomBadSignature {
        /// First byte.
        byte0: u8,
        /// Second byte.
        byte1: u8,
    },

    /// Sysfs or file access for VBIOS (`rom` enable, read, file path).
    #[error("{operation} {path}: {source}")]
    VbiosResourceIo {
        /// Short verb (`read`, `write`, etc.).
        operation: &'static str,
        /// Path accessed.
        path: String,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },

    /// No PROM/sysfs/file source yielded a valid ROM (`FalconDiagnostic::best_vbios`).
    #[error("no VBIOS source available")]
    NoVbiosSource,

    /// Sysfs ROM fallback requested but no PCI BDF was provided.
    #[error("no BDF for sysfs VBIOS fallback")]
    NoBdfForSysfsVbios,

    /// BIT `'I'` (init tables) entry missing.
    #[error("BIT 'I' not found")]
    BitINotFound,

    /// BIT `'I'` data too short for the requested field.
    #[error("BIT 'I' data too short")]
    BitIDataTooShort,

    /// Init tables base pointer from BIT `'I'` is null or out of range.
    #[error("init tables base pointer is null or invalid")]
    InterpreterInitTablesInvalid,

    /// Script table pointer derived from init tables is null or out of range.
    #[error("init script table pointer is null or invalid")]
    InterpreterScriptTableInvalid,

    /// Too many unrecognized opcodes while interpreting VBIOS init scripts.
    #[error("too many unknown VBIOS opcodes (>100), last at {last_offset:#x}: {last_opcode:#04x}")]
    InterpreterTooManyUnknownOpcodes {
        /// ROM offset of the last unknown opcode.
        last_offset: usize,
        /// Opcode byte.
        last_opcode: u8,
    },

    /// BIT `'I'` exists but layout is not suitable for PMU devinit (version / size).
    #[error(
        "BIT 'I' entry: unexpected version {version} or size {data_size} (need ver=1, size>=0x1c)"
    )]
    BitIUnexpectedLayout {
        /// Version field from BIT.
        version: u8,
        /// Data size field from BIT.
        data_size: u16,
    },

    /// No PMU app with type `0x04` (DEVINIT) in the firmware table.
    #[error("PMU DEVINIT firmware (type 0x04) not found in VBIOS")]
    PmuDevinitFirmwareNotFound,

    /// DEVINIT image regions extend past the end of the ROM buffer.
    #[error("DEVINIT firmware sections extend beyond ROM")]
    DevinitFirmwareBeyondRom,

    /// PMU did not report completion within the timeout.
    #[error("PMU DEVINIT timed out after 2s (MBOX0={mbox0:#010x})")]
    PmuDevinitTimeout {
        /// Last `FALCON_MBOX0` read.
        mbox0: u32,
    },

    /// BIT `'I'` references no boot script region (scan path).
    #[error("no boot scripts in BIT 'I'")]
    NoBootScriptsInBitI,
}

#[cfg(feature = "vfio")]
impl DevinitError {
    /// Wrap an [`std::io::Error`] with path and operation (VBIOS sysfs / file access).
    pub(crate) fn vbios_resource_io(
        operation: &'static str,
        path: impl Into<String>,
        source: std::io::Error,
    ) -> Self {
        Self::VbiosResourceIo {
            operation,
            path: path.into(),
            source,
        }
    }
}

/// Errors from VFIO channel oracle paths: BAR0 sysfs access, oracle dumps, and nouveau MMU walks.
#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    /// Sysfs or file I/O for oracle resources (`read`, `open`, etc.).
    #[error("{operation} {path}: {source}")]
    ResourceIo {
        /// Short verb for logs (`read`, `open`, etc.).
        operation: &'static str,
        /// Path that was accessed.
        path: String,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },

    /// `mmap` of sysfs BAR0 (`resource0`) failed.
    #[error("mmap BAR0 {path}: {source}")]
    Bar0Mmap {
        /// Full sysfs path to `resource0`.
        path: String,
        /// `mmap` errno from the kernel.
        #[source]
        source: rustix::io::Errno,
    },

    /// `mmap` returned a null pointer (unexpected).
    #[error("mmap returned null for BAR0 {path}")]
    Bar0MmapNull {
        /// Full sysfs path.
        path: String,
    },

    /// BAR0 binary dump is smaller than the minimum region scanned by the oracle loader.
    #[error("BAR0 dump too small: {len} bytes (need at least {need})")]
    Bar0DumpTooShort {
        /// Bytes in the file.
        len: usize,
        /// Minimum size required.
        need: usize,
    },

    /// Hex token in an oracle text line is not a valid register offset.
    #[error("invalid hex offset in oracle text dump: {token}")]
    InvalidHexOffset {
        /// Raw token from the line.
        token: String,
    },

    /// Hex token in an oracle text line is not a valid 32-bit value.
    #[error("invalid hex value in oracle text dump: {token}")]
    InvalidHexValue {
        /// Raw token from the line.
        token: String,
    },

    /// BAR0 reads as all ones — device may be in D3hot, unbound, or otherwise inaccessible.
    #[error("BAR0 reads 0xFFFFFFFF — device may be in D3hot or not accessible")]
    Bar0ReadsAllOnes,

    /// PCCSR scan did not find a channel with a usable instance pointer.
    #[error("no active channel found in PCCSR (channels 0-511)")]
    NoActivePccsrChannel,

    /// Oracle and target BOOT0 differ (e.g. different GPU or reset state).
    #[error("BOOT0 mismatch: oracle={oracle:#010x} target={target:#010x}")]
    Boot0Mismatch {
        /// BOOT0 read from the oracle card.
        oracle: u32,
        /// BOOT0 read from the target VFIO mapping.
        target: u32,
    },

    /// BAR0 read offset plus width extends past the mapped region (MMU oracle capture).
    #[error("BAR0 read out of bounds: offset=0x{offset:x}, size=0x{map_size:x}")]
    Bar0ReadOutOfBounds {
        /// Byte offset of the access.
        offset: usize,
        /// Mapped BAR0 size in bytes.
        map_size: usize,
    },

    /// BAR0 write offset plus width extends past the mapped region.
    #[error("BAR0 write out of bounds: offset=0x{offset:x}, size=0x{map_size:x}")]
    Bar0WriteOutOfBounds {
        /// Byte offset of the access.
        offset: usize,
        /// Mapped BAR0 size in bytes.
        map_size: usize,
    },

    /// External BAR0 pointer (e.g. VFIO `MappedBar`) was null.
    #[error("BAR0 mapping pointer is null")]
    Bar0ExternalNull,

    /// Device is exclusively held by a live coral-ember instance.
    ///
    /// Direct sysfs BAR0 access is blocked; use ember's VFIO fd sharing
    /// or MMIO RPC instead.
    #[error("device {bdf} is held by ember — route BAR0 access through ember")]
    DeviceHeldByEmber {
        /// PCI BDF address of the held device.
        bdf: String,
    },
}

#[cfg(feature = "vfio")]
impl ChannelError {
    /// Wrap an [`std::io::Error`] with path and operation (oracle file / sysfs access).
    pub(crate) fn resource_io(
        operation: &'static str,
        path: impl Into<String>,
        source: std::io::Error,
    ) -> Self {
        Self::ResourceIo {
            operation,
            path: path.into(),
            source,
        }
    }
}

/// Errors from sovereign init stage helpers (`sovereign_stages`): BAR0 probe, PMC,
/// HBM2/GDDR training, falcon boot, GR init, verification.
///
/// Does not nest [`DriverError`] via `#[from]` to avoid a conversion cycle with
/// [`DriverError::SovereignStages`]. Use [`SovereignStagesError::vfio_compute`] for
/// `DriverError` values from FECS/GPCCS/boot paths.
#[derive(Debug, thiserror::Error)]
pub enum SovereignStagesError {
    /// BAR0 isolated read exceeded the hang timeout (child killed).
    #[error("BAR0 probe timed out — GPU unreachable")]
    Bar0ProbeTimeout,
    /// Isolated BAR0 child exited uncleanly.
    #[error("BAR0 probe child failed (status={status})")]
    Bar0ProbeChildFailed {
        /// Exit code or negative signal number from the isolate worker.
        status: i32,
    },
    /// Fork or pipe setup failed before BAR0 isolation.
    #[error("BAR0 probe fork error: {0}")]
    Bar0ProbeFork(#[source] std::io::Error),
    /// BOOT0 read as zero or `0xffff_ffff` — device missing or inaccessible.
    #[error("BAR0 returned {boot0:#010x} — device not responding")]
    Bar0ProbeNonResponsive {
        /// Raw BOOT0 sample.
        boot0: u32,
    },

    /// Isolated PMC_ENABLE write timed out.
    #[error("PMC_ENABLE write timed out — GPU hung (child killed)")]
    PmcEnableWriteTimeout,
    /// Isolated PMC_ENABLE write failed (`IsolationResult::ChildFailed`, `ForkError`, etc.).
    #[error("PMC_ENABLE isolated MMIO failure: {message}")]
    PmcEnableIsolationFailure {
        /// Debug snapshot of isolate outcome (`{other:?}` from `IsolationResult`).
        message: String,
    },
    /// PMC_ENABLE read back as zero placeholder after programmed write.
    #[error("PMC_ENABLE stuck at 0x{after:08x} after write")]
    PmcEnableStuck {
        /// Post-write register sample.
        after: u32,
    },

    /// HBM2 typestate pipeline failed.
    #[cfg(all(target_os = "linux", feature = "vfio"))]
    #[error(transparent)]
    Hbm2Training(#[from] crate::vfio::channel::hbm2_training::Hbm2TrainingError),
    /// VBIOS / PMU devinit for GDDR5 cold training failed.
    #[error(transparent)]
    Devinit(#[from] DevinitError),
    /// PMU path reported success but PRAMIN sentinel still failed.
    #[error("DEVINIT completed but PRAMIN still returns bad data")]
    Gddr5PraminDeadAfterDevinit,
    /// Registers said devinit not needed yet VRAM probe is dead.
    #[error("DEVINIT not needed per register but PRAMIN is dead")]
    Gddr5PraminDeadDevinitSkipped,

    /// Kepler firmware blob read from `/lib/firmware/nvidia/...`.
    #[error("Kepler firmware read {path}: {source}")]
    KeplerFirmwareRead {
        /// Firmware file path.
        path: String,
        /// Underlying filesystem error.
        #[source]
        source: std::io::Error,
    },
    /// PIO falcon mailbox did not come up before timeout.
    #[error("{name}: Falcon boot timeout (cpuctl={cpuctl:#010x})")]
    KeplerFalconBootTimeout {
        /// Logical falcon (`"FECS"`, `"GPCCS"`).
        name: &'static str,
        /// Last observed `CPUCTL`.
        cpuctl: u32,
    },
    /// Kepler direct boot path did not report a running FECS.
    #[error("Kepler falcon not running: {detail}")]
    KeplerFalconNotRunning {
        /// Diagnostics string (mailbox / cpuctl state).
        detail: String,
    },

    /// PGOB power-domain step timed out (bit 31 stuck high after 2 s).
    #[error("PGOB power step {step_index} at {addr:#010x} timed out (pre={pre:#010x}, post={post:#010x})")]
    PgobStepTimeout {
        /// Zero-based index into the power domain step table.
        step_index: usize,
        /// Register address that did not clear bit 31.
        addr: u32,
        /// Register value before the write.
        pre: u32,
        /// Register value after polling expired.
        post: u32,
    },

    /// Cold falcon boot did not yield a runnable FECS (detail line for logs).
    #[error("falcon boot did not succeed: {detail}")]
    FalconBootNotRunning {
        /// Concatenated boot-path diagnostics.
        detail: String,
    },
    /// Direct boot failed after solver attempt; aggregates last driver error + ACR notes.
    #[error("all falcon boot paths failed: {detail}")]
    FalconBootPathsExhausted {
        /// Printable summary including nested driver error display.
        detail: String,
    },

    /// GR FECS reachable but halted / not mailbox-ready (`boot_fecs` success path).
    #[error("GR FECS not running: cpuctl=0x{cpuctl:08x}")]
    GrFecsNotRunning {
        /// Post-attempt `CPUCTL` sample.
        cpuctl: u32,
    },

    /// Wraps [`DriverError`] from VFIO compute boot (`boot_gr_falcons`, `boot_fecs`, ACR).
    #[error("VFIO compute: {0}")]
    VfioCompute(#[source] Box<DriverError>),

    /// Isolated verify batch timed out.
    #[error("verify timed out — GPU D-state")]
    VerifyTimeout,
    /// Isolated verify child failed.
    #[error("verify child failed (status={status})")]
    VerifyChildFailed {
        /// Exit code or negative signal number from the isolate worker.
        status: i32,
    },
    /// Isolate worker could not fork for verify.
    #[error("verify fork error: {0}")]
    VerifyFork(#[source] std::io::Error),
    /// PTIMER stuck at zero while PMC read succeeded.
    #[error("PTIMER dead (lo=0 hi=0), PMC=0x{pmc:08x}")]
    VerifyPtimerDead {
        /// Last PMC_ENABLE sample from isolate batch.
        pmc: u32,
    },
    /// VRAM sentinel failed while PTIMER was alive (`verify` prefers soft-fail semantics).
    #[error("VRAM verify failed ({detail}); PTIMER ok")]
    VerifyVramSentinelFailed {
        /// Matches prior string detail (`ptimer=...`, etc.).
        detail: String,
    },
}

impl SovereignStagesError {
    /// Bridges `DriverResult`/`DriverError` from GR/FECS helpers into this enum without a
    /// `#[from]` impl (avoids ambiguity with [`DriverError::SovereignStages`] conversions).
    #[cfg(all(target_os = "linux", feature = "vfio"))]
    pub(crate) fn vfio_compute(err: DriverError) -> Self {
        Self::VfioCompute(Box::new(err))
    }
}
