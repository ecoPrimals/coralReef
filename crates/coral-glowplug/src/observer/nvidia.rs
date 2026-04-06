// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt;

use coral_ember::observation::SwapObservation;

use super::{DriverInsight, DriverObserver, Finding, FindingCategory};

/// Observer for the proprietary NVIDIA driver.
///
/// The closed-source driver's MMIO patterns are opaque, but mmiotrace
/// captures allow us to extract timing, PRIV ring resets, PMC enable
/// sequences, and falcon boot patterns — the same register addresses
/// as nouveau, just driven by a different code path.
#[derive(Debug, Clone)]
pub struct NvidiaObserver;

const NVIDIA_BIND_SLOW_THRESHOLD_MS: u64 = 5000;

impl fmt::Display for NvidiaObserver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NvidiaObserver")
    }
}

impl DriverObserver for NvidiaObserver {
    fn personality_name(&self) -> &'static str {
        "nvidia"
    }

    fn observe_swap(&self, observation: &SwapObservation) -> Option<DriverInsight> {
        if observation.to_personality != "nvidia" {
            return None;
        }

        let mut findings = vec![Finding {
            category: FindingCategory::Other("nvidia_bind".to_string()),
            description: format!(
                "NVIDIA proprietary bind completed in {}ms",
                observation.timing.total_ms,
            ),
            register_offset: None,
            count: None,
        }];

        if observation.timing.total_ms > NVIDIA_BIND_SLOW_THRESHOLD_MS {
            findings.push(Finding {
                category: FindingCategory::PowerStateChange,
                description: format!(
                    "slow bind ({}ms > {NVIDIA_BIND_SLOW_THRESHOLD_MS}ms) — \
                     GPU may be recovering from D3cold or performing full DEVINIT",
                    observation.timing.total_ms,
                ),
                register_offset: None,
                count: None,
            });
        }

        Some(DriverInsight {
            personality: "nvidia".to_string(),
            findings,
        })
    }

    fn observe_trace(&self, trace_path: &str) -> Option<DriverInsight> {
        let content = std::fs::read_to_string(trace_path).ok()?;
        let mut findings = Vec::new();
        let mut total_writes = 0u32;
        let mut priv_resets = 0u32;
        let mut pmc_enables = 0u32;
        let mut falcon_boots = 0u32;

        for line in content.lines() {
            if !line.starts_with('W') {
                continue;
            }
            total_writes += 1;

            if let Some(addr) = extract_mmio_addr(line) {
                if addr == 0x0007_0000 {
                    priv_resets += 1;
                } else if addr == 0x0000_0200 {
                    pmc_enables += 1;
                } else if (0x0010_a000..0x0010_b000).contains(&addr)
                    || (0x0008_4000..0x0008_5000).contains(&addr)
                {
                    falcon_boots += 1;
                }
            }
        }

        findings.push(Finding {
            category: FindingCategory::Other("total_mmio_writes".to_string()),
            description: format!("{total_writes} total MMIO write lines from nvidia driver"),
            register_offset: None,
            count: Some(total_writes),
        });

        if priv_resets > 0 {
            findings.push(Finding {
                category: FindingCategory::PrivRingReset,
                description: format!("{priv_resets} PRIV ring reset writes (0x070000)"),
                register_offset: Some(0x0007_0000),
                count: Some(priv_resets),
            });
        }
        if pmc_enables > 0 {
            findings.push(Finding {
                category: FindingCategory::PmcEnable,
                description: format!("{pmc_enables} PMC_ENABLE writes"),
                register_offset: Some(0x0000_0200),
                count: Some(pmc_enables),
            });
        }
        if falcon_boots > 0 {
            findings.push(Finding {
                category: FindingCategory::FalconBoot,
                description: format!(
                    "{falcon_boots} falcon engine register writes (FECS/SEC2 range)"
                ),
                register_offset: None,
                count: Some(falcon_boots),
            });
        }

        Some(DriverInsight {
            personality: "nvidia".to_string(),
            findings,
        })
    }
}

/// Extract the MMIO address from an mmiotrace write line.
///
/// Format: `W <width> <timestamp> <address> <value> ...`
pub(crate) fn extract_mmio_addr(line: &str) -> Option<u64> {
    let mut parts = line.split_whitespace();
    parts.next()?; // W
    parts.next()?; // width
    parts.next()?; // timestamp
    let addr_str = parts.next()?;
    u64::from_str_radix(addr_str.trim_start_matches("0x"), 16).ok()
}
