// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt;

use coral_ember::observation::SwapObservation;

use super::{DriverInsight, DriverObserver, Finding, FindingCategory};

/// Observer for the open-source NVIDIA kernel module (GSP-based).
///
/// The open module uses GSP firmware for falcon management, producing
/// different MMIO patterns compared to the closed-source driver.
#[derive(Debug, Clone)]
pub struct NvidiaOpenObserver;

impl fmt::Display for NvidiaOpenObserver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NvidiaOpenObserver")
    }
}

impl DriverObserver for NvidiaOpenObserver {
    fn personality_name(&self) -> &'static str {
        "nvidia-open"
    }

    fn observe_swap(&self, observation: &SwapObservation) -> Option<DriverInsight> {
        if observation.to_personality != "nvidia-open" {
            return None;
        }

        Some(DriverInsight {
            personality: "nvidia-open".to_string(),
            findings: vec![Finding {
                category: FindingCategory::Other("nvidia_open_bind".to_string()),
                description: format!(
                    "NVIDIA Open (GSP) bind completed in {}ms",
                    observation.timing.total_ms,
                ),
                register_offset: None,
                count: None,
            }],
        })
    }

    fn observe_trace(&self, trace_path: &str) -> Option<DriverInsight> {
        let content = std::fs::read_to_string(trace_path).ok()?;
        let mut findings = Vec::new();
        let mut gsp_writes = 0u32;
        let mut total_writes = 0u32;

        for line in content.lines() {
            if !line.starts_with('W') {
                continue;
            }
            total_writes += 1;

            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 5 {
                continue;
            }
            if let Ok(addr) = u64::from_str_radix(parts[3].trim_start_matches("0x"), 16) {
                let offset = addr & 0xFF_FFFF;
                if (0x800000..=0x80FFFF).contains(&offset) {
                    gsp_writes += 1;
                }
            }
        }

        if gsp_writes > 0 {
            findings.push(Finding {
                category: FindingCategory::GspActivity,
                description: format!("{gsp_writes} GSP falcon region writes"),
                register_offset: Some(0x800000),
                count: Some(gsp_writes),
            });
        }

        findings.push(Finding {
            category: FindingCategory::Other("total_mmio_writes".to_string()),
            description: format!("{total_writes} total MMIO writes from nvidia-open driver"),
            register_offset: None,
            count: Some(total_writes),
        });

        Some(DriverInsight {
            personality: "nvidia-open".to_string(),
            findings,
        })
    }
}
