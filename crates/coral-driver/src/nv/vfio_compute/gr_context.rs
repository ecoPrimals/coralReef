// SPDX-License-Identifier: AGPL-3.0-only
//! GR context lifecycle — allocate, bind, golden save/restore, context switch.
//!
//! Wires the FECS method ring (`fecs_method.rs`) into a complete GR context
//! management flow:
//!
//! 1. **Allocate**: Query FECS for context image size, allocate DMA buffer
//! 2. **Bind**: Tell FECS about the context buffer via `bind_pointer`
//! 3. **Golden save**: Save the initial (pristine) GR context state
//! 4. **Restore**: Reload a saved golden context before dispatch
//! 5. **Context switch**: Save current context, load a different one
//!
//! This matches nouveau's `gf100_gr_init` / `gf100_grctx_generate` flow.

use super::acr_boot::fecs_method;
use crate::error::{DriverError, DriverResult};
use crate::vfio::device::MappedBar;

/// GR context — a DMA-backed buffer holding the full GR engine state image.
///
/// Lifecycle: `allocate` → `bind` → `golden_save` → `dispatch` → `save`/`restore`.
pub struct GrContext {
    /// DMA buffer IOVA for the context image.
    pub iova: u64,
    /// Context image size in bytes (from FECS method 0x10).
    pub image_size: u32,
    /// Zcull image size (from FECS method 0x16), zero if unsupported.
    pub zcull_size: u32,
    /// PM image size (from FECS method 0x25), zero if unsupported.
    pub pm_size: u32,
    /// Whether a golden save has been performed.
    pub golden_saved: bool,
    /// Whether the context is currently bound to FECS.
    pub bound: bool,
}

/// Result of a GR context lifecycle operation.
#[derive(Debug)]
pub struct GrContextStatus {
    /// Human-readable description of what happened.
    pub description: String,
    /// Whether FECS is alive and responsive.
    pub fecs_alive: bool,
    /// Context image size discovered (0 if FECS not responding).
    pub image_size: u32,
    /// Whether golden context was saved.
    pub golden_saved: bool,
}

impl std::fmt::Display for GrContextStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "GrContext: fecs_alive={} image_size={} golden={} — {}",
            self.fecs_alive, self.image_size, self.golden_saved, self.description
        )
    }
}

/// Check if FECS is alive and responding to methods.
pub fn fecs_is_alive(bar0: &MappedBar) -> bool {
    use crate::vfio::channel::registers::falcon;
    let cpuctl = bar0
        .read_u32(falcon::FECS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD_DEAD);
    let halted = cpuctl & falcon::CPUCTL_HALTED != 0;
    let hreset = cpuctl & falcon::CPUCTL_HRESET != 0;
    !halted && !hreset && cpuctl != 0xDEAD_DEAD
}

/// Discover all GR context sizes from FECS.
///
/// Returns `(image_size, zcull_size, pm_size)`.
/// Requires FECS to be running (warm from nouveau or ACR boot).
pub fn discover_context_sizes(bar0: &MappedBar) -> DriverResult<(u32, u32, u32)> {
    if !fecs_is_alive(bar0) {
        return Err(DriverError::DeviceNotFound(
            "FECS not running — cannot discover context sizes".into(),
        ));
    }

    let image_size = fecs_method::fecs_discover_image_size(bar0)?;
    let zcull_size = fecs_method::fecs_discover_zcull_image_size(bar0).unwrap_or(0);
    let pm_size = fecs_method::fecs_discover_pm_image_size(bar0).unwrap_or(0);

    tracing::info!(
        image_size,
        zcull_size,
        pm_size,
        "GR context sizes discovered"
    );
    Ok((image_size, zcull_size, pm_size))
}

