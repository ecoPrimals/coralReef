// SPDX-License-Identifier: AGPL-3.0-only

//! Falcon microcontroller registers for GR engine falcons.
//!
//! FECS (Front-End Command Scheduler) and GPCCS (GPC Command Scheduler)
//! are Falcon-class microcontrollers that manage the GR (graphics/compute)
//! engine. Without signed firmware loaded via ACR secure boot, FECS stays
//! in HRESET and the PFIFO scheduler refuses to schedule channels on the
//! GR runlist — the root cause of Layer 7 dispatch failures on cold VFIO.
//!
//! PMU falcon is at 0x10A000 (separate from GR, documented in `devinit/pmu.rs`).

/// FECS falcon base address in BAR0.
pub const FECS_BASE: usize = 0x0040_9000;
/// GPCCS falcon base address in BAR0 (GPC0 instance).
pub const GPCCS_BASE: usize = 0x0041_A000;
/// PMU falcon base address in BAR0.
pub const PMU_BASE: usize = 0x0010_A000;
/// SEC2 falcon base address in BAR0.
/// GV100 topology (PTOP at 0x22700) places SEC2 at 0x087000, NOT the legacy 0x840000.
pub const SEC2_BASE: usize = 0x0008_7000;

// Per-falcon register offsets (add to base).
// From open-gpu-doc `dev_falcon_v4.ref.txt` and nouveau `nvkm/falcon/`.

/// IRQSSET — interrupt set (write to raise IRQ).
pub const IRQSSET: usize = 0x000;
/// IRQSCLR — interrupt clear (write to clear IRQ).
#[expect(
    dead_code,
    reason = "register map constant — will be used for IRQ unmask"
)]
pub const IRQSCLR: usize = 0x004;
/// IRQSTAT — interrupt status (read pending IRQs).
pub const IRQSTAT: usize = 0x008;
/// IRQMODE — interrupt routing/enable. Nouveau sets 0xfc24 for FECS/GPCCS.
pub const IRQMODE: usize = 0x00C;
/// IRQMSET — interrupt mask set.
#[expect(
    dead_code,
    reason = "hardware register map — used as reference during bring-up"
)]
pub const IRQMSET: usize = 0x010;
/// IRQMCLR — interrupt mask clear.
pub const IRQMCLR: usize = 0x014;
/// WATCHDOG — falcon watchdog timer. Set to 0x7FFFFFFF for long-running ops.
pub const WATCHDOG: usize = 0x034;
/// MAILBOX0 — general-purpose mailbox for host<->falcon communication.
pub const MAILBOX0: usize = 0x040;
/// MAILBOX1 — general-purpose mailbox.
pub const MAILBOX1: usize = 0x044;
/// ITFEN — interface enable. BIT(2) = ACCESS_EN for DMA.
pub const ITFEN: usize = 0x048;
/// OS — falcon OS/version register.
pub const OS: usize = 0x080;
/// DEBUG1 — debug/trace register.
pub const DEBUG1: usize = 0x090;
/// CPUCTL — CPU control: start, halt, reset.
/// v0-v3: Bit 0=STARTCPU. v4+ (GM200+): Bit 0=IINVAL, Bit 1=STARTCPU.
/// Bit 4: HRESET (read), Bit 5: HALTED (read).
/// Use [`FalconCapabilities::startcpu_value`] for version-correct access.
pub const CPUCTL: usize = 0x100;
/// BOOTVEC — boot vector address (PC on start).
pub const BOOTVEC: usize = 0x104;
/// HWCFG — hardware config: IMEM/DMEM sizes, security mode.
/// Bit 8: SECURITY_MODE (1 = signed-only firmware required).
pub const HWCFG: usize = 0x108;
/// DMACTL — DMA control register.
pub const DMACTL: usize = 0x10C;
/// IMEMC — IMEM control (for direct host upload).
pub const IMEMC: usize = 0x180;
/// IMEMD — IMEM data port.
pub const IMEMD: usize = 0x184;
/// IMEMT — IMEM tag port.
pub const IMEMT: usize = 0x188;
/// CPUCTL_ALIAS — alternate CPU control register (falcon v5+).
/// On HS falcons, host may need to use this instead of CPUCTL.
pub const CPUCTL_ALIAS: usize = 0x130;
/// DMEMC — DMEM control (for direct host upload).
pub const DMEMC: usize = 0x1C0;
/// DMEMD — DMEM data port.
pub const DMEMD: usize = 0x1C4;
/// CURCTX — current context pointer.
pub const CURCTX: usize = 0x118;
/// NXTCTX — next context pointer.
pub const NXTCTX: usize = 0x11C;

// SEC2-specific registers (beyond the common falcon set above).
// EMEM PIO is the host's interface for providing HS bootloaders to the
// falcon internal ROM. Always writable, even in full HS lockdown.

