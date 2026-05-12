// SPDX-License-Identifier: AGPL-3.0-or-later
#![allow(unsafe_code)]
//! Fork-isolated GPU diagnostic tool.
//!
//! Replaces ad-hoc Python `mmap` scripts that bypass all safety guards.
//! Every MMIO operation runs in a forked child with a kill-timeout, so a
//! D-state hang kills the child — not the system.
//!
//! Usage:
//!   coral-probe read  <bdf> <offset>          — single isolated register read
//!   coral-probe write <bdf> <offset> <value>   — single isolated register write
//!   coral-probe state <bdf>                    — dump key GPU state registers
//!   coral-probe fecs  <bdf>                    — FECS/GPCCS falcon diagnostics

use std::process::ExitCode;
use std::time::Duration;

use thiserror::Error;

const BAR0_SIZE: usize = 16 * 1024 * 1024;
const MMIO_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Error)]
enum ProbeError {
    #[error("open {path}: {source}")]
    ResourceOpen {
        path: String,
        source: std::io::Error,
    },
    #[error("mmap {path}: {detail}")]
    Mmap { path: String, detail: String },
    #[error("TIMEOUT {op} {offset:#010x} — GPU hung (child killed, system safe)")]
    Timeout { op: &'static str, offset: u32 },
    #[error("TIMEOUT during batch read — GPU hung (child killed, system safe)")]
    BatchTimeout,
    #[error("child failed {op} {offset:#010x}: status={status}")]
    ChildFailed {
        op: &'static str,
        offset: u32,
        status: i32,
    },
    #[error("child failed batch read: status={status}")]
    BatchChildFailed { status: i32 },
    #[error("fork error: {0}")]
    Fork(std::io::Error),
    #[error("bad hex '{input}': {source}")]
    HexParse {
        input: String,
        source: std::num::ParseIntError,
    },
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        return usage();
    }

    let cmd = args[1].as_str();
    let bdf = &args[2];

    match cmd {
        "read" => cmd_read(bdf, &args[3..]),
        "write" => cmd_write(bdf, &args[3..]),
        "state" => cmd_state(bdf),
        "fecs" => cmd_fecs(bdf),
        _ => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!("coral-probe — fork-isolated GPU diagnostic tool");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  coral-probe read  <bdf> <offset>");
    eprintln!("  coral-probe write <bdf> <offset> <value>");
    eprintln!("  coral-probe state <bdf>");
    eprintln!("  coral-probe fecs  <bdf>");
    eprintln!();
    eprintln!("All MMIO ops are fork-isolated with a {MMIO_TIMEOUT:?} kill-timeout.");
    eprintln!("If the GPU hangs, the child dies — not the system.");
    ExitCode::FAILURE
}

// ---------------------------------------------------------------------------
// BAR0 mapping (read-write, via sysfs resource0)
// ---------------------------------------------------------------------------

struct Bar0Map {
    ptr: *mut u8,
    _fd: std::fs::File,
}

impl Bar0Map {
    fn open(bdf: &str) -> Result<Self, ProbeError> {
        let sysfs = coral_driver::linux_paths::sysfs_root();
        let path = format!("{sysfs}/bus/pci/devices/{bdf}/resource0");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|source| ProbeError::ResourceOpen {
                path: path.clone(),
                source,
            })?;

        // SAFETY: `file` is an open `resource0` fd for this BDF; BAR0 is mmap'd
        // MAP_SHARED with RW for the first `BAR0_SIZE` bytes (PCI standard window).
        // Length and offset 0 match the opened resource; alignment is chosen by the kernel.
        let raw = unsafe {
            rustix::mm::mmap(
                std::ptr::null_mut(),
                BAR0_SIZE,
                rustix::mm::ProtFlags::READ | rustix::mm::ProtFlags::WRITE,
                rustix::mm::MapFlags::SHARED,
                &file,
                0,
            )
        }
        .map_err(|e| ProbeError::Mmap {
            path: path.clone(),
            detail: e.to_string(),
        })?;

