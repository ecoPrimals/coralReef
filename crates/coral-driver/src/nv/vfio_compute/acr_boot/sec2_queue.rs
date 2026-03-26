// SPDX-License-Identifier: AGPL-3.0-only

//! SEC2 CMDQ/MSGQ ring protocol — host-side of the falcon conversation.
//!
//! After SEC2 boots its ACR firmware and sends the init message, the host
//! can communicate via two DMEM-resident circular buffers:
//!
//! - **CMDQ** (Command Queue): host writes commands, SEC2 reads them.
//! - **MSGQ** (Message Queue): SEC2 writes responses/events, host reads them.
//!
//! Queue layout is discovered at runtime from SEC2's init message
//! (`nv_sec2_init_msg`), which contains DMEM offsets and sizes for both queues.
//!
//! ## Protocol
//!
//! ```text
//! Host                         SEC2
//! ─────                        ────
//!  1. Boot SEC2 (ACR)
//!                      ──→  2. Write init_msg to MSGQ in DMEM
//!                      ──→  3. Write MSGQ head/tail regs
//!  4. Read init_msg (DMEM PIO)
//!  5. Discover CMDQ/MSGQ offsets
//!  6. Write cmd to CMDQ in DMEM
//!  7. Advance CMDQ head reg
//!  8. Poke IRQSSET to wake SEC2
//!                      ──→  9. Process cmd, write response to MSGQ
//!                      ──→ 10. Advance MSGQ head reg
//!                      ──→ 11. Raise IRQ to host
//! 12. Read response from MSGQ
//! 13. Advance MSGQ tail reg
//! ```
//!
//! ## Nouveau references
//!
//! - `nvkm/subdev/gsp/r535.c`: `nvkm_gsp_cmdq_push / _pop`
//! - `nvkm/subdev/acr/gp102.c`: SEC2 CMDQ protocol
//! - `nvkm/falcon/cmdq.c`: `nvkm_falcon_cmdq_send`
//! - `nvkm/falcon/msgq.c`: `nvkm_falcon_msgq_recv`

use crate::vfio::channel::registers::falcon;
use crate::vfio::device::MappedBar;

// ── SEC2-specific queue BAR0 register offsets (relative to falcon base) ──

/// CMDQ[0] head register (host writes, SEC2 reads).
const CMDQ0_HEAD: usize = 0xA00;
/// CMDQ[0] tail register (SEC2 writes after consuming).
const CMDQ0_TAIL: usize = 0xA04;
/// MSGQ[0] head register (SEC2 writes after producing).
const MSGQ0_HEAD: usize = 0xA30;
/// MSGQ[0] tail register (host writes after consuming).
const MSGQ0_TAIL: usize = 0xA34;

/// SEC2 unit IDs (from nouveau `nvfw/sec2.h`).
const NV_SEC2_UNIT_INIT: u8 = 0x01;
const NV_SEC2_UNIT_ACR: u8 = 0x08;

/// ACR command types.
const ACR_CMD_BOOTSTRAP_FALCON: u8 = 0x00;

/// Falcon IDs for BOOTSTRAP_FALCON command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum FalconId {
    /// FECS (Falcon Engine Control Subsystem).
    Fecs = 0x01,
    /// GPCCS (GPC Command Streamer).
    Gpccs = 0x03,
}

/// Discovered queue layout from SEC2 init message.
#[derive(Debug, Clone)]
#[must_use]
pub struct Sec2QueueInfo {
    /// DMEM offset of the CMDQ ring buffer.
    pub cmdq_offset: u32,
    /// Size of the CMDQ ring buffer in bytes.
    pub cmdq_size: u16,
    /// DMEM offset of the MSGQ ring buffer.
    pub msgq_offset: u32,
    /// Size of the MSGQ ring buffer in bytes.
    pub msgq_size: u16,
    /// OS debug entry point (from init message).
    pub os_debug_entry: u16,
}

