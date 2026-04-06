// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt;

use coral_ember::observation::SwapObservation;

use super::{DriverInsight, DriverObserver, Finding, FindingCategory};

/// Extracts insights from nouveau's mmiotrace data.
///
/// Nouveau boots falcons entirely through DMA (firmware uploaded via
/// PRAMIN/VRAM), relying heavily on PRIV ring resets and PRAMIN window
/// setup. This observer parses the mmiotrace to quantify those patterns.
#[derive(Debug, Clone)]
pub struct NouveauObserver;

impl fmt::Display for NouveauObserver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NouveauObserver")
    }
}

impl DriverObserver for NouveauObserver {
    fn personality_name(&self) -> &'static str {
        "nouveau"
    }

    fn observe_swap(&self, observation: &SwapObservation) -> Option<DriverInsight> {
        if observation.to_personality != "nouveau" {
            return None;
        }

        let mut findings = Vec::new();
        findings.push(Finding {
            category: FindingCategory::Other("swap_timing".to_string()),
            description: format!(
                "nouveau bind completed in {}ms (prepare={}ms, unbind={}ms, bind={}ms)",
                observation.timing.total_ms,
                observation.timing.prepare_ms,
                observation.timing.unbind_ms,
                observation.timing.bind_ms,
            ),
            register_offset: None,
            count: None,
        });

        Some(DriverInsight {
            personality: "nouveau".to_string(),
            findings,
        })
    }

    fn observe_trace(&self, trace_path: &str) -> Option<DriverInsight> {
        let content = std::fs::read_to_string(trace_path).ok()?;
        let mut findings = Vec::new();

        let mut priv_ring_resets = 0u32;
        let mut pramin_writes = 0u32;
        let mut pmc_enable_writes = 0u32;
        let mut gsp_writes = 0u32;
        let mut total_writes = 0u32;

        for line in content.lines() {
            if !line.starts_with('W') {
                continue;
            }
            total_writes += 1;

            // Parse mmiotrace W lines: W 4 <len> <addr> <value> ...
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 5 {
                continue;
            }
            let addr = match u64::from_str_radix(parts[3].trim_start_matches("0x"), 16) {
                Ok(a) => a,
                Err(_) => continue,
            };

            // Classify by BAR0 offset (bottom 24 bits)
            let offset = addr & 0xFF_FFFF;

            match offset {
                0x070000 => priv_ring_resets += 1,
                0x000204 => pmc_enable_writes += 1,
                0x100cb8..=0x100cfc => pramin_writes += 1,
                0x700000..=0x7FFFFF => pramin_writes += 1,
                0x800000..=0x800FFF => gsp_writes += 1,
                _ => {}
            }
        }

        findings.push(Finding {
            category: FindingCategory::PrivRingReset,
            description: format!("{priv_ring_resets} PRIV ring resets observed"),
            register_offset: Some(0x070000),
            count: Some(priv_ring_resets),
        });

        findings.push(Finding {
            category: FindingCategory::PraminWrite,
            description: format!("{pramin_writes} PRAMIN/VRAM window writes"),
            register_offset: None,
            count: Some(pramin_writes),
        });

        findings.push(Finding {
            category: FindingCategory::PmcEnable,
            description: format!("{pmc_enable_writes} PMC_ENABLE writes (engine power-on)"),
            register_offset: Some(0x000204),
            count: Some(pmc_enable_writes),
        });

        if gsp_writes > 0 {
            findings.push(Finding {
                category: FindingCategory::GspActivity,
                description: format!("{gsp_writes} GSP falcon register writes"),
                register_offset: Some(0x800000),
                count: Some(gsp_writes),
            });
        }

        findings.push(Finding {
            category: FindingCategory::Other("total_mmio_writes".to_string()),
            description: format!("{total_writes} total MMIO write lines"),
            register_offset: None,
            count: Some(total_writes),
        });

        Some(DriverInsight {
            personality: "nouveau".to_string(),
            findings,
        })
    }
}
