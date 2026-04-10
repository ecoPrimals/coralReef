// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt::Write as FmtWrite;

/// Live hardware probe results from BAR0 reads.
#[derive(Debug, Clone)]
pub struct NvVoltaProbe {
    /// Current PMC_ENABLE register value.
    pub pmc_enable: u32,
    /// Per-domain active/gated state probed from PMC_ENABLE.
    pub domain_states: Vec<(String, bool)>,
    /// FALCON microcontroller states: (name, base, ctrl, halted).
    pub falcon_states: Vec<(String, usize, u32, bool)>,
    /// Temperature in celsius (if readable).
    pub temperature_c: Option<u32>,
    /// Fuse configuration registers.
    pub fuse_config: Vec<(String, u32)>,
    /// Number of active GPC partitions (from fuses).
    pub active_gpcs: u32,
    /// Number of active TPC partitions (from fuses).
    pub active_tpcs: u32,
    /// Number of active FBP partitions (from fuses).
    pub active_fbps: u32,
    /// FBPA partition liveness.
    pub fbpa_alive: Vec<(u32, bool)>,
    /// LTC partition liveness.
    pub ltc_alive: Vec<(u32, bool)>,
}

impl NvVoltaProbe {
    /// Print human-readable summary.
    pub fn print_summary(&self) {
        let mut s = String::new();
        writeln!(
            &mut s,
            "╠══ LIVE HARDWARE PROBE ═════════════════════════════════════╣"
        )
        .expect("writing to String is infallible");
        writeln!(&mut s, "║ PMC_ENABLE = {:#010x}", self.pmc_enable)
            .expect("writing to String is infallible");
        for (name, active) in &self.domain_states {
            writeln!(
                &mut s,
                "║   {name:<8} → {}",
                if *active { "ACTIVE" } else { "gated" }
            )
            .expect("writing to String is infallible");
        }
        if let Some(t) = self.temperature_c {
            writeln!(&mut s, "║ Temperature: ~{}°C", t).expect("writing to String is infallible");
        }
        writeln!(
            &mut s,
            "║ Active: {} GPCs, {} TPCs, {} FBPs",
            self.active_gpcs, self.active_tpcs, self.active_fbps
        )
        .expect("writing to String is infallible");
        for (idx, alive) in &self.fbpa_alive {
            writeln!(
                &mut s,
                "║   FBPA{idx}: {}",
                if *alive { "alive" } else { "dead" }
            )
            .expect("writing to String is infallible");
        }
        for (idx, alive) in &self.ltc_alive {
            writeln!(
                &mut s,
                "║   LTC{idx}: {}",
                if *alive { "alive" } else { "dead" }
            )
            .expect("writing to String is infallible");
        }
        for (name, base, ctrl, halted) in &self.falcon_states {
            writeln!(
                &mut s,
                "║   {name:6} @ {base:#08x}: CTRL={ctrl:#010x} {}",
                if *halted { "HALTED" } else { "running?" },
            )
            .expect("writing to String is infallible");
        }
        tracing::info!(summary = %s, "live hardware probe");
    }

    /// Export as JSON.
    pub fn to_json_value(&self) -> serde_json::Value {
        use serde_json::json;
        json!({
            "pmc_enable": format!("{:#010x}", self.pmc_enable),
            "domains": self.domain_states.iter().map(|(n, a)| json!({
                "name": n, "active": a,
            })).collect::<Vec<_>>(),
            "temperature_c": self.temperature_c,
            "active_gpcs": self.active_gpcs,
            "active_tpcs": self.active_tpcs,
            "active_fbps": self.active_fbps,
            "fbpa": self.fbpa_alive.iter().map(|(i, a)| json!({
                "index": i, "alive": a,
            })).collect::<Vec<_>>(),
            "ltc": self.ltc_alive.iter().map(|(i, a)| json!({
                "index": i, "alive": a,
            })).collect::<Vec<_>>(),
            "falcons": self.falcon_states.iter().map(|(n, b, c, h)| json!({
                "name": n, "base": format!("{b:#x}"), "ctrl": format!("{c:#010x}"), "halted": h,
            })).collect::<Vec<_>>(),
            "fuses": self.fuse_config.iter().map(|(n, v)| json!({
                "name": n, "value": format!("{v:#010x}"),
            })).collect::<Vec<_>>(),
        })
    }
}
