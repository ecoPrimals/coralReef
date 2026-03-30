// SPDX-License-Identifier: AGPL-3.0-only

//! Shared [`AcrBootResult`] type and helpers for strategy modules.

use std::fmt;

use crate::vfio::channel::registers::falcon;
use crate::vfio::device::MappedBar;

use super::sec2_hal::Sec2Probe;

// ── Boot journal callback ────────────────────────────────────────────

/// Trait for persisting boot attempt results (e.g. to Ember's JSONL journal).
///
/// Implemented by the caller (test harness, GlowPlug, etc.) to bridge
/// coral-driver's boot solver to whatever persistence layer is available.
/// coral-driver itself has no dependency on coral-ember or coral-glowplug.
pub trait BootJournal: Send + Sync {
    /// Called after each boot strategy attempt with the full result.
    fn record_boot_attempt(&self, result: &AcrBootResult);
}

// ── Boot attempts ────────────────────────────────────────────────────

/// Result of a boot chain attempt.
#[derive(Debug)]
pub struct AcrBootResult {
    /// Name of the boot strategy (EMEM, IMEM, mailbox, etc.).
    pub strategy: &'static str,
    /// SEC2 probe before the attempt.
    pub sec2_before: Sec2Probe,
    /// SEC2 probe after the attempt.
    pub sec2_after: Sec2Probe,
    /// FECS `CPUCTL` after the attempt.
    pub fecs_cpuctl_after: u32,
    /// FECS `MAILBOX0` after the attempt.
    pub fecs_mailbox0_after: u32,
    /// GPCCS `CPUCTL` after the attempt.
    pub gpccs_cpuctl_after: u32,
    /// FECS program counter after the attempt.
    pub fecs_pc_after: u32,
    /// FECS exception info after the attempt.
    pub fecs_exci_after: u32,
    /// GPCCS program counter after the attempt.
    pub gpccs_pc_after: u32,
    /// GPCCS exception info after the attempt.
    pub gpccs_exci_after: u32,
    /// Whether the chain reported success (FECS ready / expected mailbox).
    pub success: bool,
    /// Human-readable diagnostic lines from the attempt.
    pub notes: Vec<String>,
}

impl AcrBootResult {
    /// Serialize key fields to a JSON value matching coral-ember's
    /// `JournalEntry::BootAttempt` schema. The caller provides the BDF.
    pub fn to_journal_json(&self, bdf: &str) -> serde_json::Value {
        let timestamp_epoch_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        serde_json::json!({
            "kind": "BootAttempt",
            "bdf": bdf,
            "strategy": self.strategy,
            "success": self.success,
            "sec2_exci": self.sec2_after.exci,
            "fecs_pc": self.fecs_pc_after,
            "gpccs_exci": self.gpccs_exci_after,
            "notes": self.notes,
            "timestamp_epoch_ms": timestamp_epoch_ms,
        })
    }
}

impl fmt::Display for AcrBootResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "╔══ ACR Boot: {} ═══════════════════════════════════╗",
            self.strategy
        )?;
        writeln!(f, "  success: {}", self.success)?;
        writeln!(f, "  SEC2 before: {}", self.sec2_before)?;
        writeln!(f, "  SEC2 after:  {}", self.sec2_after)?;
        writeln!(
            f,
            "  FECS after:  cpuctl={:#010x} pc={:#06x} exci={:#010x} mb0={:#010x}",
            self.fecs_cpuctl_after,
            self.fecs_pc_after,
            self.fecs_exci_after,
            self.fecs_mailbox0_after
        )?;
        writeln!(
            f,
            "  GPCCS after: cpuctl={:#010x} pc={:#06x} exci={:#010x}",
            self.gpccs_cpuctl_after, self.gpccs_pc_after, self.gpccs_exci_after
        )?;
        for note in &self.notes {
            writeln!(f, "  note: {note}")?;
        }
        write!(
            f,
            "╚═══════════════════════════════════════════════════════╝"
        )
    }
}

/// Evaluate whether falcons are genuinely running (not just cpuctl==0).
///
/// Requires: FECS not in HRESET, GPCCS EXCI == 0, and GPCCS PC != 0.
pub(crate) fn evaluate_boot_success(fecs_cpuctl: u32, gpccs_pc: u32, gpccs_exci: u32) -> bool {
    let fecs_not_reset = fecs_cpuctl & falcon::CPUCTL_HALTED == 0;
    let gpccs_healthy = gpccs_exci == 0 && gpccs_pc != 0;
    fecs_not_reset && gpccs_healthy
}

/// Capture FECS/GPCCS PC+EXCI for post-boot result population.
pub(crate) struct PostBootCapture {
    pub fecs_cpuctl: u32,
    pub fecs_mailbox0: u32,
    pub fecs_pc: u32,
    pub fecs_exci: u32,
    pub gpccs_cpuctl: u32,
    pub gpccs_pc: u32,
    pub gpccs_exci: u32,
}