/// Live handle to SEC2's CMDQ/MSGQ — holds discovered layout and
/// manages head/tail state for sending commands and receiving responses.
#[derive(Debug)]
pub struct Sec2Queues {
    info: Sec2QueueInfo,
    /// Next sequence ID for commands (wraps at 256).
    next_seq: u8,
}

/// A response message read from the MSGQ.
#[derive(Debug)]
#[must_use]
pub struct Sec2Message {
    /// Raw DMEM words of the message.
    pub words: Vec<u32>,
    /// Parsed unit_id from the message header.
    pub unit_id: u8,
    /// Parsed message size.
    pub size: u8,
    /// Parsed sequence ID (matches the command that triggered this).
    pub seq_id: u8,
}

/// Errors during queue operations.
#[derive(Debug)]
pub enum Sec2QueueError {
    /// Init message not found in DMEM.
    InitMessageNotFound,
    /// CMDQ is full (head would wrap into tail).
    CmdqFull,
    /// MSGQ is empty (no response available).
    MsgqEmpty,
    /// BAR0 read returned PRI error.
    PriError(u32),
    /// Message header invalid.
    BadMessageHeader(u32),
}

impl std::fmt::Display for Sec2QueueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InitMessageNotFound => write!(f, "SEC2 init message not found in DMEM"),
            Self::CmdqFull => write!(f, "SEC2 CMDQ full"),
            Self::MsgqEmpty => write!(f, "SEC2 MSGQ empty"),
            Self::PriError(v) => write!(f, "PRI error: {v:#010x}"),
            Self::BadMessageHeader(v) => write!(f, "bad message header: {v:#010x}"),
        }
    }
}

impl std::error::Error for Sec2QueueError {}

// ── DMEM PIO helpers ─────────────────────────────────────────────────

fn dmem_read_word(bar0: &MappedBar, addr: u32) -> u32 {
    let base = falcon::SEC2_BASE;
    let ctrl = (1u32 << 25) | (addr & 0xFFFC);
    let _ = bar0.write_u32(base + falcon::DMEMC, ctrl);
    bar0.read_u32(base + falcon::DMEMD).unwrap_or(0xDEAD_DEAD)
}

fn dmem_read_block(bar0: &MappedBar, start: u32, count: usize) -> Vec<u32> {
    let base = falcon::SEC2_BASE;
    let ctrl = (1u32 << 25) | (start & 0xFFFC);
    let _ = bar0.write_u32(base + falcon::DMEMC, ctrl);
    (0..count)
        .map(|_| bar0.read_u32(base + falcon::DMEMD).unwrap_or(0xDEAD_DEAD))
        .collect()
}

#[expect(dead_code, reason = "available for individual DMEM word writes")]
fn dmem_write_word(bar0: &MappedBar, addr: u32, val: u32) {
    let base = falcon::SEC2_BASE;
    let ctrl = (1u32 << 24) | (addr & 0xFFFC);
    let _ = bar0.write_u32(base + falcon::DMEMC, ctrl);
    let _ = bar0.write_u32(base + falcon::DMEMD, val);
}

fn dmem_write_block(bar0: &MappedBar, start: u32, words: &[u32]) {
    let base = falcon::SEC2_BASE;
    let ctrl = (1u32 << 24) | (start & 0xFFFC);
    let _ = bar0.write_u32(base + falcon::DMEMC, ctrl);
    for &w in words {
        let _ = bar0.write_u32(base + falcon::DMEMD, w);
    }
}

// ── Queue register helpers ───────────────────────────────────────────

fn read_queue_reg(bar0: &MappedBar, offset: usize) -> u32 {
    bar0.read_u32(falcon::SEC2_BASE + offset)
        .unwrap_or(0xBADF_DEAD)
}

fn write_queue_reg(bar0: &MappedBar, offset: usize, val: u32) {
    let _ = bar0.write_u32(falcon::SEC2_BASE + offset, val);
}

