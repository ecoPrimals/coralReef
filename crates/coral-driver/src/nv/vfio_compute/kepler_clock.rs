// SPDX-License-Identifier: AGPL-3.0-or-later
//! GK110 (Tesla K80) clock PLL configuration from nvidia-470 cold→warm diff.
//!
//! On cold VFIO boot, VBIOS DEVINIT enables basic clock domains but does NOT
//! configure the full PLL tree. The GPC, HUB, and LTPC clocks remain unconfigured,
//! leaving GPCs PRI-faulted (`0xBADFxxxx` on any GPC register read).
//!
//! This module contains the exact register writes that nvidia-470 applies to
//! configure the PCLOCK domain (0x130xxx–0x137xxx) and PRI ring routing
//! (0x1007xx). These were captured via `coralctl mmio diff` between a cold-VFIO
//! GPU and a warm nvidia-470 GPU, then filtered to the clock-critical subset.

/// PCLOCK PLL + clock domain + PRI ring writes extracted from
/// `data/k80/nvidia470-captures/k80_clock_recipe.sh`.
///
/// Entries are `(BAR0_offset, value)`. The 0xBADFxxxx capture artifacts
/// from PRI-faulted reads in the original diff are excluded.
static GK110_CLOCK_RECIPE: &[(u32, u32)] = &[
    // ── PCLOCK PLLs (stride 0x200, 7 PLLs: CLK0–CLK5 + SCLK) ──
    (0x130000, 0x98010000),
    (0x130004, 0x00011001),
    (0x130014, 0x00000249),
    (0x130020, 0x20030001),
    (0x130024, 0x00032301),
    (0x130028, 0xf0000000),
    (0x13002c, 0x00000300),
    (0x130030, 0x10001007),
    (0x130034, 0x0a3e1000),
    (0x1300c4, 0x00140a05),
    (0x1300c8, 0x00000003),
    (0x1300cc, 0x0000021e),
    (0x1300e0, 0x0000007f),
    (0x130124, 0x00000001),
    (0x130200, 0x98010000),
    (0x130204, 0x00011001),
    (0x130214, 0x00000249),
    (0x130220, 0x20030001),
    (0x130224, 0x00032301),
    (0x130228, 0xf0000000),
    (0x13022c, 0x00000300),
    (0x130230, 0x10001007),
    (0x130234, 0x0a3e1000),
    (0x1302c4, 0x00140a05),
    (0x1302c8, 0x00000003),
    (0x1302cc, 0x0000021e),
    (0x1302e0, 0x0000007f),
    (0x130324, 0x00000001),
    (0x130400, 0x98010000),
    (0x130404, 0x00011001),
    (0x130414, 0x00000249),
    (0x130420, 0x20030001),
    (0x130424, 0x00032301),
    (0x130428, 0xf0000000),
    (0x13042c, 0x00000300),
    (0x130430, 0x10001007),
    (0x130434, 0x0a3e1000),
    (0x1304c4, 0x00140a05),
    (0x1304c8, 0x00000003),
    (0x1304cc, 0x0000021e),
    (0x1304e0, 0x0000007f),
    (0x130524, 0x00000001),
    (0x130600, 0x98010000),
    (0x130604, 0x00011001),
    (0x130614, 0x00000249),
    (0x130620, 0x20030001),
    (0x130624, 0x00032301),
    (0x130628, 0xf0000000),
    (0x13062c, 0x00000300),
    (0x130630, 0x10001007),
    (0x130634, 0x0a3e1000),
    (0x1306c4, 0x00140a05),
    (0x1306c8, 0x00000003),
    (0x1306cc, 0x0000021e),
    (0x1306e0, 0x0000007f),
    (0x130724, 0x00000001),
    (0x130800, 0x98010000),
    (0x130804, 0x00011001),
    (0x130814, 0x00000249),
    (0x130820, 0x20030001),
    (0x130824, 0x00032301),
    (0x130828, 0xf0000000),
    (0x13082c, 0x00000300),
    (0x130830, 0x10001007),
    (0x130834, 0x0a3e1000),
    (0x1308c4, 0x00140a05),
    (0x1308c8, 0x00000003),
    (0x1308cc, 0x0000021e),
    (0x1308e0, 0x0000007f),
    (0x130924, 0x00000001),
    (0x130a00, 0x98010000),
    (0x130a04, 0x00011001),
    (0x130a14, 0x00000249),
    (0x130a20, 0x20030001),
    (0x130a24, 0x00032301),
    (0x130a28, 0xf0000000),
    (0x130a2c, 0x00000300),
    (0x130a30, 0x10001007),
    (0x130a34, 0x0a3e1000),
    (0x130ac4, 0x00140a05),
    (0x130ac8, 0x00000003),
    (0x130acc, 0x0000021e),
    (0x130ae0, 0x0000007f),
    (0x130b24, 0x00000001),
    // ── PCLOCK PLLs (0x131xxx–0x132xxx: additional clock sources) ──
    (0x131a00, 0x98010000),
    (0x131a04, 0x00011001),
    (0x131a14, 0x00000249),
    (0x131a20, 0x20030001),
    (0x131a24, 0x00032301),
    (0x131a28, 0xf0000000),
    (0x131a2c, 0x00000300),
    (0x131a30, 0x10001007),
    (0x131a34, 0x0a3e1000),
    (0x131ac4, 0x00140a05),
    (0x131ac8, 0x00000003),
    (0x131acc, 0x0000021e),
    (0x131ae0, 0x0000007f),
    (0x131b24, 0x00000001),
    (0x131c00, 0x98010000),
    (0x131c04, 0x00011001),
    (0x131c14, 0x00000249),
    (0x131c20, 0x20030001),
    (0x131c24, 0x00032301),
    (0x131c28, 0xf0000000),
    (0x131c2c, 0x00000300),
    (0x131c30, 0x10001007),
    (0x131c34, 0x0a3e1000),
    (0x131cc4, 0x00140a05),
    (0x131cc8, 0x00000003),
    (0x131ccc, 0x0000021e),
    (0x131ce0, 0x0000007f),
    (0x131d24, 0x00000001),
    (0x131e00, 0x98010000),
    (0x131e04, 0x00011001),
    (0x131e14, 0x00000249),
    (0x131e20, 0x20030001),
    (0x131e24, 0x00032301),
    (0x131e28, 0xf0000000),
    (0x131e2c, 0x00000300),
    (0x131e30, 0x10001007),
    (0x131e34, 0x0a3e1000),
    (0x131ec4, 0x00140a05),
    (0x131ec8, 0x00000003),
    (0x131ecc, 0x0000021e),
    (0x131ee0, 0x0000007f),
    (0x131f24, 0x00000001),
    (0x132000, 0x98010000),
    (0x132004, 0x00011001),
    (0x132014, 0x00000249),
    (0x132020, 0x20030001),
    (0x132024, 0x00032301),
    (0x132028, 0xf0000000),
    (0x13202c, 0x00000300),
    (0x132030, 0x10001007),
    (0x132034, 0x0a3e1000),
    (0x1320c4, 0x00140a05),
    (0x1320c8, 0x00000003),
    (0x1320cc, 0x0000021e),
    (0x1320e0, 0x0000007f),
    (0x132124, 0x00000001),
    // ── Clock domain selectors (0x132800–0x134xxx) ──
    (0x132800, 0x00010000),
    (0x132804, 0x0001190a),
    (0x13280c, 0x02000000),
    (0x132818, 0x00030000),
    (0x13281c, 0x41001919),
    (0x132880, 0x00000009),
    (0x132888, 0x0000ff02),
    (0x1328a0, 0x01010019),
    (0x132900, 0x00000001),
    (0x132924, 0x01010000),
    (0x134000, 0x00010000),
    (0x134004, 0x0001190a),
    (0x13400c, 0x02000000),
    (0x134018, 0x00030000),
    (0x13401c, 0x41001919),
    (0x134080, 0x00000009),
    (0x134088, 0x0000ff02),
    (0x1340a0, 0x01010019),
    (0x134100, 0x00000001),
    (0x134124, 0x01010000),
    (0x134200, 0x00010000),
    (0x134204, 0x0001190a),
    (0x13420c, 0x02000000),
    (0x134218, 0x00030000),
    (0x13421c, 0x41001919),
    (0x134280, 0x00000009),
    (0x134288, 0x0000ff02),
    (0x1342a0, 0x01010019),
    (0x134300, 0x00000001),
    (0x134324, 0x00113fff),
    (0x134328, 0x00027aca),
    (0x134400, 0x00010000),
    (0x134404, 0x0001190a),
    (0x13440c, 0x02000000),
    (0x134418, 0x00030000),
    (0x13441c, 0x41001919),
    (0x134480, 0x00000009),
    (0x134488, 0x0000ff02),
    (0x1344a0, 0x01010019),
    (0x134500, 0x00000001),
    (0x134524, 0x00113fff),
    (0x134528, 0x0006d64b),
    (0x134600, 0x00010000),
    (0x134604, 0x0001190a),
    (0x13460c, 0x02000000),
    (0x134618, 0x00030000),
    (0x13461c, 0x41001919),
    (0x134680, 0x00000009),
    (0x134688, 0x0000ff02),
    (0x1346a0, 0x01010019),
    (0x134700, 0x00000001),
    (0x134724, 0x00113fff),
    (0x134728, 0x00068d6c),
    (0x134800, 0x00010000),
    (0x134804, 0x0001190a),
    (0x13480c, 0x02000000),
    (0x134818, 0x00030000),
    (0x13481c, 0x41001919),
    (0x134880, 0x00000009),
    (0x134888, 0x0000ff02),
    (0x1348a0, 0x01010019),
    (0x134900, 0x00000001),
    (0x134924, 0x00113fff),
    (0x134928, 0x0008abe5),
    // ── Master clock routing (0x137xxx) ──
    (0x137000, 0x00010000),
    (0x137004, 0x0001190a),
    (0x13700c, 0x02000000),
    (0x137018, 0x00030000),
    (0x13701c, 0x41001919),
    (0x137120, 0x00000003),
    (0x137124, 0x00000003),
    (0x137128, 0x00000003),
    (0x13713c, 0x00000003),
    (0x137160, 0x00000003),
    (0x137164, 0x00000003),
    (0x137168, 0x00000003),
    (0x13717c, 0x60000003),
    (0x137180, 0x00000003),
    (0x137190, 0x20000003),
    (0x137198, 0x00000003),
    (0x137300, 0x00000103),
    (0x137380, 0x00000002),
    (0x1373ec, 0x00010000),
    (0x1373f4, 0x00001111),
    (0x137404, 0x00000003),
    (0x137024, 0x00023012),
    (0x137044, 0x00023012),
    (0x1370a8, 0x00000001),
    (0x1370e4, 0x00012b1f),
    (0x137140, 0x81100606),
    (0x137144, 0x81100202),
    (0x137148, 0x81100202),
    (0x13715c, 0x81100202),
    (0x13718c, 0x00030000),
    (0x1371d0, 0x81100303),
    (0x1371d4, 0x81100303),
    (0x1371d8, 0x81100303),
    (0x1371ec, 0x81100000),
    (0x137250, 0x81100000),
    (0x137254, 0x81100000),
    (0x137258, 0x81100000),
    (0x13726c, 0x81100003),
    (0x137270, 0x81100303),
    (0x13727c, 0x81101c1c),
    (0x137280, 0x81100808),
    (0x137288, 0x81100606),
    (0x1372c8, 0x81100000),
    (0x1372cc, 0x81100000),
    (0x137310, 0x81101608),
    (0x137330, 0x81100034),
    (0x137340, 0x00000001),
    (0x137360, 0x00000001),
    (0x137390, 0x00030001),
    (0x137450, 0x000001b0),
    (0x137470, 0x10300000),
    (0x137474, 0x10100008),
    (0x137478, 0x00080804),
    (0x137d90, 0x00030001),
    // ── PMC engine enable (nvidia-470 specific mask) ──
    (0x000200, 0xe011312c),
    // ── PMC secondary control ──
    (0x000640, 0xfebfb1e1),
    // ── PRI ring master control ──
    (0x100800, 0x00000006),
    // ── PRI ring hub routing ──
    (0x100700, 0x8d0a00a5),
    (0x100708, 0x001f1f1f),
    (0x10070c, 0x78000180),
    (0x100710, 0x800c120e),
    (0x100714, 0x00000304),
    (0x100718, 0x04040004),
    (0x10071c, 0x00000404),
    (0x100720, 0x04040004),
    (0x100724, 0x00000404),
    (0x100728, 0x00000404),
];