        if raw.is_null() {
            return Err(ProbeError::Mmap {
                path,
                detail: "returned null".into(),
            });
        }

        Ok(Self {
            ptr: raw.cast::<u8>(),
            _fd: file,
        })
    }

    const fn base(&self) -> *mut u8 {
        self.ptr
    }
}

impl Drop for Bar0Map {
    fn drop(&mut self) {
        // SAFETY: `self.ptr`/`BAR0_SIZE` pair comes from the successful `mmap` in `open`.
        unsafe {
            let _ = rustix::mm::munmap(self.ptr.cast(), BAR0_SIZE);
        }
    }
}

// ---------------------------------------------------------------------------
// Fork-isolated read/write using coral-driver's isolation module
// ---------------------------------------------------------------------------

fn isolated_read(bar0_ptr: *const u8, offset: u32, timeout: Duration) -> Result<u32, ProbeError> {
    use coral_driver::vfio::isolation::{IsolationResult, fork_isolated_mmio_read};
    // SAFETY: `bar0_ptr` is the BAR0 map base from `Bar0Map`; caller offsets are in-range
    // for the 16MiB mapping (see `fork_isolated_mmio_read` safety contract).
    match unsafe { fork_isolated_mmio_read(bar0_ptr, offset, timeout) } {
        IsolationResult::Ok(v) => Ok(v),
        IsolationResult::Timeout => Err(ProbeError::Timeout {
            op: "reading",
            offset,
        }),
        IsolationResult::ChildFailed { status } => Err(ProbeError::ChildFailed {
            op: "reading",
            offset,
            status,
        }),
        IsolationResult::ForkError(e) => Err(ProbeError::Fork(e)),
    }
}

fn isolated_write(
    bar0_ptr: *mut u8,
    offset: u32,
    value: u32,
    timeout: Duration,
) -> Result<(), ProbeError> {
    use coral_driver::vfio::isolation::{IsolationResult, fork_isolated_mmio_write};
    // SAFETY: `bar0_ptr` is the BAR0 map base; `offset + 4` lies within the same mapping.
    match unsafe { fork_isolated_mmio_write(bar0_ptr, offset, value, timeout) } {
        IsolationResult::Ok(()) => Ok(()),
        IsolationResult::Timeout => Err(ProbeError::Timeout {
            op: "writing",
            offset,
        }),
        IsolationResult::ChildFailed { status } => Err(ProbeError::ChildFailed {
            op: "writing",
            offset,
            status,
        }),
        IsolationResult::ForkError(e) => Err(ProbeError::Fork(e)),
    }
}

fn isolated_batch_read(
    bar0_ptr: *mut u8,
    offsets: &[u32],
    timeout: Duration,
) -> Result<Vec<u32>, ProbeError> {
    use coral_driver::vfio::isolation::{IsolationResult, fork_isolated_mmio_batch};
    let ops: Vec<(u32, Option<u32>)> = offsets.iter().map(|&o| (o, None)).collect();
    // SAFETY: `bar0_ptr` is the BAR0 base; every read offset in `ops` is below the map size.
    match unsafe { fork_isolated_mmio_batch(bar0_ptr, &ops, timeout) } {
        IsolationResult::Ok(vals) => Ok(vals),
        IsolationResult::Timeout => Err(ProbeError::BatchTimeout),
        IsolationResult::ChildFailed { status } => Err(ProbeError::BatchChildFailed { status }),
        IsolationResult::ForkError(e) => Err(ProbeError::Fork(e)),
    }
}

// ---------------------------------------------------------------------------
// PRI fault detection
// ---------------------------------------------------------------------------

const fn is_pri_fault(val: u32) -> bool {
    val == 0xFFFF_FFFF || val == 0xDEAD_DEAD || (val >> 16) == 0xBADF || (val >> 16) == 0xBAD0
}