/// Poke SEC2's IRQSSET to wake the firmware for CMDQ processing.
fn poke_sec2_irq(bar0: &MappedBar) {
    let _ = bar0.write_u32(falcon::SEC2_BASE + falcon::IRQSSET, 0x40);
}

// ── Discovery ────────────────────────────────────────────────────────

impl Sec2Queues {
    /// Scan SEC2 DMEM for the init message and discover queue layout.
    ///
    /// The init message (`nv_sec2_init_msg`) has this layout:
    /// ```text
    /// offset 0: { u8 unit_id=0x01, u8 size, u8 ctrl_flags, u8 seq_id }
    /// offset 4: { u8 msg_type=0x00, u8 num_queues=2, u16 os_debug_entry }
    /// offset 8: queue_info[0] { u32 offset, u16 size, u8 index, u8 id }
    /// offset 16: queue_info[1] { u32 offset, u16 size, u8 index, u8 id }
    /// ```
    pub fn discover(bar0: &MappedBar) -> Result<Self, Sec2QueueError> {
        let hwcfg = bar0
            .read_u32(falcon::SEC2_BASE + falcon::HWCFG)
            .unwrap_or(0);
        let dmem_size = ((hwcfg >> 9) & 0x1FF) << 8;
        if dmem_size == 0 {
            return Err(Sec2QueueError::PriError(hwcfg));
        }

        for off in (0..dmem_size).step_by(4) {
            let w0 = dmem_read_word(bar0, off);
            let unit_id = w0 & 0xFF;
            let size = (w0 >> 8) & 0xFF;

            if unit_id == NV_SEC2_UNIT_INIT as u32 && (24..=48).contains(&size) {
                let w1 = dmem_read_word(bar0, off + 4);
                let msg_type = w1 & 0xFF;
                let num_queues = (w1 >> 8) & 0xFF;

                if msg_type == 0x00 && num_queues == 2 {
                    let block = dmem_read_block(bar0, off + 8, 4);

                    let q0_offset = block[0];
                    let q0_meta = block[1];
                    let q0_size = (q0_meta & 0xFFFF) as u16;
                    let q0_id = ((q0_meta >> 24) & 0xFF) as u8;

                    let q1_offset = block[2];
                    let q1_meta = block[3];
                    let q1_size = (q1_meta & 0xFFFF) as u16;
                    let _q1_id = ((q1_meta >> 24) & 0xFF) as u8;

                    let (cmdq_offset, cmdq_size, msgq_offset, msgq_size) = if q0_id == 0 {
                        (q0_offset, q0_size, q1_offset, q1_size)
                    } else {
                        (q1_offset, q1_size, q0_offset, q0_size)
                    };

                    let os_debug_entry = ((w1 >> 16) & 0xFFFF) as u16;

                    tracing::info!(
                        "SEC2 init message found at DMEM[{off:#06x}]: \
                         CMDQ(off={cmdq_offset:#x}, sz={cmdq_size}) \
                         MSGQ(off={msgq_offset:#x}, sz={msgq_size})"
                    );

                    return Ok(Self {
                        info: Sec2QueueInfo {
                            cmdq_offset,
                            cmdq_size,
                            msgq_offset,
                            msgq_size,
                            os_debug_entry,
                        },
                        next_seq: 0,
                    });
                }
            }
        }

        Err(Sec2QueueError::InitMessageNotFound)
    }

    /// Discover queues from pre-known offsets (skip DMEM scan).
    pub fn from_known(info: Sec2QueueInfo) -> Self {
        Self {
            info,
            next_seq: 0,
        }
    }

    /// Get the discovered queue info.
    pub fn info(&self) -> &Sec2QueueInfo {
        &self.info
    }

    // ── Send ─────────────────────────────────────────────────────────

    /// Send a BOOTSTRAP_FALCON command for the given falcon.
    ///
    /// This writes the command to the CMDQ in DMEM, advances the head
    /// register, and pokes SEC2's IRQ to trigger processing.
    pub fn cmd_bootstrap_falcon(
        &mut self,
        bar0: &MappedBar,
        falcon_id: FalconId,
    ) -> Result<u8, Sec2QueueError> {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);

