// SPDX-License-Identifier: AGPL-3.0-only
//! PMU mailbox register tracer — maps the communication protocol between
//! nouveau and the PMU falcon by polling mailbox registers alongside nouveau.
//!
//! Opens BAR0 via sysfs resource0 (same as hot_handoff_nouveau) and
//! continuously reads PMU falcon registers to observe the command protocol.
//!
//! Usage: sudo cargo run -p coral-driver --features vfio --example pmu_mailbox_trace -- 0000:03:00.0

use std::os::fd::AsFd;
use std::time::{Duration, Instant};

use coral_driver::nv::vfio_compute::falcon_capability::FalconProbe;
use coral_driver::vfio::device::MappedBar;
use rustix::mm::{MapFlags, ProtFlags, mmap};

const BAR0_SIZE: usize = 16 * 1024 * 1024;

// PMU falcon registers (base 0x10A000).
const PMU_BASE: usize = 0x0010_A000;
const PMU_CPUCTL: usize = PMU_BASE + 0x100;
const PMU_BOOTVEC: usize = PMU_BASE + 0x104;
const PMU_OS: usize = PMU_BASE + 0x080;
const PMU_MAILBOX0: usize = PMU_BASE + 0x040;
const PMU_MAILBOX1: usize = PMU_BASE + 0x044;
const PMU_IRQSTAT: usize = PMU_BASE + 0x008;
const PMU_IRQMODE: usize = PMU_BASE + 0x00C;
const PMU_DEBUG1: usize = PMU_BASE + 0x090;
const PMU_PC: usize = PMU_BASE + 0x030;
const PMU_ITFEN: usize = PMU_BASE + 0x048;
const PMU_DMACTL: usize = PMU_BASE + 0x10C;
const PMU_EXCI: usize = PMU_BASE + 0x148;

// PMU queue registers — Volta PMU uses a queue-based protocol.
// Queue doorbell: host writes to trigger processing.
// Queue head/tail: managed by firmware for ring buffer protocol.
const PMU_QUEUE_HEAD_BASE: usize = PMU_BASE + 0x4A0;
const PMU_QUEUE_TAIL_BASE: usize = PMU_BASE + 0x4B0;
const PMU_MSGQ_HEAD: usize = PMU_BASE + 0x4C8;
const PMU_MSGQ_TAIL: usize = PMU_BASE + 0x4CC;

// SEC2 falcon for comparison.
const SEC2_BASE: usize = 0x0008_7000;
const SEC2_CPUCTL: usize = SEC2_BASE + 0x100;
const SEC2_MAILBOX0: usize = SEC2_BASE + 0x040;
const SEC2_MAILBOX1: usize = SEC2_BASE + 0x044;

// FECS for comparison.
const FECS_BASE: usize = 0x0040_9000;
const FECS_CPUCTL: usize = FECS_BASE + 0x100;
const FECS_MAILBOX0: usize = FECS_BASE + 0x040;
const FECS_MAILBOX1: usize = FECS_BASE + 0x044;

#[derive(Default, Clone)]
struct PmuSnapshot {
    cpuctl: u32,
    bootvec: u32,
    os: u32,
    mbox0: u32,
    mbox1: u32,
    irqstat: u32,
    irqmode: u32,
    debug1: u32,
    pc: u32,
    itfen: u32,
    dmactl: u32,
    exci: u32,
    queue_head: [u32; 4],
    queue_tail: [u32; 4],
    msgq_head: u32,
    msgq_tail: u32,
    sec2_cpuctl: u32,
    sec2_mbox0: u32,
    sec2_mbox1: u32,
    fecs_cpuctl: u32,
    fecs_mbox0: u32,
    fecs_mbox1: u32,
}

