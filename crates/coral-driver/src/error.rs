// SPDX-License-Identifier: AGPL-3.0-or-later
//! Driver error types.

use std::borrow::Cow;

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
}

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

/// Result alias for driver operations.
///
/// All GPU device operations return this type; errors are [`DriverError`] variants.
pub type DriverResult<T> = Result<T, DriverError>;

/// Errors from GPU device operations.
///
/// String-carrying variants use `Cow<'static, str>` so that static messages
/// (the common case) are zero-alloc, while dynamic messages still work via
/// `format!("...").into()`.
#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    /// No matching GPU device was found (e.g. no amdgpu/nouveau render node).
    #[error("device not found: {0}")]
    DeviceNotFound(Cow<'static, str>),

    /// A DRM ioctl syscall failed; the kernel returned an error.
    #[error("DRM ioctl failed: {name} returned {errno}")]
    IoctlFailed {
        /// Name of the ioctl for error reporting.
        name: &'static str,
        /// Kernel errno (negative on Linux).
        errno: i32,
    },

    /// Buffer allocation failed (OOM or invalid domain).
    #[error("buffer allocation failed: size={size}, domain={domain:?} — {detail}")]
    AllocFailed {
        /// Requested buffer size in bytes.
        size: u64,
        /// Memory domain that was requested.
        domain: crate::MemoryDomain,
        /// Additional context.
        detail: String,
    },

    /// The buffer handle is invalid or was already freed.
    #[error("buffer not found: handle={0:?}")]
    BufferNotFound(crate::BufferHandle),

    /// Memory mapping of a GEM buffer failed.
    #[error("mmap failed: {0}")]
    MmapFailed(Cow<'static, str>),

    /// Command submission to the GPU failed.
    #[error("command submission failed: {0}")]
    SubmitFailed(Cow<'static, str>),

    /// The fence did not signal within the timeout period.
    #[error("fence timeout after {ms}ms")]
    FenceTimeout {
        /// Timeout duration in milliseconds.
        ms: u64,
    },

    /// Device open / context creation failed.
    #[error("device open failed: {0}")]
    OpenFailed(Cow<'static, str>),

    /// Compute dispatch (kernel launch) failed.
    #[error("dispatch failed: {0}")]
    DispatchFailed(Cow<'static, str>),

    /// GPU synchronization (fence / stream sync) failed.
    #[error("sync failed: {0}")]
    SyncFailed(Cow<'static, str>),

    /// Oracle / BAR0 register operation failed (page table walk, PMU probe, etc.).
    #[error("oracle error: {0}")]
    OracleError(Cow<'static, str>),

    /// Wrapped I/O error from file operations.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Operation or API not available for this device / backend (e.g. legacy VFIO group fd on iommufd).
    #[error("unsupported: {0}")]
    Unsupported(Cow<'static, str>),

    /// PCI sysfs/config-space discovery or PM transition failed.
    #[error("PCI discovery: {0}")]
    PciDiscovery(#[from] PciDiscoveryError),

    /// VFIO channel oracle / BAR0 resource access failed.
    #[error("channel: {0}")]
    Channel(#[from] ChannelError),

    /// VBIOS / devinit (PROM, interpreter, PMU upload) failed.
    #[error("devinit: {0}")]
    Devinit(#[from] DevinitError),
}

impl DriverError {
    /// Platform overflow during numeric conversion (e.g. `usize`→`u64`, `u64`→`off_t`).
    /// Used for conversions that cannot fail on 64-bit Linux but should still
    /// propagate as errors rather than panicking.
    pub(crate) fn platform_overflow(msg: &'static str) -> Self {
        Self::MmapFailed(msg.into())
    }

    /// Create an oracle error from a dynamic string (bridges `Result<T, String>`
    /// from the oracle module into `DriverResult`).
    pub fn oracle(msg: impl Into<Cow<'static, str>>) -> Self {
        Self::OracleError(msg.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn error_display_device_not_found() {
        let e = DriverError::DeviceNotFound("no amdgpu".into());
        assert!(e.to_string().contains("no amdgpu"));
    }

    #[test]
    fn error_display_ioctl_failed() {
        let e = DriverError::IoctlFailed {
            name: "drm_ioctl",
            errno: -22,
        };
        let msg = e.to_string();
        assert!(msg.contains("drm_ioctl"));
        assert!(msg.contains("-22"));
    }

    #[test]
    fn error_display_alloc_failed() {
        let e = DriverError::AllocFailed {
            size: 4096,
            domain: crate::MemoryDomain::Vram,
            detail: "oom".into(),
        };
        assert!(e.to_string().contains("4096"));
    }

    #[test]
    fn error_display_buffer_not_found() {
        let e = DriverError::BufferNotFound(crate::BufferHandle(42));
        assert!(e.to_string().contains("42"));
    }

    #[test]
    fn error_display_mmap_failed() {
        let e = DriverError::MmapFailed("out of memory".into());
        assert!(e.to_string().contains("out of memory"));
    }

    #[test]
    fn error_display_submit_failed() {
        let e = DriverError::SubmitFailed("context lost".into());
        assert!(e.to_string().contains("context lost"));
    }

    #[test]
    fn error_display_fence_timeout() {
        let e = DriverError::FenceTimeout { ms: 5000 };
        assert!(e.to_string().contains("5000"));
    }

    #[test]
    fn error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "no device");
        let e: DriverError = io_err.into();
        assert!(e.to_string().contains("no device"));
    }

    #[test]
    fn error_is_std_error() {
        let e: Box<dyn std::error::Error> = Box::new(DriverError::DeviceNotFound("test".into()));
        assert!(e.to_string().contains("test"));
    }

    #[test]
    fn error_platform_overflow() {
        let e = DriverError::platform_overflow("offset exceeds platform pointer width");
        let msg = e.to_string();
        assert!(msg.contains("offset exceeds platform pointer width"));
    }

    #[test]
    fn error_alloc_failed_domain_display() {
        for domain in [
            crate::MemoryDomain::Vram,
            crate::MemoryDomain::Gtt,
            crate::MemoryDomain::VramOrGtt,
        ] {
            let e = DriverError::AllocFailed {
                size: 8192,
                domain,
                detail: "test".into(),
            };
            let msg = e.to_string();
            assert!(msg.contains("8192"));
            assert!(msg.contains("domain"));
        }
    }

    #[test]
    fn error_debug_format() {
        let e = DriverError::DeviceNotFound("probe failed".into());
        let debug = format!("{e:?}");
        assert!(debug.contains("DeviceNotFound"));
        assert!(debug.contains("probe failed"));
    }

    #[test]
    fn error_display_dynamic_cow() {
        let msg = format!("custom error: {}", 42);
        let e = DriverError::MmapFailed(msg.into());
        assert!(e.to_string().contains("custom error: 42"));
    }

    #[test]
    fn error_display_device_not_found_static() {
        let e = DriverError::DeviceNotFound(Cow::Borrowed("static message"));
        assert_eq!(e.to_string(), "device not found: static message");
    }

    #[test]
    fn error_source_chain() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "root required");
        let e: DriverError = io_err.into();
        let source = e.source();
        assert!(source.is_some());
        assert!(source.unwrap().to_string().contains("root required"));
    }

    #[test]
    fn error_display_io_variant() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let e: DriverError = io_err.into();
        let msg = e.to_string();
        assert!(msg.contains("I/O"), "Io variant should display 'I/O'");
        assert!(msg.contains("file not found"));
    }

    #[test]
    fn error_from_io_conversion() {
        let inner = std::io::Error::new(std::io::ErrorKind::WouldBlock, "would block");
        let e: DriverError = DriverError::from(inner);
        assert!(matches!(e, DriverError::Io(_)));
        assert!(e.to_string().contains("would block"));
    }

    #[test]
    fn error_display_unsupported() {
        let e = DriverError::Unsupported("legacy API on iommufd".into());
        assert!(e.to_string().contains("unsupported"));
        assert!(e.to_string().contains("legacy API"));
    }

    #[test]
    fn error_display_pci_discovery_variant() {
        let inner = PciDiscoveryError::InvalidBdf { bdf: "bad".into() };
        let e: DriverError = inner.into();
        assert!(e.to_string().contains("PCI discovery"));
        assert!(e.to_string().contains("bad"));
    }

    #[test]
    fn error_display_channel_variant() {
        let inner = ChannelError::Bar0ReadsAllOnes;
        let e: DriverError = inner.into();
        assert!(e.to_string().contains("channel"));
        assert!(e.to_string().contains("0xFFFFFFFF"));
    }

    #[test]
    fn error_display_devinit_variant() {
        let inner = DevinitError::BitSignatureNotFound;
        let e: DriverError = inner.into();
        assert!(e.to_string().contains("devinit"));
        assert!(e.to_string().contains("BIT"));
    }

    #[test]
    fn error_display_bar0_oob() {
        let e = ChannelError::Bar0ReadOutOfBounds {
            offset: 0x1000_0000,
            map_size: 0x0100_0000,
        };
        let s = e.to_string();
        assert!(s.contains("read out of bounds"));
        assert!(s.contains("10000000"));
    }

    #[test]
    fn error_channel_resource_io_source() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let e = ChannelError::resource_io("read", "/tmp/x", io_err);
        let s = e.to_string();
        assert!(s.contains("read"));
        assert!(s.contains("/tmp/x"));
        let de: DriverError = e.into();
        assert!(de.source().is_some());
    }

    #[test]
    fn error_devinit_vbios_resource_io_source() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let e = DevinitError::vbios_resource_io("read", "/sys/.../rom", io_err);
        let de: DriverError = e.into();
        assert!(de.source().is_some());
        assert!(de.to_string().contains("devinit"));
    }

    #[test]
    fn pci_discovery_config_too_short_display() {
        let e = PciDiscoveryError::ConfigTooShort { len: 32, need: 64 };
        let s = e.to_string();
        assert!(s.contains("32"));
        assert!(s.contains("64"));
    }

    #[test]
    fn error_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DriverError>();
    }

    #[test]
    fn error_display_open_failed() {
        let e = DriverError::OpenFailed("permission denied".into());
        assert!(e.to_string().contains("permission denied"));
    }

    #[test]
    fn error_display_dispatch_failed() {
        let e = DriverError::DispatchFailed("illegal instruction".into());
        assert!(e.to_string().contains("illegal instruction"));
    }

    #[test]
    fn error_display_sync_failed() {
        let e = DriverError::SyncFailed("fence wait failed".into());
        assert!(e.to_string().contains("fence wait failed"));
    }

    #[test]
    fn error_display_oracle_error() {
        let e = DriverError::OracleError("bar0 walk failed".into());
        assert!(e.to_string().contains("bar0 walk failed"));
    }

    #[test]
    fn error_oracle_helper_builds_variant() {
        let e = DriverError::oracle("dynamic oracle message");
        assert!(matches!(e, DriverError::OracleError(_)));
        assert!(e.to_string().contains("dynamic oracle message"));
    }

    #[test]
    fn error_oracle_static_cow() {
        let e = DriverError::oracle(Cow::Borrowed("static oracle"));
        assert_eq!(e.to_string(), "oracle error: static oracle");
    }
}
