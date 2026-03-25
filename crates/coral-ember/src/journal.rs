// SPDX-License-Identifier: AGPL-3.0-only
//! Persistent experiment journal — JSONL append log for swap, reset, and boot observations.
//!
//! Every driver swap, device reset, and boot attempt is recorded as a
//! [`JournalEntry`]. The journal lives on disk as newline-delimited JSON
//! (JSONL) so it survives daemon restarts and can be queried for
//! cross-personality comparison and adaptive lifecycle tuning.
//!
//! # Storage
//!
//! Default path: `/var/lib/coralreef/journal.jsonl`
//! Override: `CORALREEF_JOURNAL_PATH` environment variable.
//!
//! Writes are append-only with per-entry flushing for crash safety.

use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::observation::{ResetObservation, SwapObservation};

/// Default journal file path.
const DEFAULT_JOURNAL_PATH: &str = "/var/lib/coralreef/journal.jsonl";

/// A single journal entry, tagged by kind for heterogeneous queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum JournalEntry {
    /// A driver swap completed (successfully or not).
    Swap(SwapObservation),
    /// A device reset was attempted.
    Reset(ResetObservation),
    /// A sovereign boot attempt from coral-driver.
    BootAttempt {
        /// PCI BDF address.
        bdf: String,
        /// Boot strategy name.
        strategy: String,
        /// Whether the boot attempt succeeded.
        success: bool,
        /// SEC2 exception info register after attempt.
        sec2_exci: u32,
        /// FECS program counter after attempt.
        fecs_pc: u32,
        /// GPCCS exception info register after attempt.
        gpccs_exci: u32,
        /// Human-readable diagnostic notes.
        notes: Vec<String>,
        /// Unix epoch milliseconds.
        timestamp_epoch_ms: u64,
    },
}

impl JournalEntry {
    /// BDF of the device this entry pertains to.
    pub fn bdf(&self) -> &str {
        match self {
            Self::Swap(obs) => &obs.bdf,
            Self::Reset(obs) => &obs.bdf,
            Self::BootAttempt { bdf, .. } => bdf,
        }
    }

    /// Unix epoch milliseconds for this entry.
    pub fn timestamp_epoch_ms(&self) -> u64 {
        match self {
            Self::Swap(obs) => obs.timestamp_epoch_ms,
            Self::Reset(obs) => obs.timestamp_epoch_ms,
            Self::BootAttempt {
                timestamp_epoch_ms, ..
            } => *timestamp_epoch_ms,
        }
    }

    /// Kind tag string for filtering.
    pub fn kind_tag(&self) -> &'static str {
        match self {
            Self::Swap(_) => "Swap",
            Self::Reset(_) => "Reset",
            Self::BootAttempt { .. } => "BootAttempt",
        }
    }
}

/// Filter for journal queries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JournalFilter {
    /// Only entries for this BDF.
    pub bdf: Option<String>,
    /// Only entries of this kind ("Swap", "Reset", "BootAttempt").
    pub kind: Option<String>,
    /// Only entries matching this personality (as from or to).
    pub personality: Option<String>,
    /// Only entries after this timestamp (epoch ms).
    pub after: Option<u64>,
    /// Only entries before this timestamp (epoch ms).
    pub before: Option<u64>,
    /// Maximum number of entries to return (most recent first).
    pub limit: Option<usize>,
}

impl JournalFilter {
    fn matches(&self, entry: &JournalEntry) -> bool {
        if let Some(ref bdf) = self.bdf {
            if entry.bdf() != bdf {
                return false;
            }
        }
        if let Some(ref kind) = self.kind {
            if entry.kind_tag() != kind {
                return false;
            }
        }
        if let Some(ref personality) = self.personality {
            let matches = match entry {
                JournalEntry::Swap(obs) => {
                    obs.to_personality == *personality
                        || obs.from_personality.as_deref() == Some(personality)
                }
                JournalEntry::Reset(_) => false,
                JournalEntry::BootAttempt { strategy, .. } => strategy == personality,
            };
            if !matches {
                return false;
            }
        }
        if let Some(after) = self.after {
            if entry.timestamp_epoch_ms() < after {
                return false;
            }
        }
        if let Some(before) = self.before {
            if entry.timestamp_epoch_ms() > before {
                return false;
            }
        }
        true
    }
}

