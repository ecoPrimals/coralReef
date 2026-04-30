// SPDX-License-Identifier: AGPL-3.0-or-later

//! Nouveau-style GK104/GK110 engine clock programming via 0x137xxx registers.
//!
//! The nvidia-470 proprietary driver uses 0x130xxx PCLOCK PLLs, which are in a
//! power-gated domain that requires the PMU to enable. Nouveau's `gk104_clk`
//! uses a completely different clock tree at 0x137xxx that routes through
//! crystal dividers and optional PLLs at `0x137000 + idx * 0x20`.
//!
//! Clock tree (Nouveau gk104_clk):
//! ```text
//! Crystal(27MHz) ─┬─ divider_src[idx] (0x137160+idx*4) ─┬─ divider_ctl[idx] (0x1371d0+idx*4)
//!                 │                                       └─ → engine (when SSEL[idx]=0)
//!                 └─ ref_div_src[idx] (0x137120+idx*4) ── ref_div_ctl[idx] (0x137140+idx*4)
//!                                                          └─ PLL[idx] (0x137000+idx*0x20)
//!                                                               └─ → engine (when SSEL[idx]=1)
//! Output stage: engine → output_div[idx] (0x137250+idx*4) → final clock
//! ```
//!
//! Engine indices (gk104_clk):
//!   0 = GPC, 1 = ROP (hubk07 in nouveau), 2 = HUB (rop in nouveau),
//!   7 = hubk06, 8 = hubk01, 0x0c = PMU, 0x0e = VDEC

/// Full diagnostic of the Nouveau-style clock tree.
pub(super) fn nouveau_clock_diagnostic(r: &dyn Fn(u32) -> u32) {
    // Source select: bit[idx] = 1 means PLL, 0 means divider
    let ssel = r(0x13_7100);

    // Per-engine diagnostics for the 3 critical engines
    for &(idx, name) in &[(0u32, "GPC"), (1, "ROP"), (2, "HUB")] {
        let pll_ctrl = r(0x13_7000 + idx * 0x20);
        let pll_coef = r(0x13_7000 + idx * 0x20 + 0x04);
        let div_src = r(0x13_7160 + idx * 4);
        let div_ctl = r(0x13_71D0 + idx * 4);
        let ref_src = r(0x13_7120 + idx * 4);
        let ref_ctl = r(0x13_7140 + idx * 4);
        let out_div = r(0x13_7250 + idx * 4);
        let using_pll = ssel & (1 << idx) != 0;

        tracing::info!(
            engine = name,
            idx,
            using_pll,
            pll_ctrl = format_args!("{pll_ctrl:#010x}"),
            pll_coef = format_args!("{pll_coef:#010x}"),
            div_src = format_args!("{div_src:#010x}"),
            div_ctl = format_args!("{div_ctl:#010x}"),
            ref_src = format_args!("{ref_src:#010x}"),
            ref_ctl = format_args!("{ref_ctl:#010x}"),
            out_div = format_args!("{out_div:#010x}"),
            "Nouveau CLK tree (0x137xxx)"
        );
    }

    // Additional indices: hubk06(7), hubk01(8), PMU(0x0c)
    for &(idx, name) in &[(7u32, "hubk06"), (8, "hubk01"), (0x0c, "PMU_CLK")] {
        let div_src = r(0x13_7160 + idx * 4);
        let div_ctl = r(0x13_71D0 + idx * 4);
        let out_div = r(0x13_7250 + idx * 4);
        tracing::info!(
            engine = name,
            idx,
            div_src = format_args!("{div_src:#010x}"),
            div_ctl = format_args!("{div_ctl:#010x}"),
            out_div = format_args!("{out_div:#010x}"),
            "Nouveau CLK tree extended"
        );
    }

    // Memory clock source
    let mem_sel = r(0x13_73F4);
    tracing::info!(
        ssel = format_args!("{ssel:#010x}"),
        mem_sel = format_args!("{mem_sel:#010x}"),
        "Nouveau CLK source selectors"
    );
}