fn fmt_reg(name: &str, val: u32) -> String {
    let suffix = if is_pri_fault(val) {
        " [PRI FAULT]"
    } else {
        ""
    };
    format!("  {name:<20} = {val:#010x}{suffix}")
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

fn parse_hex(s: &str) -> Result<u32, ProbeError> {
    let stripped = s.strip_prefix("0x").unwrap_or(s);
    u32::from_str_radix(stripped, 16).map_err(|source| ProbeError::HexParse {
        input: s.to_owned(),
        source,
    })
}

fn cmd_read(bdf: &str, args: &[String]) -> ExitCode {
    if args.is_empty() {
        eprintln!("usage: coral-probe read <bdf> <offset>");
        return ExitCode::FAILURE;
    }
    let offset = match parse_hex(&args[0]) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    let bar0 = match Bar0Map::open(bdf) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    match isolated_read(bar0.base(), offset, MMIO_TIMEOUT) {
        Ok(val) => {
            println!("{bdf} [{offset:#010x}] = {val:#010x}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("FATAL: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_write(bdf: &str, args: &[String]) -> ExitCode {
    if args.len() < 2 {
        eprintln!("usage: coral-probe write <bdf> <offset> <value>");
        return ExitCode::FAILURE;
    }
    let offset = match parse_hex(&args[0]) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let value = match parse_hex(&args[1]) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    let bar0 = match Bar0Map::open(bdf) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    match isolated_write(bar0.base(), offset, value, MMIO_TIMEOUT) {
        Ok(()) => {
            println!("OK: {bdf} [{offset:#010x}] <- {value:#010x}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("FATAL: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_state(bdf: &str) -> ExitCode {
    let bar0 = match Bar0Map::open(bdf) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    let offsets: &[u32] = &[
        0x0000_0000, // BOOT0
        0x0000_0200, // PMC_ENABLE
        0x0000_0640, // PMC secondary
        0x0000_9400, // PTIMER
        0x0012_004C, // PRI_RING_MASTER
        0x0013_0000, // PLL0 ctrl
        0x0013_0004, // PLL0 coeff
        0x0013_7000, // PCLOCK_CTRL
        0x0040_9100, // FECS CPUCTL
        0x0040_93C0, // FECS ENGCTL
        0x0040_9030, // FECS PC
        0x0040_9800, // FECS STATUS
        0x0040_9604, // GPC topology
        0x0041_A100, // GPCCS CPUCTL
        0x0041_A3C0, // GPCCS ENGCTL
        0x0040_0100, // GR_INTR
        0x0040_0700, // GR_STATUS
    ];

    let names: &[&str] = &[
        "BOOT0",
        "PMC_ENABLE",
        "PMC_0x640",
        "PTIMER",
        "PRI_RING_MASTER",
        "PLL0_CTRL",
        "PLL0_COEFF",
        "PCLOCK_CTRL",
        "FECS_CPUCTL",
        "FECS_ENGCTL",
        "FECS_PC",
        "FECS_STATUS",
        "GPC_TOPOLOGY",
        "GPCCS_CPUCTL",
        "GPCCS_ENGCTL",
        "GR_INTR",
        "GR_STATUS",
    ];

    match isolated_batch_read(bar0.base(), offsets, MMIO_TIMEOUT) {
        Ok(vals) => {
            println!("GPU state for {bdf}:");
            let boot0 = vals[0];
            if boot0 == 0xFFFF_FFFF {
                println!("  BOOT0 = 0xFFFFFFFF — PCIe LINK DOWN, device is dead");
                return ExitCode::FAILURE;
            }
            for (name, &val) in names.iter().zip(vals.iter()) {
                println!("{}", fmt_reg(name, val));
            }

            let pmc = vals[1];
            println!();
            println!("  PMC decode:");
            println!(
                "    PGRAPH (bit 12) = {}",
                if pmc & (1 << 12) != 0 { "ON" } else { "OFF" }
            );
            println!(
                "    PFIFO  (bit  8) = {}",
                if pmc & (1 << 8) != 0 { "ON" } else { "OFF" }
            );
            println!(
                "    PCOPY  (bit  6) = {}",
                if pmc & (1 << 6) != 0 { "ON" } else { "OFF" }
            );

            let fecs_cpuctl = vals[8];
            println!();
            println!("  FECS decode:");
            println!(
                "    HALTED = {}",
                if fecs_cpuctl & 0x10 != 0 { "YES" } else { "no" }
            );
            println!(
                "    CPUCTL bit 6 (alias) = {}",
                if fecs_cpuctl & 0x40 != 0 {
                    "YES (use 0x130 for STARTCPU)"
                } else {
                    "no (use 0x100)"
                }
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("FATAL: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_fecs(bdf: &str) -> ExitCode {
    const FECS_BASE: u32 = 0x0040_9000;

    let bar0 = match Bar0Map::open(bdf) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    let offsets: &[u32] = &[
        0x0000_0000,       // BOOT0
        0x0000_0200,       // PMC
        FECS_BASE + 0x100, // CPUCTL
        FECS_BASE + 0x104, // BOOTVEC
        FECS_BASE + 0x108, // HWCFG
        FECS_BASE + 0x10C, // DMACTL
        FECS_BASE + 0x030, // PC
        FECS_BASE + 0x148, // EXCI
        FECS_BASE + 0x040, // MAILBOX0
        FECS_BASE + 0x044, // MAILBOX1
        FECS_BASE + 0x3C0, // ENGCTL
        FECS_BASE + 0x500, // SCRATCH0
        FECS_BASE + 0x504, // SCRATCH1
        FECS_BASE + 0x840, // INTR_UP
        FECS_BASE + 0x804, // CTX_SIZE
        0x0040_9800,       // FECS_STATUS
        0x0040_9604,       // GPC topology
        0x0041_A100,       // GPCCS CPUCTL
        0x0041_A3C0,       // GPCCS ENGCTL
        0x0041_A030,       // GPCCS PC
    ];
    let names: &[&str] = &[
        "BOOT0",
        "PMC_ENABLE",
        "FECS_CPUCTL",
        "FECS_BOOTVEC",
        "FECS_HWCFG",
        "FECS_DMACTL",
        "FECS_PC",
        "FECS_EXCI",
        "FECS_MAILBOX0",
        "FECS_MAILBOX1",
        "FECS_ENGCTL",
        "FECS_SCRATCH0",
        "FECS_SCRATCH1",
        "FECS_INTR_UP",
        "FECS_CTX_SIZE",
        "FECS_STATUS",
        "GPC_TOPOLOGY",
        "GPCCS_CPUCTL",
        "GPCCS_ENGCTL",
        "GPCCS_PC",
    ];

    match isolated_batch_read(bar0.base(), offsets, MMIO_TIMEOUT) {
        Ok(vals) => {
            println!("FECS/GPCCS diagnostic for {bdf}:");
            for (name, &val) in names.iter().zip(vals.iter()) {
                println!("{}", fmt_reg(name, val));
            }

            let hwcfg = vals[4];
            if !is_pri_fault(hwcfg) {
                let imem_size = 256 << ((hwcfg >> 4) & 0x1F);
                let dmem_size = ((hwcfg >> 9) & 0x1FF) * 256;
                println!();
                println!("  HWCFG decode:");
                println!("    IMEM = {imem_size} bytes");
                println!("    DMEM = {dmem_size} bytes");
            }

            let cpuctl = vals[2];
            let pc = vals[6];
            let exci = vals[7];
            println!();
            if is_pri_fault(cpuctl) {
                println!("  FECS: PRI FAULT — PGRAPH likely not enabled in PMC");
            } else if cpuctl & 0x10 != 0 && pc == 0 && exci == 0 {
                println!(
                    "  FECS: HALTED at PC=0, no exception — firmware never started or in HRESET"
                );
            } else if cpuctl & 0x10 != 0 && pc != 0 {
                println!("  FECS: HALTED at PC={pc:#010x}, EXCI={exci:#010x} — firmware trapped");
            } else if cpuctl & 0x10 == 0 {
                println!("  FECS: RUNNING at PC={pc:#010x}");
            }

            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("FATAL: {e}");
            ExitCode::FAILURE
        }
    }
}