        // nvfw_falcon_cmd header (4 bytes):
        //   unit_id=NV_SEC2_UNIT_ACR, size=16, ctrl_flags=0x03, seq_id
        // Then ACR-specific (12 bytes):
        //   cmd_type=BOOTSTRAP_FALCON, pad[3]
        //   flags=0 (RESET_YES)
        //   falcon_id
        let cmd: [u32; 4] = [
            u32::from(NV_SEC2_UNIT_ACR)
                | (0x10u32 << 8)
                | (0x03u32 << 16)
                | ((seq as u32) << 24),
            ACR_CMD_BOOTSTRAP_FALCON as u32,
            0x0000_0000, // flags = RESET_YES
            falcon_id as u32,
        ];

        self.send_raw(bar0, &cmd)?;
        Ok(seq)
    }

    /// Write raw command words into the CMDQ and poke SEC2.
    fn send_raw(&self, bar0: &MappedBar, words: &[u32]) -> Result<(), Sec2QueueError> {
        let head = read_queue_reg(bar0, CMDQ0_HEAD);
        let tail = read_queue_reg(bar0, CMDQ0_TAIL);
        let cmd_bytes = (words.len() * 4) as u32;

        let new_head = head.wrapping_add(cmd_bytes);
        if self.info.cmdq_size > 0 && new_head > self.info.cmdq_offset + self.info.cmdq_size as u32
        {
            return Err(Sec2QueueError::CmdqFull);
        }
        let _ = tail; // SEC2 advances tail when it consumes

        dmem_write_block(bar0, head, words);
        write_queue_reg(bar0, CMDQ0_HEAD, new_head);
        poke_sec2_irq(bar0);

        tracing::debug!(
            "SEC2 CMDQ: wrote {} words at DMEM[{head:#06x}], head {head:#x}→{new_head:#x}",
            words.len()
        );
        Ok(())
    }

    // ── Receive ──────────────────────────────────────────────────────

    /// Poll the MSGQ for a response. Returns `None` if the queue is empty.
    pub fn recv(&self, bar0: &MappedBar) -> Option<Sec2Message> {
        let head = read_queue_reg(bar0, MSGQ0_HEAD);
        let tail = read_queue_reg(bar0, MSGQ0_TAIL);

        if head == tail {
            return None;
        }

        let w0 = dmem_read_word(bar0, tail);
        let unit_id = (w0 & 0xFF) as u8;
        let size = ((w0 >> 8) & 0xFF) as u8;
        let seq_id = ((w0 >> 24) & 0xFF) as u8;

        if !(4..=128).contains(&size) {
            tracing::warn!("SEC2 MSGQ: bad header at DMEM[{tail:#06x}]: {w0:#010x}");
            return None;
        }

        let word_count = (size as usize).div_ceil(4);
        let words = dmem_read_block(bar0, tail, word_count);

        let new_tail = tail.wrapping_add(size as u32);
        write_queue_reg(bar0, MSGQ0_TAIL, new_tail);

        tracing::debug!(
            "SEC2 MSGQ: read {size}B at DMEM[{tail:#06x}], unit={unit_id:#04x} seq={seq_id}"
        );

        Some(Sec2Message {
            words,
            unit_id,
            size,
            seq_id,
        })
    }

    /// Poll MSGQ with timeout, waiting for a response matching `seq_id`.
    pub fn recv_wait(
        &self,
        bar0: &MappedBar,
        expected_seq: u8,
        timeout_ms: u64,
    ) -> Result<Sec2Message, Sec2QueueError> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(timeout_ms);

        loop {
            if let Some(msg) = self.recv(bar0) {
                if msg.seq_id == expected_seq {
                    return Ok(msg);
                }
                tracing::debug!(
                    "SEC2 MSGQ: skipping msg seq={} (expected {})",
                    msg.seq_id,
                    expected_seq
                );
            }

            if start.elapsed() > timeout {
                return Err(Sec2QueueError::MsgqEmpty);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    // ── Diagnostics ──────────────────────────────────────────────────

    /// Snapshot current queue register state for diagnostics.
    pub fn probe(bar0: &MappedBar) -> Sec2QueueProbe {
        Sec2QueueProbe {
            cmdq_head: read_queue_reg(bar0, CMDQ0_HEAD),
            cmdq_tail: read_queue_reg(bar0, CMDQ0_TAIL),
            msgq_head: read_queue_reg(bar0, MSGQ0_HEAD),
            msgq_tail: read_queue_reg(bar0, MSGQ0_TAIL),
        }
    }
}

/// Diagnostic snapshot of SEC2 queue registers.
#[derive(Debug, Clone)]
#[must_use]
pub struct Sec2QueueProbe {
    /// CMDQ head (host write position).
    pub cmdq_head: u32,
    /// CMDQ tail (SEC2 read position).
    pub cmdq_tail: u32,
    /// MSGQ head (SEC2 write position).
    pub msgq_head: u32,
    /// MSGQ tail (host read position).
    pub msgq_tail: u32,
}

impl std::fmt::Display for Sec2QueueProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SEC2 Queues: CMDQ(h={:#x} t={:#x} {}) MSGQ(h={:#x} t={:#x} {})",
            self.cmdq_head,
            self.cmdq_tail,
            if self.cmdq_head == self.cmdq_tail {
                "EMPTY"
            } else {
                "PENDING"
            },
            self.msgq_head,
            self.msgq_tail,
            if self.msgq_head == self.msgq_tail {
                "EMPTY"
            } else {
                "HAS DATA"
            },
        )
    }
}