/// Test writability of key 0x137xxx registers.
/// Returns (pll_ctrl_writable, div_src_writable, out_div_writable).
pub(super) fn test_137xxx_writability(
    r: &dyn Fn(u32) -> u32,
    w: &dyn Fn(u32, u32),
) -> (bool, bool, bool) {
    // Test PLL CTRL at 0x137000 (GPC PLL control)
    let pll_orig = r(0x13_7000);
    w(0x13_7000, pll_orig ^ 0x0000_0001);
    let pll_test = r(0x13_7000);
    w(0x13_7000, pll_orig);
    let pll_writable = pll_test == (pll_orig ^ 0x0000_0001);

    // Test divider source at 0x137160 (GPC engine clock source)
    let div_orig = r(0x13_7160);
    w(0x13_7160, div_orig ^ 0x0000_0002);
    let div_test = r(0x13_7160);
    w(0x13_7160, div_orig);
    let div_writable = div_test == (div_orig ^ 0x0000_0002);

    // Test output divider at 0x137250 (GPC output divider)
    let out_orig = r(0x13_7250);
    w(0x13_7250, out_orig ^ 0x0000_0001);
    let out_test = r(0x13_7250);
    w(0x13_7250, out_orig);
    let out_writable = out_test == (out_orig ^ 0x0000_0001);

    tracing::info!(
        pll_ctrl_writable = pll_writable,
        div_src_writable = div_writable,
        out_div_writable = out_writable,
        pll_test = format_args!("{pll_test:#010x}"),
        div_test = format_args!("{div_test:#010x}"),
        out_test = format_args!("{out_test:#010x}"),
        "0x137xxx writability test"
    );

    (pll_writable, div_writable, out_writable)
}

