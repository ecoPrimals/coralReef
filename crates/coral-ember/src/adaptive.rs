// SPDX-License-Identifier: AGPL-3.0-only
//! Adaptive vendor lifecycle — learns from the experiment journal.
//!
//! Wraps any [`VendorLifecycle`] and overrides `settle_secs` and
//! `available_reset_methods` based on historical success data from the
//! journal. All other lifecycle methods delegate directly to the inner
//! implementation.
//!
//! # Safety bounds
//!
//! - Settle times can increase up to 2x the static default but never
//!   decrease below 50% of it.
//! - Reset method reordering requires at least 3 data points before
//!   overriding the static order.

use std::sync::Arc;

use crate::journal::Journal;
use crate::vendor_lifecycle::{RebindStrategy, ResetMethod, VendorLifecycle};

/// Minimum number of journal entries needed before adaptive overrides apply.
const MIN_DATA_POINTS: u64 = 3;

/// Wraps a [`VendorLifecycle`] with journal-informed adaptive behavior.
pub struct AdaptiveLifecycle {
    inner: Box<dyn VendorLifecycle>,
    journal: Arc<Journal>,
    bdf: String,
}

impl std::fmt::Debug for AdaptiveLifecycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdaptiveLifecycle")
            .field("inner", &self.inner)
            .field("bdf", &self.bdf)
            .field("journal_path", &self.journal.path())
            .finish()
    }
}

impl AdaptiveLifecycle {
    /// Create an adaptive wrapper around a static lifecycle.
    pub fn new(inner: Box<dyn VendorLifecycle>, journal: Arc<Journal>, bdf: String) -> Self {
        Self {
            inner,
            journal,
            bdf,
        }
    }

    fn historical_bind_ms(&self, target_driver: &str) -> Option<u64> {
        let filter = crate::journal::JournalFilter {
            bdf: Some(self.bdf.clone()),
            kind: Some("Swap".to_string()),
            personality: Some(target_driver.to_string()),
            limit: Some(10),
            ..Default::default()
        };

        let entries = self.journal.query(&filter).ok()?;
        if (entries.len() as u64) < MIN_DATA_POINTS {
            return None;
        }

        let total_bind_ms: u64 = entries
            .iter()
            .filter_map(|e| {
                if let crate::journal::JournalEntry::Swap(obs) = e {
                    Some(obs.timing.bind_ms)
                } else {
                    None
                }
            })
            .sum();

        let count = entries.len() as u64;
        if count > 0 {
            Some(total_bind_ms / count)
        } else {
            None
        }
    }

    fn historical_reset_success_rates(&self) -> Vec<(ResetMethod, f64, u64)> {
        let filter = crate::journal::JournalFilter {
            bdf: Some(self.bdf.clone()),
            kind: Some("Reset".to_string()),
            limit: Some(50),
            ..Default::default()
        };

        let entries = match self.journal.query(&filter) {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };

        let mut method_stats: std::collections::HashMap<String, (u64, u64)> =
            std::collections::HashMap::new();

        for entry in &entries {
            if let crate::journal::JournalEntry::Reset(obs) = entry {
                let acc = method_stats.entry(obs.method.clone()).or_insert((0, 0));
                acc.0 += 1;
                if obs.success {
                    acc.1 += 1;
                }
            }
        }

        let mut results = Vec::new();
        for (method_name, (attempts, successes)) in &method_stats {
            let reset_method = match method_name.as_str() {
                "bridge-sbr" => ResetMethod::BridgeSbr,
                "sbr" => ResetMethod::SysfsSbr,
                "remove-rescan" => ResetMethod::RemoveRescan,
                "flr" => ResetMethod::VfioFlr,
                _ => continue,
            };
            let rate = if *attempts > 0 {
                *successes as f64 / *attempts as f64
            } else {
                0.0
            };
            results.push((reset_method, rate, *attempts));
        }

        results
    }
}

impl VendorLifecycle for AdaptiveLifecycle {
    fn description(&self) -> &str {
        self.inner.description()
    }

    fn prepare_for_unbind(&self, bdf: &str, current_driver: &str) -> Result<(), String> {
        self.inner.prepare_for_unbind(bdf, current_driver)
    }

