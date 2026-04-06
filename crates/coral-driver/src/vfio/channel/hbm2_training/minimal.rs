// SPDX-License-Identifier: AGPL-3.0-or-later
//! Binary search for minimal write set.

use crate::vfio::device::MappedBar;
use crate::vfio::memory::{MemoryRegion, PraminRegion};

/// Binary search within a single domain's writes to find the minimal set
/// that unlocks VRAM. Requires that the full set is known to work.
pub fn binary_search_minimal_writes(
    bar0: &MappedBar,
    bdf: Option<&str>,
    domain_writes: &[(usize, u32)],
) -> Vec<(usize, u32)> {
    if domain_writes.is_empty() {
        return vec![];
    }

    for &(off, val) in domain_writes {
        let _ = bar0.write_u32(off, val);
    }
    std::thread::sleep(std::time::Duration::from_millis(100));

    let vram_ok = if let Ok(mut region) = PraminRegion::new(bar0, 0x0002_6000, 8) {
        region.probe_sentinel(0, 0xCAFE_DEAD).is_working()
    } else {
        false
    };

    if !vram_ok {
        tracing::debug!("MinimalSet: full set doesn't work, cannot binary search");
        return domain_writes.to_vec();
    }

    let mut needed = domain_writes.to_vec();

    if needed.len() > 4 {
        let mid = needed.len() / 2;
        let first_half = &needed[..mid];

        if let Some(bdf) = bdf {
            let _ = crate::vfio::pci_discovery::set_pci_power_state(
                bdf,
                crate::vfio::pci_discovery::PciPmState::D3Hot,
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
            let _ = crate::vfio::pci_discovery::force_pci_d0(bdf);
            std::thread::sleep(std::time::Duration::from_millis(50));

            for &(off, val) in first_half {
                let _ = bar0.write_u32(off, val);
            }
            std::thread::sleep(std::time::Duration::from_millis(100));

            let half_ok = if let Ok(mut region) = PraminRegion::new(bar0, 0x0002_6000, 8) {
                region.probe_sentinel(0, 0xCAFE_DEAD).is_working()
            } else {
                false
            };

            if half_ok {
                tracing::debug!(
                    "MinimalSet: first half ({} writes) sufficient",
                    first_half.len()
                );
                needed = first_half.to_vec();
            } else {
                tracing::debug!("MinimalSet: need full set ({} writes)", needed.len());
            }
        }
    }

    needed
}