/// Program basic crystal-divider clocks for all engine domains.
///
/// Sets up the minimum viable clock tree: crystal (27 MHz) through
/// divider path (bypass PLL). This provides a low but functional clock
/// for FECS falcon boot.
pub(super) fn program_crystal_clocks(
    r: &dyn Fn(u32) -> u32,
    w: &dyn Fn(u32, u32),
) {
    // Disable PLL mode for all standard engines (0-6)
    // SSEL = 0 means all engines use divider path
    let ssel = r(0x13_7100);
    if ssel & 0x7F != 0 {
        w(0x13_7100, ssel & !0x7F);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    // Configure divider sources for engines 0-2 (GPC, ROP, HUB)
    // ssrc = 0x00000000: bits[1:0]=0 → crystal source, bits[17:16]=0 → 27 MHz
    // ssrc = 0x00030000: bits[1:0]=0, bits[17:16]=3 → 108 MHz
    //
    // Use 108 MHz for faster FECS boot
    for idx in 0..3u32 {
        w(0x13_7160 + idx * 4, 0x0003_0000); // 108 MHz from crystal
        w(0x13_71D0 + idx * 4, 0x0000_0000); // no divider (pass-through)
    }

    // hubk06(7), hubk01(8), PMU(0x0c) — crystal sources
    for &idx in &[7u32, 8, 0x0c] {
        w(0x13_7160 + idx * 4, 0x0003_0000); // 108 MHz
        w(0x13_71D0 + idx * 4, 0x0000_0000);
    }

    // Clear output dividers (pass-through mode)
    for idx in 0..3u32 {
        let cur = r(0x13_7250 + idx * 4);
        if cur & 0x8000_0000 != 0 {
            w(0x13_7250 + idx * 4, cur & !0x8000_0000);
        }
    }

    std::thread::sleep(std::time::Duration::from_millis(10));

    tracing::info!(
        ssel = format_args!("{:#010x}", r(0x13_7100)),
        gpc_src = format_args!("{:#010x}", r(0x13_7160)),
        rop_src = format_args!("{:#010x}", r(0x13_7164)),
        hub_src = format_args!("{:#010x}", r(0x13_7168)),
        "Crystal clocks programmed (108 MHz target)"
    );
}

/// Program engine PLLs using Nouveau gk104_clk_prog_2 sequence.
///
/// For each engine domain (GPC=0, ROP=1, HUB=2), programs the PLL at
/// `0x137000 + idx * 0x20` with the given coefficients and waits for lock.
///
/// PLL coefficient format: `P[21:16] | N[15:8] | M[7:0]`
/// Freq = ref_clk * N / (M * P)
///
/// With crystal ref (27 MHz):
///   coef = 0x0001_0F01 → 27 * 15 / (1*1) = 405 MHz
///   coef = 0x0001_1E01 → 27 * 30 / (1*1) = 810 MHz
pub(super) fn program_engine_plls(
    r: &dyn Fn(u32) -> u32,
    w: &dyn Fn(u32, u32),
) {
    // First ensure PLL reference dividers provide crystal input
    for idx in 0..3u32 {
        w(0x13_7120 + idx * 4, 0x0000_0003); // VCO source
        w(0x13_7140 + idx * 4, 0x0000_0000); // no divider
    }
    std::thread::sleep(std::time::Duration::from_millis(5));

    // Target: 405 MHz for GPC, ROP, HUB (conservative initial clock)
    // Crystal = 27 MHz, N=15, M=1, P=1 → 27*15/1 = 405 MHz
    let coef: u32 = (1 << 16) | (15 << 8) | 1; // 0x00010F01

    let mut any_locked = false;

    for &(idx, name) in &[(0u32, "GPC"), (1, "ROP"), (2, "HUB")] {
        let addr = 0x13_7000 + idx * 0x20;

        // Nouveau gk104_clk_prog_2 sequence:
        // 1. Clear bypass (bit 2) and disable (bit 0)
        let cur = r(addr);
        w(addr, cur & !0x0000_0005); // clear bits 0,2
        std::thread::sleep(std::time::Duration::from_millis(1));

        // 2. Write coefficients
        w(addr + 0x04, coef);

        // 3. Enable PLL (bit 0)
        w(addr, 0x0000_0001);

        // 4. Wait for PLL lock (bit 17)
        let cur2 = r(addr);
        w(addr, cur2 & !0x0000_0010); // clear test bit (bit 4)

        let mut locked = false;
        for _ in 0..200 {
            std::thread::sleep(std::time::Duration::from_millis(1));
            let ctrl = r(addr);
            if ctrl & 0x0002_0000 != 0 {
                locked = true;
                break;
            }
        }

        if locked {
            // 5. Set test bit + enable sync mode (bits 4, 2)
            let ctrl = r(addr);
            w(addr, ctrl | 0x0000_0010);
            let ctrl2 = r(addr);
            w(addr, ctrl2 | 0x0000_0004);
            any_locked = true;
        }

        let final_ctrl = r(addr);
        let final_coef = r(addr + 0x04);
        tracing::info!(
            engine = name,
            locked,
            ctrl = format_args!("{final_ctrl:#010x}"),
            coef = format_args!("{final_coef:#010x}"),
            target_mhz = 405,
            "PLL programming (gk104_clk_prog_2)"
        );
    }

    if any_locked {
        // Switch engines from divider to PLL mode
        let ssel = r(0x13_7100);
        w(0x13_7100, ssel | 0x0000_0007); // enable PLL for engines 0,1,2
        std::thread::sleep(std::time::Duration::from_millis(5));

        // Verify SSEL took effect
        let ssel_after = r(0x13_7100);
        tracing::info!(
            ssel_before = format_args!("{ssel:#010x}"),
            ssel_after = format_args!("{ssel_after:#010x}"),
            "Switched to PLL mode for GPC/ROP/HUB"
        );
    } else {
        tracing::warn!("No PLLs locked — engines remain on crystal divider path");
    }
}