    fn rebind_strategy(&self, target_driver: &str) -> RebindStrategy {
        self.inner.rebind_strategy(target_driver)
    }

    fn settle_secs(&self, target_driver: &str) -> u64 {
        let static_settle = self.inner.settle_secs(target_driver);

        if let Some(avg_bind_ms) = self.historical_bind_ms(target_driver) {
            let avg_bind_secs = (avg_bind_ms / 1000) + 1;
            let adaptive_settle = avg_bind_secs + (avg_bind_secs / 5); // +20% buffer

            let floor = static_settle / 2;
            let ceiling = static_settle * 2;
            let clamped = adaptive_settle.clamp(floor, ceiling);

            if clamped != static_settle {
                tracing::info!(
                    bdf = %self.bdf,
                    target_driver,
                    static_settle,
                    adaptive_settle = clamped,
                    avg_bind_ms,
                    "adaptive lifecycle: adjusted settle time from journal"
                );
            }

            clamped
        } else {
            static_settle
        }
    }

    fn stabilize_after_bind(&self, bdf: &str, target_driver: &str) {
        self.inner.stabilize_after_bind(bdf, target_driver);
    }

    fn verify_health(&self, bdf: &str, target_driver: &str) -> Result<(), String> {
        self.inner.verify_health(bdf, target_driver)
    }