/// Aggregate statistics from journal entries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JournalStats {
    /// Total number of swap entries.
    pub total_swaps: u64,
    /// Total number of reset entries.
    pub total_resets: u64,
    /// Total number of boot attempt entries.
    pub total_boot_attempts: u64,
    /// Per-personality swap statistics.
    pub personality_stats: Vec<PersonalityStats>,
    /// Per-method reset statistics.
    pub reset_method_stats: Vec<ResetMethodStats>,
}

/// Per-personality swap statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalityStats {
    /// Personality name.
    pub personality: String,
    /// Number of successful swaps to this personality.
    pub swap_count: u64,
    /// Average total swap time in milliseconds.
    pub avg_total_ms: u64,
    /// Average bind phase time in milliseconds.
    pub avg_bind_ms: u64,
    /// Average unbind phase time in milliseconds.
    pub avg_unbind_ms: u64,
}

/// Per-method reset statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResetMethodStats {
    /// Reset method name.
    pub method: String,
    /// Number of attempts with this method.
    pub attempts: u64,
    /// Number of successes.
    pub successes: u64,
    /// Success rate (0.0 to 1.0).
    pub success_rate: f64,
    /// Average duration in milliseconds.
    pub avg_duration_ms: u64,
}

/// Persistent JSONL experiment journal.
pub struct Journal {
    path: PathBuf,
}