impl PmuSnapshot {
    fn capture(bar0: &MappedBar) -> Self {
        let r = |reg: usize| bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD);
        let mut s = Self {
            cpuctl: r(PMU_CPUCTL),
            bootvec: r(PMU_BOOTVEC),
            os: r(PMU_OS),
            mbox0: r(PMU_MAILBOX0),
            mbox1: r(PMU_MAILBOX1),
            irqstat: r(PMU_IRQSTAT),
            irqmode: r(PMU_IRQMODE),
            debug1: r(PMU_DEBUG1),
            pc: r(PMU_PC),
            itfen: r(PMU_ITFEN),
            dmactl: r(PMU_DMACTL),
            exci: r(PMU_EXCI),
            queue_head: [0; 4],
            queue_tail: [0; 4],
            msgq_head: r(PMU_MSGQ_HEAD),
            msgq_tail: r(PMU_MSGQ_TAIL),
            sec2_cpuctl: r(SEC2_CPUCTL),
            sec2_mbox0: r(SEC2_MAILBOX0),
            sec2_mbox1: r(SEC2_MAILBOX1),
            fecs_cpuctl: r(FECS_CPUCTL),
            fecs_mbox0: r(FECS_MAILBOX0),
            fecs_mbox1: r(FECS_MAILBOX1),
        };
        for i in 0..4 {
            s.queue_head[i] = r(PMU_QUEUE_HEAD_BASE + i * 4);
            s.queue_tail[i] = r(PMU_QUEUE_TAIL_BASE + i * 4);
        }
        s
    }

    fn diff(&self, prev: &Self) -> Vec<String> {
        let mut changes = Vec::new();
        macro_rules! chk {
            ($field:ident, $name:expr) => {
                if self.$field != prev.$field {
                    changes.push(format!(
                        "{}: {:#010x} -> {:#010x}",
                        $name, prev.$field, self.$field
                    ));
                }
            };
        }
        chk!(cpuctl, "PMU_CPUCTL");
        chk!(mbox0, "PMU_MBOX0");
        chk!(mbox1, "PMU_MBOX1");
        chk!(irqstat, "PMU_IRQSTAT");
        chk!(debug1, "PMU_DEBUG1");
        chk!(pc, "PMU_PC");
        chk!(itfen, "PMU_ITFEN");
        chk!(dmactl, "PMU_DMACTL");
        chk!(exci, "PMU_EXCI");
        chk!(msgq_head, "PMU_MSGQ_HEAD");
        chk!(msgq_tail, "PMU_MSGQ_TAIL");
        chk!(sec2_cpuctl, "SEC2_CPUCTL");
        chk!(sec2_mbox0, "SEC2_MBOX0");
        chk!(sec2_mbox1, "SEC2_MBOX1");
        chk!(fecs_cpuctl, "FECS_CPUCTL");
        chk!(fecs_mbox0, "FECS_MBOX0");
        chk!(fecs_mbox1, "FECS_MBOX1");
        for i in 0..4 {
            if self.queue_head[i] != prev.queue_head[i] {
                changes.push(format!(
                    "PMU_QHEAD[{i}]: {:#010x} -> {:#010x}",
                    prev.queue_head[i], self.queue_head[i]
                ));
            }
            if self.queue_tail[i] != prev.queue_tail[i] {
                changes.push(format!(
                    "PMU_QTAIL[{i}]: {:#010x} -> {:#010x}",
                    prev.queue_tail[i], self.queue_tail[i]
                ));
            }
        }
        changes
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bdf = args.get(1).map(String::as_str).unwrap_or("0000:03:00.0");
    let duration_secs: u64 = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);
    let resource0_path = format!("/sys/bus/pci/devices/{bdf}/resource0");

    eprintln!("═══ PMU Mailbox Register Trace ═══");
    eprintln!("  BDF: {bdf}");
    eprintln!("  Duration: {duration_secs}s");

    let fd = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&resource0_path)
        .unwrap_or_else(|e| {
            eprintln!("ERROR: Cannot open {resource0_path}: {e}");
            std::process::exit(1);
        });

    let bar0 = unsafe {
        let ptr = mmap(
            std::ptr::null_mut(),
            BAR0_SIZE,
            ProtFlags::READ | ProtFlags::WRITE,
            MapFlags::SHARED,
            fd.as_fd(),
            0,
        ).unwrap_or_else(|e| {
            eprintln!("ERROR: mmap failed: {e}");
            std::process::exit(1);
        });
        MappedBar::from_raw(ptr.cast(), BAR0_SIZE)
    };
    std::mem::forget(fd);

    // Initial probe.
    eprintln!("\n▶ Firmware State:");
    let probe = FalconProbe::discover(&bar0);
    eprintln!("{probe}");

    // Initial snapshot.
    eprintln!("\n▶ PMU Register Snapshot (t=0):");
    let initial = PmuSnapshot::capture(&bar0);
    eprintln!("  CPUCTL   = {:#010x}", initial.cpuctl);
    eprintln!("  BOOTVEC  = {:#010x}", initial.bootvec);
    eprintln!("  OS       = {:#010x}", initial.os);
    eprintln!("  MBOX0    = {:#010x}", initial.mbox0);
    eprintln!("  MBOX1    = {:#010x}", initial.mbox1);
    eprintln!("  IRQSTAT  = {:#010x}", initial.irqstat);
    eprintln!("  IRQMODE  = {:#010x}", initial.irqmode);
    eprintln!("  DEBUG1   = {:#010x}", initial.debug1);
    eprintln!("  PC       = {:#010x}", initial.pc);
    eprintln!("  ITFEN    = {:#010x}", initial.itfen);
    eprintln!("  DMACTL   = {:#010x}", initial.dmactl);
    eprintln!("  EXCI     = {:#010x}", initial.exci);
    for i in 0..4 {
        eprintln!(
            "  QUEUE[{i}] HEAD={:#010x} TAIL={:#010x}",
            initial.queue_head[i], initial.queue_tail[i]
        );
    }
    eprintln!(
        "  MSGQ     HEAD={:#010x} TAIL={:#010x}",
        initial.msgq_head, initial.msgq_tail
    );
    eprintln!("  SEC2_CPUCTL = {:#010x}", initial.sec2_cpuctl);
    eprintln!("  SEC2_MBOX0  = {:#010x}", initial.sec2_mbox0);
    eprintln!("  FECS_CPUCTL = {:#010x}", initial.fecs_cpuctl);
    eprintln!("  FECS_MBOX0  = {:#010x}", initial.fecs_mbox0);

    // Poll for changes.
    eprintln!("\n▶ Polling for {duration_secs}s (changes only)...");
    let start = Instant::now();
    let mut prev = initial;
    let mut change_count = 0u32;
    let poll_interval = Duration::from_micros(100);

    while start.elapsed() < Duration::from_secs(duration_secs) {
        std::thread::sleep(poll_interval);
        let current = PmuSnapshot::capture(&bar0);
        let diffs = current.diff(&prev);
        if !diffs.is_empty() {
            let elapsed_ms = start.elapsed().as_millis();
            change_count += 1;
            eprintln!("  [{elapsed_ms:>6}ms] #{change_count}:");
            for d in &diffs {
                eprintln!("    {d}");
            }
            prev = current;
        }
    }

    eprintln!("\n▶ Summary:");
    eprintln!("  {change_count} register changes in {duration_secs}s");

    // Final snapshot.
    let final_snap = PmuSnapshot::capture(&bar0);
    let final_diffs = final_snap.diff(&PmuSnapshot::default());
    eprintln!("\n▶ Final PMU State:");
    eprintln!("  CPUCTL   = {:#010x}", final_snap.cpuctl);
    eprintln!("  MBOX0    = {:#010x}", final_snap.mbox0);
    eprintln!("  MBOX1    = {:#010x}", final_snap.mbox1);
    eprintln!("  PC       = {:#010x}", final_snap.pc);
    for i in 0..4 {
        eprintln!(
            "  QUEUE[{i}] HEAD={:#010x} TAIL={:#010x}",
            final_snap.queue_head[i], final_snap.queue_tail[i]
        );
    }
    eprintln!(
        "  MSGQ     HEAD={:#010x} TAIL={:#010x}",
        final_snap.msgq_head, final_snap.msgq_tail
    );

    // Protocol analysis.
    eprintln!("\n▶ Protocol Analysis:");
    let pmu_state = if final_snap.cpuctl & 0x20 != 0 {
        "WAITING (bit 5 — firmware loaded, idle)"
    } else if final_snap.cpuctl & 0x10 != 0 {
        "HALTED (bit 4 — firmware exited)"
    } else {
        "RUNNING (actively executing)"
    };
    eprintln!("  PMU state: {pmu_state}");

    if final_snap.mbox0 != 0 {
        eprintln!("  PMU MBOX0={:#010x} — possible status/version word", final_snap.mbox0);
    }

    let queues_active = (0..4).any(|i| final_snap.queue_head[i] != final_snap.queue_tail[i]);
    eprintln!(
        "  Queue activity: {}",
        if queues_active { "ACTIVE (head != tail)" } else { "IDLE (head == tail)" }
    );

    let msgq_pending = final_snap.msgq_head != final_snap.msgq_tail;
    eprintln!(
        "  Message queue: {}",
        if msgq_pending { "PENDING messages" } else { "EMPTY" }
    );

    if change_count == 0 {
        eprintln!("  No register changes observed — PMU is in idle state.");
        eprintln!("  This is expected when no GPU workload is running.");
        eprintln!("  To see mailbox activity, try running a GPU workload (glxgears, etc.).");
    } else {
        eprintln!("  {change_count} changes — mailbox protocol is active!");
    }
}