/// Access the raw recipe entries for split-phase application.
pub(crate) fn gk110_clock_recipe_entries() -> &'static [(u32, u32)] {
    GK110_CLOCK_RECIPE
}

/// Apply the full GK110 clock PLL + routing recipe to a cold-VFIO K80.
///
/// Returns `(applied, skipped)` counts.
///
/// SAFETY NOTES: This function writes to PCLOCK PLL registers (0x130000+)
/// which are in the `GuardedBar` caution range. These writes are harmless
/// on cold K80 (silently dropped due to power-gating) and necessary on
/// warm K80 (post-nouveau POST). A BOOT0 canary check is performed
/// before and after the recipe to detect link-down.
pub(crate) fn apply_gk110_clock_recipe(
    guard: &super::hardware_guard::GuardedBar<'_>,
) -> (u32, u32) {
    let boot0_pre = guard.read_u32(0).unwrap_or(0xFFFF_FFFF);
    if boot0_pre == 0xFFFF_FFFF || boot0_pre == 0 {
        tracing::error!(
            boot0 = format_args!("{boot0_pre:#010x}"),
            "clock recipe ABORTED: GPU dead before clock writes"
        );
        return (0, 0);
    }

    let r = guard.read_fn();
    let w = |reg: u32, val: u32| {
        if let Err(refusal) = guard.write_u32(reg, val) {
            tracing::error!(%refusal, "clock recipe: hardware guard refused write");
        }
    };
    let is_fault = |v: u32| v & 0xBAD0_0000 == 0xBAD0_0000 || v == 0xDEAD_DEAD;

    // First: try proper PLL enable sequence on PLL0 to test if PLLs accept writes
    {
        let pll_ctrl = 0x13_0000u32;
        let pll_coef = 0x13_0004u32;
        let pll_stat = 0x13_0014u32;

        let ctrl_pre = r(pll_ctrl);
        tracing::info!(
            ctrl = format_args!("{ctrl_pre:#010x}"),
            fault = is_fault(ctrl_pre),
            "PLL0 pre-test state"
        );

        // Step 1: Clear bypass + enable (nouveau sequence for PLL_CORE)
        w(pll_ctrl, 0x0000_0000);
        // Step 2: Write coefficients
        w(pll_coef, 0x0001_1001);
        // Step 3: Enable (bit 0)
        w(pll_ctrl, 0x0000_0001);
        std::thread::sleep(std::time::Duration::from_millis(20));
        // Step 4: Set bypass (bit 2)
        w(pll_ctrl, 0x0000_0005);
        std::thread::sleep(std::time::Duration::from_millis(20));

        let ctrl_post = r(pll_ctrl);
        let coef_post = r(pll_coef);
        let stat_post = r(pll_stat);
        tracing::info!(
            ctrl = format_args!("{ctrl_post:#010x}"),
            coef = format_args!("{coef_post:#010x}"),
            stat = format_args!("{stat_post:#010x}"),
            "PLL0 after enable sequence"
        );

        // Also test: can we write ANY value to a register in 0x131xxx range?
        // (different PCLOCK sub-block)
        let test_reg = 0x13_4000u32;
        let pre = r(test_reg);
        w(test_reg, 0xDEAD_BEEF);
        let post = r(test_reg);
        tracing::info!(
            pre = format_args!("{pre:#010x}"),
            post = format_args!("{post:#010x}"),
            "PCLOCK 0x134000 write test"
        );
    }

    let mut applied = 0u32;
    let mut skipped = 0u32;
    let mut mismatched = 0u32;

    for &(reg, val) in GK110_CLOCK_RECIPE {
        if guard.write_u32(reg, val).is_ok() {
            applied += 1;
        } else {
            skipped += 1;
        }
    }

    // Verify first 5 entries
    for &(reg, expected) in GK110_CLOCK_RECIPE.iter().take(5) {
        let readback = guard.read_u32(reg).unwrap_or(0xDEAD_DEAD);
        if readback != expected {
            mismatched += 1;
        }
    }

    tracing::info!(
        applied,
        skipped,
        mismatched,
        total = GK110_CLOCK_RECIPE.len(),
        "GK110 clock recipe applied"
    );

    let boot0_post = guard.read_u32(0).unwrap_or(0xFFFF_FFFF);
    if boot0_post == 0xFFFF_FFFF || boot0_post == 0 {
        tracing::error!(
            boot0 = format_args!("{boot0_post:#010x}"),
            applied,
            "HARDWARE GUARD: GPU died during clock recipe!"
        );
    } else if boot0_post != boot0_pre {
        tracing::error!(
            before = format_args!("{boot0_pre:#010x}"),
            after = format_args!("{boot0_post:#010x}"),
            "HARDWARE GUARD: BOOT0 changed during clock recipe — GPU state corrupted"
        );
    }

    (applied, skipped)
}
