// SPDX-License-Identifier: AGPL-3.0-only

use crate::vfio::channel::registers::{falcon, misc};
use crate::vfio::device::MappedBar;

use super::super::boot_result::AcrBootResult;
use super::super::sec2_hal::{Sec2Probe, sec2_emem_read, sec2_emem_write};

/// Attempt 081a: Direct HRESET release experiments.
///
/// Tries several low-cost approaches before committing to the full ACR chain:
/// 1. Direct write to FECS CPUCTL to clear HRESET bit
/// 2. PMC GR engine reset toggle
/// 3. SEC2 EMEM probe (verify accessibility)
pub fn attempt_direct_hreset(bar0: &MappedBar) -> AcrBootResult {
    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);
    notes.push(format!("SEC2 initial state: {:?}", sec2_before.state));

    let fecs_r = |off: usize| bar0.read_u32(falcon::FECS_BASE + off).unwrap_or(0xDEAD);

    // Experiment 1: Try direct CPUCTL write to clear HRESET
    let fecs_cpuctl_before = fecs_r(falcon::CPUCTL);
    notes.push(format!("FECS cpuctl before: {fecs_cpuctl_before:#010x}"));

    if fecs_cpuctl_before & falcon::CPUCTL_HALTED != 0 {
        // Try writing 0 to CPUCTL (clear all bits including HRESET)
        let _ = bar0.write_u32(falcon::FECS_BASE + falcon::CPUCTL, 0);
        std::thread::sleep(std::time::Duration::from_millis(5));
        let after = fecs_r(falcon::CPUCTL);
        notes.push(format!("FECS cpuctl after direct clear: {after:#010x}"));

        if after & falcon::CPUCTL_HALTED == 0 {
            notes.push("Direct HRESET clear SUCCEEDED".to_string());
        } else {
            notes.push("Direct HRESET clear failed (expected — ACR-managed)".to_string());
        }
    }

    let pmc = bar0.read_u32(misc::PMC_ENABLE).unwrap_or(0);
    let gr_bit: u32 = 1 << 12;
    notes.push(format!("PMC before GR toggle: {pmc:#010x}"));

    let _ = bar0.write_u32(misc::PMC_ENABLE, pmc & !gr_bit);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let _ = bar0.write_u32(misc::PMC_ENABLE, pmc | gr_bit);
    std::thread::sleep(std::time::Duration::from_millis(10));

    let fecs_after_pmc = fecs_r(falcon::CPUCTL);
    notes.push(format!(
        "FECS cpuctl after PMC GR toggle: {fecs_after_pmc:#010x}"
    ));

    // Experiment 3: SEC2 EMEM accessibility test
    let test_pattern: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];
    sec2_emem_write(bar0, 0, &test_pattern);
    let readback = sec2_emem_read(bar0, 0, 4);
    let expected_word: u32 = 0xEFBE_ADDE;
    let emem_ok = readback.first().copied() == Some(expected_word);
    notes.push(format!(
        "SEC2 EMEM write/read: wrote={:#010x} read={:#010x} match={}",
        expected_word,
        readback.first().copied().unwrap_or(0),
        emem_ok
    ));

    // ── SEC2 Conversation probe ──
    super::super::sec2_queue::probe_and_bootstrap(bar0, &mut notes);

    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::super::boot_result::PostBootCapture::capture(bar0);

    post.into_result(
        "081a: direct HRESET experiments",
        sec2_before,
        sec2_after,
        notes,
    )
}