impl Journal {
    /// Open (or create) a journal at the given path.
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        Self { path }
    }

    /// Open using the default or env-overridden path.
    pub fn open_default() -> Self {
        let path = std::env::var("CORALREEF_JOURNAL_PATH")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_JOURNAL_PATH.to_string());
        Self::open(path)
    }

    /// Path to the journal file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append a single entry (atomic: serialize + write + flush).
    pub fn append(&self, entry: &JournalEntry) -> io::Result<()> {
        let mut line = serde_json::to_string(entry)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        line.push('\n');

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(line.as_bytes())?;
        file.flush()
    }

    /// Read all entries matching a filter. Returns newest-first when limit is set.
    pub fn query(&self, filter: &JournalFilter) -> io::Result<Vec<JournalEntry>> {
        let file = match File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };

        let reader = BufReader::new(file);
        let mut results: Vec<JournalEntry> = Vec::new();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<JournalEntry>(&line) {
                Ok(entry) if filter.matches(&entry) => results.push(entry),
                Ok(_) => {}
                Err(e) => {
                    tracing::debug!(error = %e, "skipping malformed journal line");
                }
            }
        }

        if let Some(limit) = filter.limit {
            if results.len() > limit {
                let start = results.len() - limit;
                results = results[start..].to_vec();
            }
        }

        Ok(results)
    }

    /// Compute aggregate statistics, optionally filtered by BDF.
    pub fn stats(&self, bdf: Option<&str>) -> io::Result<JournalStats> {
        let filter = JournalFilter {
            bdf: bdf.map(|s| s.to_string()),
            ..Default::default()
        };
        let entries = self.query(&filter)?;

        let mut stats = JournalStats::default();
        let mut personality_accum: std::collections::HashMap<String, (u64, u64, u64, u64)> =
            std::collections::HashMap::new();
        let mut reset_accum: std::collections::HashMap<String, (u64, u64, u64)> =
            std::collections::HashMap::new();

        for entry in &entries {
            match entry {
                JournalEntry::Swap(obs) => {
                    stats.total_swaps += 1;
                    let acc = personality_accum
                        .entry(obs.to_personality.clone())
                        .or_insert((0, 0, 0, 0));
                    acc.0 += 1;
                    acc.1 += obs.timing.total_ms;
                    acc.2 += obs.timing.bind_ms;
                    acc.3 += obs.timing.unbind_ms;
                }
                JournalEntry::Reset(obs) => {
                    stats.total_resets += 1;
                    let acc = reset_accum
                        .entry(obs.method.clone())
                        .or_insert((0, 0, 0));
                    acc.0 += 1;
                    if obs.success {
                        acc.1 += 1;
                    }
                    acc.2 += obs.duration_ms;
                }
                JournalEntry::BootAttempt { .. } => {
                    stats.total_boot_attempts += 1;
                }
            }
        }

        stats.personality_stats = personality_accum
            .into_iter()
            .map(|(personality, (count, total, bind, unbind))| PersonalityStats {
                personality,
                swap_count: count,
                avg_total_ms: if count > 0 { total / count } else { 0 },
                avg_bind_ms: if count > 0 { bind / count } else { 0 },
                avg_unbind_ms: if count > 0 { unbind / count } else { 0 },
            })
            .collect();
        stats.personality_stats.sort_by(|a, b| b.swap_count.cmp(&a.swap_count));

        stats.reset_method_stats = reset_accum
            .into_iter()
            .map(|(method, (attempts, successes, duration))| ResetMethodStats {
                method,
                attempts,
                successes,
                success_rate: if attempts > 0 {
                    successes as f64 / attempts as f64
                } else {
                    0.0
                },
                avg_duration_ms: if attempts > 0 {
                    duration / attempts
                } else {
                    0
                },
            })
            .collect();
        stats
            .reset_method_stats
            .sort_by(|a, b| b.attempts.cmp(&a.attempts));

        Ok(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observation::{HealthResult, SwapTiming};

    fn test_swap_obs(bdf: &str, to: &str, total_ms: u64) -> SwapObservation {
        SwapObservation {
            bdf: bdf.to_string(),
            from_personality: Some("vfio".to_string()),
            to_personality: to.to_string(),
            timestamp_epoch_ms: 1700000000000 + total_ms,
            timing: SwapTiming {
                prepare_ms: 10,
                unbind_ms: 100,
                bind_ms: total_ms.saturating_sub(200),
                stabilize_ms: 50,
                total_ms,
            },
            trace_path: None,
            health: HealthResult::Ok,
            lifecycle_description: "test".to_string(),
            reset_method_used: None,
        }
    }

    fn test_reset_obs(bdf: &str, method: &str, success: bool) -> ResetObservation {
        ResetObservation {
            bdf: bdf.to_string(),
            method: method.to_string(),
            success,
            error: if success {
                None
            } else {
                Some("test error".to_string())
            },
            timestamp_epoch_ms: 1700000000000,
            duration_ms: 500,
        }
    }

    #[test]
    fn journal_append_and_query_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let journal = Journal::open(dir.path().join("test.jsonl"));

        let swap = JournalEntry::Swap(test_swap_obs("0000:03:00.0", "nouveau", 5000));
        journal.append(&swap).expect("append swap");

        let reset = JournalEntry::Reset(test_reset_obs("0000:03:00.0", "bridge-sbr", true));
        journal.append(&reset).expect("append reset");

        let entries = journal
            .query(&JournalFilter::default())
            .expect("query all");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].kind_tag(), "Swap");
        assert_eq!(entries[1].kind_tag(), "Reset");
    }

    #[test]
    fn journal_query_filters_by_bdf() {
        let dir = tempfile::tempdir().expect("tempdir");
        let journal = Journal::open(dir.path().join("test.jsonl"));

        journal
            .append(&JournalEntry::Swap(test_swap_obs(
                "0000:03:00.0",
                "nouveau",
                5000,
            )))
            .unwrap();
        journal
            .append(&JournalEntry::Swap(test_swap_obs(
                "0000:4a:00.0",
                "vfio",
                3000,
            )))
            .unwrap();

        let filter = JournalFilter {
            bdf: Some("0000:03:00.0".to_string()),
            ..Default::default()
        };
        let entries = journal.query(&filter).expect("filtered query");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].bdf(), "0000:03:00.0");
    }

    #[test]
    fn journal_query_filters_by_kind() {
        let dir = tempfile::tempdir().expect("tempdir");
        let journal = Journal::open(dir.path().join("test.jsonl"));

        journal
            .append(&JournalEntry::Swap(test_swap_obs(
                "0000:03:00.0",
                "nouveau",
                5000,
            )))
            .unwrap();
        journal
            .append(&JournalEntry::Reset(test_reset_obs(
                "0000:03:00.0",
                "bridge-sbr",
                true,
            )))
            .unwrap();

        let filter = JournalFilter {
            kind: Some("Reset".to_string()),
            ..Default::default()
        };
        let entries = journal.query(&filter).expect("kind filter");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind_tag(), "Reset");
    }

    #[test]
    fn journal_query_with_limit_returns_newest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let journal = Journal::open(dir.path().join("test.jsonl"));

        for i in 0..5 {
            journal
                .append(&JournalEntry::Swap(test_swap_obs(
                    "0000:03:00.0",
                    "nouveau",
                    1000 * (i + 1),
                )))
                .unwrap();
        }

        let filter = JournalFilter {
            limit: Some(2),
            ..Default::default()
        };
        let entries = journal.query(&filter).expect("limited query");
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn journal_stats_computes_aggregates() {
        let dir = tempfile::tempdir().expect("tempdir");
        let journal = Journal::open(dir.path().join("test.jsonl"));

        journal
            .append(&JournalEntry::Swap(test_swap_obs(
                "0000:03:00.0",
                "nouveau",
                5000,
            )))
            .unwrap();
        journal
            .append(&JournalEntry::Swap(test_swap_obs(
                "0000:03:00.0",
                "nouveau",
                7000,
            )))
            .unwrap();
        journal
            .append(&JournalEntry::Reset(test_reset_obs(
                "0000:03:00.0",
                "bridge-sbr",
                true,
            )))
            .unwrap();
        journal
            .append(&JournalEntry::Reset(test_reset_obs(
                "0000:03:00.0",
                "bridge-sbr",
                false,
            )))
            .unwrap();

        let stats = journal.stats(None).expect("stats");
        assert_eq!(stats.total_swaps, 2);
        assert_eq!(stats.total_resets, 2);
        assert_eq!(stats.personality_stats.len(), 1);
        assert_eq!(stats.personality_stats[0].personality, "nouveau");
        assert_eq!(stats.personality_stats[0].avg_total_ms, 6000);
        assert_eq!(stats.reset_method_stats.len(), 1);
        assert_eq!(stats.reset_method_stats[0].successes, 1);
        assert!((stats.reset_method_stats[0].success_rate - 0.5).abs() < 0.01);
    }

    #[test]
    fn journal_query_empty_file_returns_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let journal = Journal::open(dir.path().join("nonexistent.jsonl"));
        let entries = journal
            .query(&JournalFilter::default())
            .expect("query empty");
        assert!(entries.is_empty());
    }

    #[test]
    fn journal_entry_serde_roundtrip_boot_attempt() {
        let entry = JournalEntry::BootAttempt {
            bdf: "0000:03:00.0".to_string(),
            strategy: "EmemBoot".to_string(),
            success: false,
            sec2_exci: 0x091f0000,
            fecs_pc: 0x023c,
            gpccs_exci: 0x08070000,
            notes: vec!["SEC2 faulted during ROM".to_string()],
            timestamp_epoch_ms: 1700000000000,
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        let back: JournalEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.kind_tag(), "BootAttempt");
        assert_eq!(back.bdf(), "0000:03:00.0");
    }

    #[test]
    fn journal_filter_personality_matches_swap() {
        let filter = JournalFilter {
            personality: Some("nouveau".to_string()),
            ..Default::default()
        };
        let matching = JournalEntry::Swap(test_swap_obs("x", "nouveau", 100));
        let non_matching = JournalEntry::Swap(test_swap_obs("x", "vfio", 100));
        assert!(filter.matches(&matching));
        assert!(!filter.matches(&non_matching));
    }
}
