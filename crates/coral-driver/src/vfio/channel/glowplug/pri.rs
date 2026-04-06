// SPDX-License-Identifier: AGPL-3.0-or-later
//! PRI bus health monitoring and recovery.

use super::super::pri_monitor::{self, PriBusMonitor};
use super::GlowPlug;

impl GlowPlug<'_> {
    /// PRI backpressure check — probe all HBM2-critical domains and report health.
    ///
    /// Returns (alive_count, faulted_count, log_messages).
    /// If the bus is faulted, attempts recovery before returning.
    pub fn check_pri_health(&self) -> (usize, usize, Vec<String>) {
        let mut monitor = PriBusMonitor::new(self.bar0);
        let health = monitor.probe_all_domains();
        let mut log = Vec::new();

        let alive = health
            .iter()
            .filter(|(_, _, h)| matches!(h, pri_monitor::DomainHealth::Alive))
            .count();
        let faulted = health
            .iter()
            .filter(|(_, _, h)| matches!(h, pri_monitor::DomainHealth::Faulted { .. }))
            .count();

        for (name, off, h) in &health {
            match h {
                pri_monitor::DomainHealth::Alive => {
                    log.push(format!("  PRI {name} ({off:#08x}): ALIVE"));
                }
                pri_monitor::DomainHealth::Faulted {
                    fault_count,
                    last_error,
                } => {
                    log.push(format!(
                        "  PRI {name} ({off:#08x}): FAULTED ({fault_count}x, last={last_error:#010x})"
                    ));
                }
                pri_monitor::DomainHealth::Skipped => {
                    log.push(format!("  PRI {name} ({off:#08x}): SKIPPED"));
                }
            }
        }

        if faulted > 0 {
            log.push(format!(
                "  PRI bus: {alive} alive, {faulted} faulted — attempting recovery..."
            ));
            let recovered = monitor.attempt_recovery();
            if recovered {
                log.push("  PRI bus: recovery successful (BOOT0 reads clean)".into());
            } else {
                log.push("  PRI bus: recovery failed — bus may be locked".into());
            }
        } else {
            log.push(format!("  PRI bus: all {alive} probed domains alive"));
        }

        (alive, faulted, log)
    }

    /// Attempt PRI bus recovery without full health probe.
    pub(crate) fn recover_pri_bus(&self) -> bool {
        let mut monitor = PriBusMonitor::new(self.bar0);
        monitor.attempt_recovery()
    }
}