impl Sec2QueueProbe {
    /// Whether any queue registers are non-zero (queues initialized).
    pub fn is_initialized(&self) -> bool {
        self.cmdq_head != 0
            || self.cmdq_tail != 0
            || self.msgq_head != 0
            || self.msgq_tail != 0
    }
}

/// Probe SEC2 queue state and attempt BOOTSTRAP_FALCON for GPCCS + FECS.
///
/// This is the shared "conversation probe" used by all boot strategies after
/// SEC2 has reached HS mode. It:
///
/// 1. Captures unified exit diagnostics (SCTL, EMEM, TRACEPC, EXCI).
/// 2. Captures a [`Sec2QueueProbe`] snapshot.
/// 3. Scans DMEM for the SEC2 init message via [`Sec2Queues::discover`].
/// 4. If queues are found, sends `BOOTSTRAP_FALCON` for GPCCS then FECS.
/// 5. Waits up to 2 s for each response.
/// 6. Appends all results to `notes`.
pub fn probe_and_bootstrap(bar0: &MappedBar, notes: &mut Vec<String>) {
    super::sec2_hal::sec2_exit_diagnostics(bar0, notes);

    let qprobe = Sec2Queues::probe(bar0);
    notes.push(format!("Queue probe: {qprobe}"));

    match Sec2Queues::discover(bar0) {
        Ok(mut queues) => {
            notes.push(format!(
                "SEC2 queues discovered: CMDQ(off={:#x} sz={}) MSGQ(off={:#x} sz={})",
                queues.info().cmdq_offset,
                queues.info().cmdq_size,
                queues.info().msgq_offset,
                queues.info().msgq_size,
            ));

            for (name, fid) in [("GPCCS", FalconId::Gpccs), ("FECS", FalconId::Fecs)] {
                match queues.cmd_bootstrap_falcon(bar0, fid) {
                    Ok(seq) => {
                        notes.push(format!("CMDQ: sent BOOTSTRAP_FALCON({name}) seq={seq}"));
                        match queues.recv_wait(bar0, seq, 2000) {
                            Ok(msg) => {
                                notes.push(format!(
                                    "MSGQ: response for {name}: unit={:#04x} size={} words={:?}",
                                    msg.unit_id,
                                    msg.size,
                                    msg.words
                                        .iter()
                                        .map(|w| format!("{w:#010x}"))
                                        .collect::<Vec<_>>()
                                ));
                            }
                            Err(e) => {
                                notes.push(format!("MSGQ: no response for {name}: {e}"));
                            }
                        }
                    }
                    Err(e) => {
                        notes.push(format!("CMDQ: {name} send failed: {e}"));
                    }
                }
            }
        }
        Err(e) => {
            notes.push(format!("SEC2 queue discovery failed: {e}"));
        }
    }
}