/// Full GR context lifecycle: discover sizes, bind context buffer, save golden state.
///
/// `context_iova` is the IOVA of a pre-allocated DMA buffer large enough for the
/// context image. Callers should first call `discover_context_sizes` to determine the
/// required buffer size and allocate via `alloc_dma`.
///
/// Follows nouveau's sequence:
/// 1. `fecs_init_exceptions` — enable FECS exception handling
/// 2. `fecs_set_watchdog_timeout` — prevent FECS firmware watchdog timeout
/// 3. `fecs_bind_pointer` — point FECS at our context buffer
/// 4. `fecs_wfi_golden_save` — save pristine GR state as the golden image
pub fn bind_and_golden_save(bar0: &MappedBar, context_iova: u64) -> DriverResult<GrContext> {
    if !fecs_is_alive(bar0) {
        return Err(DriverError::DeviceNotFound(
            "FECS not running — cannot perform GR context lifecycle".into(),
        ));
    }

    let (image_size, zcull_size, pm_size) = discover_context_sizes(bar0)?;

    fecs_method::fecs_init_exceptions(bar0);
    fecs_method::fecs_set_watchdog_timeout(bar0, 0x7fff_ffff)?;

    tracing::info!(
        context_iova = format!("{context_iova:#010x}"),
        "binding context pointer to FECS"
    );
    fecs_method::fecs_bind_pointer(bar0, context_iova)?;

    tracing::info!("performing WFI + golden context save");
    fecs_method::fecs_wfi_golden_save(bar0, context_iova)?;

    tracing::info!(
        image_size,
        "golden context saved — GR engine ready for dispatch"
    );

    Ok(GrContext {
        iova: context_iova,
        image_size,
        zcull_size,
        pm_size,
        golden_saved: true,
        bound: true,
    })
}

/// Rebind a previously saved context to FECS (for context restore / switch).
///
/// After a swap cycle or FECS re-initialization, call this with the IOVA
/// of a saved context image to restore GR state.
pub fn rebind_context(bar0: &MappedBar, context_iova: u64) -> DriverResult<()> {
    if !fecs_is_alive(bar0) {
        return Err(DriverError::DeviceNotFound(
            "FECS not running — cannot rebind context".into(),
        ));
    }

    fecs_method::fecs_init_exceptions(bar0);
    fecs_method::fecs_bind_pointer(bar0, context_iova)?;

    tracing::info!(
        context_iova = format!("{context_iova:#010x}"),
        "context rebound to FECS"
    );
    Ok(())
}

/// Attempt a context save: issue WFI + save the current GR state.
///
/// The context at `context_iova` must already be bound via `bind_and_golden_save`
/// or `rebind_context`. The save captures the full GR engine state including
/// register values, texture samplers, and shader program state.
pub fn save_context(bar0: &MappedBar, context_iova: u64) -> DriverResult<()> {
    if !fecs_is_alive(bar0) {
        return Err(DriverError::DeviceNotFound(
            "FECS not running — cannot save context".into(),
        ));
    }
    fecs_method::fecs_wfi_golden_save(bar0, context_iova)?;
    tracing::info!(
        context_iova = format!("{context_iova:#010x}"),
        "context saved via WFI"
    );
    Ok(())
}

/// Full GR context lifecycle probe — attempts the entire chain and returns status.
///
/// Non-panicking: returns a `GrContextStatus` even if any step fails.
/// Used by the experiment sweep and observer infrastructure.
pub fn probe_gr_context_lifecycle(bar0: &MappedBar, context_iova: u64) -> GrContextStatus {
    let alive = fecs_is_alive(bar0);
    if !alive {
        return GrContextStatus {
            description: "FECS not running — skipping GR context lifecycle".into(),
            fecs_alive: false,
            image_size: 0,
            golden_saved: false,
        };
    }

    let sizes = match discover_context_sizes(bar0) {
        Ok((img, _zcull, _pm)) => img,
        Err(e) => {
            return GrContextStatus {
                description: format!("discover_context_sizes failed: {e}"),
                fecs_alive: true,
                image_size: 0,
                golden_saved: false,
            };
        }
    };

    match bind_and_golden_save(bar0, context_iova) {
        Ok(_ctx) => GrContextStatus {
            description: format!(
                "GR context lifecycle complete: {}B image, golden saved at IOVA {context_iova:#x}",
                sizes
            ),
            fecs_alive: true,
            image_size: sizes,
            golden_saved: true,
        },
        Err(e) => GrContextStatus {
            description: format!("bind_and_golden_save failed: {e}"),
            fecs_alive: true,
            image_size: sizes,
            golden_saved: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gr_context_status_display() {
        let status = GrContextStatus {
            description: "test status".into(),
            fecs_alive: true,
            image_size: 4096,
            golden_saved: true,
        };
        let s = format!("{status}");
        assert!(s.contains("fecs_alive=true"));
        assert!(s.contains("image_size=4096"));
        assert!(s.contains("golden=true"));
    }

    #[test]
    fn gr_context_struct_defaults() {
        let ctx = GrContext {
            iova: 0x1000,
            image_size: 8192,
            zcull_size: 0,
            pm_size: 0,
            golden_saved: false,
            bound: false,
        };
        assert_eq!(ctx.iova, 0x1000);
        assert_eq!(ctx.image_size, 8192);
        assert!(!ctx.golden_saved);
        assert!(!ctx.bound);
    }
}