    fn available_reset_methods(&self) -> Vec<ResetMethod> {
        let static_methods = self.inner.available_reset_methods();
        let historical = self.historical_reset_success_rates();

        let has_enough_data = historical
            .iter()
            .any(|(_, _, attempts)| *attempts >= MIN_DATA_POINTS);
        if !has_enough_data {
            return static_methods;
        }

        let mut scored: Vec<(ResetMethod, f64)> = static_methods
            .iter()
            .map(|m| {
                let rate = historical
                    .iter()
                    .find(|(hm, _, _)| hm == m)
                    .map(|(_, rate, _)| *rate)
                    .unwrap_or(0.5); // unknown methods get neutral score
                (*m, rate)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let reordered: Vec<ResetMethod> = scored.into_iter().map(|(m, _)| m).collect();

        if reordered != static_methods {
            tracing::info!(
                bdf = %self.bdf,
                "adaptive lifecycle: reordered reset methods based on journal success rates"
            );
        }

        reordered
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::{Journal, JournalEntry};
    use crate::observation::{HealthResult, ResetObservation, SwapObservation, SwapTiming};

    #[derive(Debug)]
    struct StubLifecycle;

    impl VendorLifecycle for StubLifecycle {
        fn description(&self) -> &str {
            "Stub"
        }
        fn prepare_for_unbind(&self, _bdf: &str, _driver: &str) -> Result<(), String> {
            Ok(())
        }
        fn rebind_strategy(&self, _target: &str) -> RebindStrategy {
            RebindStrategy::SimpleBind
        }
        fn settle_secs(&self, _target: &str) -> u64 {
            10
        }
        fn stabilize_after_bind(&self, _bdf: &str, _target: &str) {}
        fn verify_health(&self, _bdf: &str, _target: &str) -> Result<(), String> {
            Ok(())
        }
        fn available_reset_methods(&self) -> Vec<ResetMethod> {
            vec![ResetMethod::BridgeSbr, ResetMethod::SysfsSbr]
        }
    }

    fn test_journal() -> (tempfile::TempDir, Arc<Journal>) {
        let dir = tempfile::tempdir().expect("tempdir");
        let journal = Arc::new(Journal::open(dir.path().join("test.jsonl")));
        (dir, journal)
    }

    fn swap_entry(bdf: &str, personality: &str, bind_ms: u64) -> JournalEntry {
        JournalEntry::Swap(SwapObservation {
            bdf: bdf.to_string(),
            from_personality: Some("vfio".to_string()),
            to_personality: personality.to_string(),
            timestamp_epoch_ms: 1700000000000,
            timing: SwapTiming {
                prepare_ms: 10,
                unbind_ms: 100,
                bind_ms,
                stabilize_ms: 50,
                total_ms: bind_ms + 160,
            },
            trace_path: None,
            health: HealthResult::Ok,
            lifecycle_description: "test".to_string(),
            reset_method_used: None,
            firmware_pre: None,
            firmware_post: None,
        })
    }

    fn reset_entry(bdf: &str, method: &str, success: bool) -> JournalEntry {
        JournalEntry::Reset(ResetObservation {
            bdf: bdf.to_string(),
            method: method.to_string(),
            success,
            error: None,
            timestamp_epoch_ms: 1700000000000,
            duration_ms: 500,
        })
    }

    #[test]
    fn settle_secs_uses_static_when_no_data() {
        let (_dir, journal) = test_journal();
        let adaptive =
            AdaptiveLifecycle::new(Box::new(StubLifecycle), journal, "0000:03:00.0".into());
        assert_eq!(adaptive.settle_secs("nouveau"), 10);
    }

    #[test]
    fn settle_secs_adapts_with_enough_data() {
        let (_dir, journal) = test_journal();
        let bdf = "0000:03:00.0";

        // 5 swaps with 15s bind time → adaptive should increase settle
        for _ in 0..5 {
            journal.append(&swap_entry(bdf, "nouveau", 15000)).unwrap();
        }

        let adaptive = AdaptiveLifecycle::new(Box::new(StubLifecycle), journal, bdf.to_string());
        let settle = adaptive.settle_secs("nouveau");
        // avg_bind = 15000ms → 15s + 20% = 18s, clamped to ceiling of 20
        assert!(settle > 10, "expected adaptive settle > 10, got {settle}");
        assert!(settle <= 20, "expected adaptive settle <= 20, got {settle}");
    }

    #[test]
    fn settle_secs_respects_floor() {
        let (_dir, journal) = test_journal();
        let bdf = "0000:03:00.0";

        // 5 swaps with very fast 1s bind time
        for _ in 0..5 {
            journal.append(&swap_entry(bdf, "nouveau", 1000)).unwrap();
        }

        let adaptive = AdaptiveLifecycle::new(Box::new(StubLifecycle), journal, bdf.to_string());
        let settle = adaptive.settle_secs("nouveau");
        // floor = 10/2 = 5
        assert!(settle >= 5, "expected adaptive settle >= 5, got {settle}");
    }

    #[test]
    fn reset_methods_unchanged_without_data() {
        let (_dir, journal) = test_journal();
        let adaptive =
            AdaptiveLifecycle::new(Box::new(StubLifecycle), journal, "0000:03:00.0".into());
        let methods = adaptive.available_reset_methods();
        assert_eq!(methods, vec![ResetMethod::BridgeSbr, ResetMethod::SysfsSbr]);
    }

    #[test]
    fn reset_methods_reorder_by_success_rate() {
        let (_dir, journal) = test_journal();
        let bdf = "0000:03:00.0";

        // bridge-sbr: 1/3 success, sbr: 3/3 success
        journal
            .append(&reset_entry(bdf, "bridge-sbr", true))
            .unwrap();
        journal
            .append(&reset_entry(bdf, "bridge-sbr", false))
            .unwrap();
        journal
            .append(&reset_entry(bdf, "bridge-sbr", false))
            .unwrap();
        journal.append(&reset_entry(bdf, "sbr", true)).unwrap();
        journal.append(&reset_entry(bdf, "sbr", true)).unwrap();
        journal.append(&reset_entry(bdf, "sbr", true)).unwrap();

        let adaptive = AdaptiveLifecycle::new(Box::new(StubLifecycle), journal, bdf.to_string());
        let methods = adaptive.available_reset_methods();
        // sbr has higher success rate → should come first
        assert_eq!(methods[0], ResetMethod::SysfsSbr);
        assert_eq!(methods[1], ResetMethod::BridgeSbr);
    }

    #[test]
    fn delegates_other_methods() {
        let (_dir, journal) = test_journal();
        let adaptive = AdaptiveLifecycle::new(Box::new(StubLifecycle), journal, "x".into());
        assert_eq!(adaptive.description(), "Stub");
        assert_eq!(
            adaptive.rebind_strategy("nouveau"),
            RebindStrategy::SimpleBind
        );
        adaptive.prepare_for_unbind("x", "vfio").unwrap();
        adaptive.stabilize_after_bind("x", "vfio");
        adaptive.verify_health("x", "vfio").unwrap();
    }
}