/// Build the BOOTSTRAP_FALCON command words for a given falcon (no hardware access).
///
/// Useful for tests and offline validation of the wire format.
pub fn build_bootstrap_cmd(seq: u8, falcon_id: FalconId) -> [u32; 4] {
    [
        u32::from(NV_SEC2_UNIT_ACR)
            | (0x10u32 << 8)
            | (0x03u32 << 16)
            | ((seq as u32) << 24),
        ACR_CMD_BOOTSTRAP_FALCON as u32,
        0x0000_0000,
        falcon_id as u32,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── FalconId ─────────────────────────────────────────────────────

    #[test]
    fn falcon_id_repr_matches_nouveau() {
        assert_eq!(FalconId::Fecs as u32, 0x01);
        assert_eq!(FalconId::Gpccs as u32, 0x03);
    }

    // ── Sec2QueueError display ───────────────────────────────────────

    #[test]
    fn error_display_strings() {
        assert_eq!(
            Sec2QueueError::InitMessageNotFound.to_string(),
            "SEC2 init message not found in DMEM"
        );
        assert_eq!(Sec2QueueError::CmdqFull.to_string(), "SEC2 CMDQ full");
        assert_eq!(Sec2QueueError::MsgqEmpty.to_string(), "SEC2 MSGQ empty");
        assert_eq!(
            Sec2QueueError::PriError(0xBADF_5040).to_string(),
            "PRI error: 0xbadf5040"
        );
        assert_eq!(
            Sec2QueueError::BadMessageHeader(0xDEAD).to_string(),
            "bad message header: 0x0000dead"
        );
    }

    #[test]
    fn error_is_std_error() {
        let e: Box<dyn std::error::Error> = Box::new(Sec2QueueError::MsgqEmpty);
        assert!(!e.to_string().is_empty());
    }

    // ── Sec2QueueInfo / from_known ───────────────────────────────────

    #[test]
    fn from_known_preserves_info() {
        let info = Sec2QueueInfo {
            cmdq_offset: 0x0F00,
            cmdq_size: 256,
            msgq_offset: 0x1000,
            msgq_size: 512,
            os_debug_entry: 0x42,
        };
        let queues = Sec2Queues::from_known(info.clone());
        assert_eq!(queues.info().cmdq_offset, 0x0F00);
        assert_eq!(queues.info().cmdq_size, 256);
        assert_eq!(queues.info().msgq_offset, 0x1000);
        assert_eq!(queues.info().msgq_size, 512);
        assert_eq!(queues.info().os_debug_entry, 0x42);
    }

    #[test]
    fn from_known_starts_seq_at_zero() {
        let info = Sec2QueueInfo {
            cmdq_offset: 0,
            cmdq_size: 64,
            msgq_offset: 0x100,
            msgq_size: 64,
            os_debug_entry: 0,
        };
        let queues = Sec2Queues::from_known(info);
        assert_eq!(queues.next_seq, 0);
    }

    // ── Command encoding ─────────────────────────────────────────────

    #[test]
    fn bootstrap_cmd_gpccs_wire_format() {
        let cmd = build_bootstrap_cmd(0, FalconId::Gpccs);
        // Word 0: unit=0x08, size=0x10, ctrl=0x03, seq=0x00
        assert_eq!(cmd[0], 0x0003_1008);
        // Word 1: cmd_type=BOOTSTRAP_FALCON (0x00)
        assert_eq!(cmd[1], 0x0000_0000);
        // Word 2: flags=RESET_YES (0x00)
        assert_eq!(cmd[2], 0x0000_0000);
        // Word 3: falcon_id=GPCCS (0x03)
        assert_eq!(cmd[3], 0x0000_0003);
    }

    #[test]
    fn bootstrap_cmd_fecs_wire_format() {
        let cmd = build_bootstrap_cmd(7, FalconId::Fecs);
        // seq=7 in bits [31:24]
        assert_eq!(cmd[0] >> 24, 7);
        // unit_id=0x08 in bits [7:0]
        assert_eq!(cmd[0] & 0xFF, NV_SEC2_UNIT_ACR as u32);
        // size=0x10 in bits [15:8]
        assert_eq!((cmd[0] >> 8) & 0xFF, 0x10);
        // falcon_id=FECS (0x01)
        assert_eq!(cmd[3], 0x0000_0001);
    }

    #[test]
    fn bootstrap_cmd_seq_wraps() {
        let cmd = build_bootstrap_cmd(255, FalconId::Gpccs);
        assert_eq!(cmd[0] >> 24, 255);
    }

    // ── Sec2QueueProbe ───────────────────────────────────────────────

    #[test]
    fn probe_all_zero_is_not_initialized() {
        let p = Sec2QueueProbe {
            cmdq_head: 0,
            cmdq_tail: 0,
            msgq_head: 0,
            msgq_tail: 0,
        };
        assert!(!p.is_initialized());
    }

    #[test]
    fn probe_any_nonzero_is_initialized() {
        for field in 0..4 {
            let mut p = Sec2QueueProbe {
                cmdq_head: 0,
                cmdq_tail: 0,
                msgq_head: 0,
                msgq_tail: 0,
            };
            match field {
                0 => p.cmdq_head = 1,
                1 => p.cmdq_tail = 1,
                2 => p.msgq_head = 1,
                3 => p.msgq_tail = 1,
                _ => unreachable!(),
            }
            assert!(
                p.is_initialized(),
                "field {field} should make probe initialized"
            );
        }
    }

    #[test]
    fn probe_display_empty() {
        let p = Sec2QueueProbe {
            cmdq_head: 0x100,
            cmdq_tail: 0x100,
            msgq_head: 0x200,
            msgq_tail: 0x200,
        };
        let s = p.to_string();
        assert!(s.contains("EMPTY"), "both queues equal → EMPTY: {s}");
    }

    #[test]
    fn probe_display_pending() {
        let p = Sec2QueueProbe {
            cmdq_head: 0x110,
            cmdq_tail: 0x100,
            msgq_head: 0x200,
            msgq_tail: 0x200,
        };
        let s = p.to_string();
        assert!(s.contains("PENDING"), "CMDQ head > tail → PENDING: {s}");
    }

    #[test]
    fn probe_display_has_data() {
        let p = Sec2QueueProbe {
            cmdq_head: 0x100,
            cmdq_tail: 0x100,
            msgq_head: 0x210,
            msgq_tail: 0x200,
        };
        let s = p.to_string();
        assert!(s.contains("HAS DATA"), "MSGQ head > tail → HAS DATA: {s}");
    }

    // ── Constants ────────────────────────────────────────────────────

    #[test]
    fn queue_register_offsets_match_nouveau() {
        assert_eq!(CMDQ0_HEAD, 0xA00);
        assert_eq!(CMDQ0_TAIL, 0xA04);
        assert_eq!(MSGQ0_HEAD, 0xA30);
        assert_eq!(MSGQ0_TAIL, 0xA34);
    }

    #[test]
    fn unit_ids_match_nouveau() {
        assert_eq!(NV_SEC2_UNIT_INIT, 0x01);
        assert_eq!(NV_SEC2_UNIT_ACR, 0x08);
    }

    #[test]
    fn acr_cmd_bootstrap_is_zero() {
        assert_eq!(ACR_CMD_BOOTSTRAP_FALCON, 0x00);
    }
}