impl PostBootCapture {
    pub fn capture(bar0: &MappedBar) -> Self {
        let fecs_r = |off: usize| bar0.read_u32(falcon::FECS_BASE + off).unwrap_or(0xDEAD);
        let gpccs_r = |off: usize| bar0.read_u32(falcon::GPCCS_BASE + off).unwrap_or(0xDEAD);
        Self {
            fecs_cpuctl: fecs_r(falcon::CPUCTL),
            fecs_mailbox0: fecs_r(falcon::MAILBOX0),
            fecs_pc: fecs_r(falcon::PC),
            fecs_exci: fecs_r(falcon::EXCI),
            gpccs_cpuctl: gpccs_r(falcon::CPUCTL),
            gpccs_pc: gpccs_r(falcon::PC),
            gpccs_exci: gpccs_r(falcon::EXCI),
        }
    }

    pub fn success(&self) -> bool {
        evaluate_boot_success(self.fecs_cpuctl, self.gpccs_pc, self.gpccs_exci)
    }

    pub fn into_result(
        self,
        strategy: &'static str,
        sec2_before: Sec2Probe,
        sec2_after: Sec2Probe,
        notes: Vec<String>,
    ) -> AcrBootResult {
        let success = self.success();
        AcrBootResult {
            strategy,
            sec2_before,
            sec2_after,
            fecs_cpuctl_after: self.fecs_cpuctl,
            fecs_mailbox0_after: self.fecs_mailbox0,
            gpccs_cpuctl_after: self.gpccs_cpuctl,
            fecs_pc_after: self.fecs_pc,
            fecs_exci_after: self.fecs_exci,
            gpccs_pc_after: self.gpccs_pc,
            gpccs_exci_after: self.gpccs_exci,
            success,
            notes,
        }
    }
}

/// Poll a falcon's CPUCTL/MAILBOX for boot completion.
#[expect(
    dead_code,
    reason = "shared helper — strategies will adopt incrementally"
)]
///
/// Returns `(cpuctl, mailbox0)` when the falcon either signals ready (mailbox0 != 0),
/// halts (HALTED bit set), or times out. Appends poll notes to the provided vec.
pub(crate) fn poll_falcon_boot(
    bar0: &MappedBar,
    base: usize,
    name: &str,
    timeout_ms: u64,
    notes: &mut Vec<String>,
) -> (u32, u32) {
    let timeout = std::time::Duration::from_millis(timeout_ms);
    let start = std::time::Instant::now();

    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let cpuctl = bar0.read_u32(base + falcon::CPUCTL).unwrap_or(0xDEAD);
        let mb0 = bar0.read_u32(base + falcon::MAILBOX0).unwrap_or(0);

        let stopped = cpuctl & falcon::CPUCTL_STOPPED != 0;
        let fw_halted = cpuctl & falcon::CPUCTL_HALTED != 0;

        if mb0 != 0 {
            notes.push(format!(
                "{name}: mailbox0={mb0:#010x} cpuctl={cpuctl:#010x} ({}ms)",
                start.elapsed().as_millis()
            ));
            return (cpuctl, mb0);
        }

        if stopped && !fw_halted {
            notes.push(format!(
                "{name}: stopped without mailbox: cpuctl={cpuctl:#010x} ({}ms)",
                start.elapsed().as_millis()
            ));
            return (cpuctl, mb0);
        }

        if start.elapsed() > timeout {
            notes.push(format!(
                "{name}: poll timeout ({timeout_ms}ms): cpuctl={cpuctl:#010x} mb0={mb0:#010x}"
            ));
            return (cpuctl, mb0);
        }
    }
}

/// Dump non-zero ranges in a DMEM word buffer for diagnostics.
pub(crate) fn dmem_nonzero_summary(dmem: &[u32]) -> String {
    let mut ranges = Vec::new();
    let mut in_nonzero = false;
    let mut start = 0;
    for (i, &word) in dmem.iter().enumerate() {
        if word != 0 && word != 0xDEAD_DEAD {
            if !in_nonzero {
                start = i;
                in_nonzero = true;
            }
        } else if in_nonzero {
            ranges.push(format!("[{:#05x}..{:#05x}]", start * 4, i * 4));
            in_nonzero = false;
        }
    }
    if in_nonzero {
        ranges.push(format!("[{:#05x}..{:#05x}]", start * 4, dmem.len() * 4));
    }
    if ranges.is_empty() {
        "NONE".to_string()
    } else {
        ranges.join(", ")
    }
}

/// Dump first N words of DMEM as non-zero detail lines for diagnostics.
pub(crate) fn dmem_detail(dmem: &[u32], word_offset: usize, count: usize) -> Vec<String> {
    dmem.iter()
        .skip(word_offset)
        .take(count)
        .enumerate()
        .filter(|&(_, &word)| word != 0)
        .map(|(i, &word)| format!("[{:#05x}]={word:#010x}", (i + word_offset) * 4))
        .collect()
}