/// SCTL — security mode register (envytools: SEC_MODE at 0x240).
/// Bits[13:12]: SEC_MODE (0=NS, 1=LS, 2=HS). Fuse-enforced on GV100.
/// DOES NOT block host PIO to IMEM/DMEM — PIO works with correct IMEMC
/// format (BIT(24) write, BIT(25) read) regardless of security mode.
/// Use [`FalconCapabilities::security`] for structured decode.
pub const SCTL: usize = 0x240;
/// PC — falcon program counter (read-only snapshot of current execution address).
pub const PC: usize = 0x030;
/// EXCI — exception info: [31:16]=cause, [15:0]=PC.
pub const EXCI: usize = 0x148;
/// TRACEPC — trace program counter (write index to EXCI, read here).
pub const TRACEPC: usize = 0x14C;
/// ENGCTL — engine control register for falcon-local reset.
/// Write 0x01 to reset, 0x00 to release.
pub const ENGCTL: usize = 0x3C0;
/// EMEMC — EMEM control port 0. BIT(24)=write, BIT(25)=read, auto-inc.
pub const EMEMC0: usize = 0xAC0;
/// EMEMD — EMEM data port 0.
pub const EMEMD0: usize = 0xAC4;
/// Falcon DMA transfer base (external address, shifted >>8).
#[expect(
    dead_code,
    reason = "hardware register map — used as reference during bring-up"
)]
pub const DMATRFBASE: usize = 0x110;
/// Falcon DMA transfer IMEM/DMEM offset.
#[expect(
    dead_code,
    reason = "hardware register map — used as reference during bring-up"
)]
pub const DMATRFMOFFS: usize = 0x114;
/// Falcon DMA transfer command: bit 1=IMEM(1)/DMEM(0), bit 2=SIZE(0=256B,1=4B), bit 4=direction.
#[expect(
    dead_code,
    reason = "hardware register map — used as reference during bring-up"
)]
pub const DMATRFCMD: usize = 0x118;
/// Falcon DMA transfer framebuffer/external offset.
#[expect(
    dead_code,
    reason = "hardware register map — used as reference during bring-up"
)]
pub const DMATRFFBOFFS: usize = 0x11C;

/// Falcon v4+ CPUCTL: bit 0 = IINVAL (instruction cache invalidate).
/// On v0-v3, bit 0 was STARTCPU. On v4+, STARTCPU moved to bit 1.
pub const CPUCTL_IINVAL: u32 = 1 << 0;
/// Falcon v4+ CPUCTL: bit 1 = STARTCPU (release from HRESET).
/// nouveau `gm200_flcn_fw_boot` writes 0x02 to start the CPU.
pub const CPUCTL_STARTCPU: u32 = 1 << 1;
/// CPUCTL bit: falcon is in hard reset state.
pub const CPUCTL_HRESET: u32 = 1 << 4;
/// CPUCTL bit: falcon is halted.
pub const CPUCTL_HALTED: u32 = 1 << 5;
/// HWCFG bit: security mode — signed firmware required.
pub const HWCFG_SECURITY_MODE: u32 = 1 << 8;

/// FECS method data register (base + 0x500).
pub const MTHD_DATA: usize = 0x500;
/// FECS method command register (base + 0x504).
pub const MTHD_CMD: usize = 0x504;
/// FECS method status register (base + 0x800).
pub const MTHD_STATUS: usize = 0x800;
/// FECS method status2 register (base + 0x804).
pub const MTHD_STATUS2: usize = 0x804;
/// FECS exception configuration register (base + 0xC24).
pub const EXCEPTION_REG: usize = 0xC24;
/// GR class configuration register (base-relative within PGRAPH).
pub const GR_CLASS_CFG: usize = 0x802C;

/// FBIF_TRANSCFG — falcon bus interface configuration register.
///
/// Per-falcon DMA aperture control. Key bits:
/// - `[1:0]`: target mode (0=VIRT, 1=PHYS_VID, 2=PHYS_SYS_COH, 3=PHYS_SYS_NCOH)
/// - `[7]`:   physical addressing override (nouveau `nvkm_falcon_mask(falcon, 0x624, 0x80, 0x80)`)
///
/// The FBIF VIRT mode creates a circular dependency during instance block bind:
/// the MMU walker needs FBIF to read page tables from VRAM, but FBIF is set to
/// VIRT which requires the bind it's trying to complete. Setting PHYS_VID (0x01)
/// or the physical override bit (0x80) breaks this dependency.
pub const FBIF_TRANSCFG: usize = 0x624;
/// FBIF target mode: virtual addressing (requires active instance block bind).
#[expect(
    dead_code,
    reason = "hardware register map — VIRT mode is the reset default, documented for reference"
)]
pub const FBIF_TARGET_VIRT: u32 = 0x00;
/// FBIF target mode: physical video memory (bypasses MMU).
pub const FBIF_TARGET_PHYS_VID: u32 = 0x01;
/// FBIF physical addressing override bit (nouveau `0x80` mask).
pub const FBIF_PHYSICAL_OVERRIDE: u32 = 0x80;

/// Extract IMEM size in bytes from HWCFG register.
/// IMEM_SIZE field is bits [8:0] of HWCFG, in units of 256 bytes.
#[must_use]
pub const fn imem_size_bytes(hwcfg: u32) -> u32 {
    (hwcfg & 0x1FF) * 256
}

/// Extract DMEM size in bytes from HWCFG register.
/// DMEM_SIZE field is bits [17:9] of HWCFG, in units of 256 bytes.
#[must_use]
pub const fn dmem_size_bytes(hwcfg: u32) -> u32 {
    ((hwcfg >> 9) & 0x1FF) * 256
}