pub(crate) fn make_fail_result(
    strategy: &'static str,
    sec2_before: Sec2Probe,
    bar0: &MappedBar,
    notes: Vec<String>,
) -> AcrBootResult {
    let sec2_after = Sec2Probe::capture(bar0);
    let fecs_r = |off: usize| bar0.read_u32(falcon::FECS_BASE + off).unwrap_or(0xDEAD);
    let gpccs_r = |off: usize| bar0.read_u32(falcon::GPCCS_BASE + off).unwrap_or(0xDEAD);
    AcrBootResult {
        strategy,
        sec2_before,
        sec2_after,
        fecs_cpuctl_after: fecs_r(falcon::CPUCTL),
        fecs_mailbox0_after: fecs_r(falcon::MAILBOX0),
        gpccs_cpuctl_after: gpccs_r(falcon::CPUCTL),
        fecs_pc_after: fecs_r(falcon::PC),
        fecs_exci_after: fecs_r(falcon::EXCI),
        gpccs_pc_after: gpccs_r(falcon::PC),
        gpccs_exci_after: gpccs_r(falcon::EXCI),
        success: false,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_sec2_probe() -> Sec2Probe {
        Sec2Probe {
            cpuctl: 0,
            sctl: 0x3002,
            bootvec: 0x100,
            hwcfg: 0x1234,
            mailbox0: 0,
            mailbox1: 0,
            pc: 0x200,
            exci: 0,
            state: crate::nv::vfio_compute::acr_boot::sec2_hal::Sec2State::CleanReset,
        }
    }

    fn fake_result() -> AcrBootResult {
        AcrBootResult {
            strategy: "test_strategy",
            sec2_before: fake_sec2_probe(),
            sec2_after: fake_sec2_probe(),
            fecs_cpuctl_after: 0x40,
            fecs_mailbox0_after: 0,
            gpccs_cpuctl_after: 0,
            fecs_pc_after: 0x1000,
            fecs_exci_after: 0,
            gpccs_pc_after: 0x2000,
            gpccs_exci_after: 0x0207,
            success: false,
            notes: vec!["note one".into(), "note two".into()],
        }
    }

    #[test]
    fn to_journal_json_has_required_fields() {
        let r = fake_result();
        let j = r.to_journal_json("0000:3b:00.0");

        assert_eq!(j["kind"], "BootAttempt");
        assert_eq!(j["bdf"], "0000:3b:00.0");
        assert_eq!(j["strategy"], "test_strategy");
        assert_eq!(j["success"], false);
        assert_eq!(j["sec2_exci"], 0);
        assert_eq!(j["fecs_pc"], 0x1000);
        assert_eq!(j["gpccs_exci"], 0x0207);
        assert!(j["timestamp_epoch_ms"].as_u64().unwrap() > 0);
        assert_eq!(j["notes"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn to_journal_json_is_valid_json_string() {
        let r = fake_result();
        let j = r.to_journal_json("0000:00:00.0");
        let s = serde_json::to_string(&j).unwrap();
        let _: serde_json::Value = serde_json::from_str(&s).unwrap();
    }

    #[test]
    fn boot_journal_is_object_safe() {
        struct NoopJournal;
        impl BootJournal for NoopJournal {
            fn record_boot_attempt(&self, _: &AcrBootResult) {}
        }
        let _: &dyn BootJournal = &NoopJournal;
    }

    #[test]
    fn display_includes_strategy() {
        let r = fake_result();
        let s = format!("{r}");
        assert!(s.contains("test_strategy"));
        assert!(s.contains("note one"));
        assert!(s.contains("note two"));
    }

    #[test]
    fn evaluate_boot_success_happy() {
        assert!(evaluate_boot_success(0x00, 0x100, 0x00));
    }

    #[test]
    fn evaluate_boot_success_fecs_in_hreset_fails() {
        assert!(!evaluate_boot_success(falcon::CPUCTL_HALTED, 0x100, 0x00));
    }

    #[test]
    fn evaluate_boot_success_gpccs_exci_nonzero_fails() {
        assert!(!evaluate_boot_success(0x00, 0x100, 0x0207));
    }

    #[test]
    fn evaluate_boot_success_gpccs_pc_zero_fails() {
        assert!(!evaluate_boot_success(0x00, 0x00, 0x00));
    }

    #[test]
    fn dmem_nonzero_summary_empty_buffer() {
        assert_eq!(dmem_nonzero_summary(&[0; 16]), "NONE");
    }

    #[test]
    fn dmem_nonzero_summary_with_ranges() {
        let mut buf = vec![0u32; 32];
        buf[4] = 0x1234;
        buf[5] = 0x5678;
        buf[10] = 0xABCD;
        let s = dmem_nonzero_summary(&buf);
        assert!(s.contains("[0x010..0x018]"));
        assert!(s.contains("[0x028..0x02c]"));
    }
}
