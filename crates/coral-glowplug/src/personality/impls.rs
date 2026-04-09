// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt;

use super::GpuPersonality;

/// VFIO-PCI personality — direct hardware access for sovereign GPU control.
#[derive(Debug, Clone)]
pub struct VfioPersonality {
    /// VFIO group this device belongs to.
    pub group_id: u32,
}

impl fmt::Display for VfioPersonality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "vfio (group {})", self.group_id)
    }
}

impl GpuPersonality for VfioPersonality {
    fn name(&self) -> &'static str {
        "vfio"
    }
    fn provides_vfio(&self) -> bool {
        true
    }
    fn drm_card(&self) -> Option<&str> {
        None
    }
    fn supports_hbm2_training(&self) -> bool {
        false
    }
    fn driver_module(&self) -> &'static str {
        "vfio-pci"
    }
}

/// Nouveau personality — open-source NVIDIA driver (required for HBM2 training).
#[derive(Debug, Clone)]
pub struct NouveauPersonality {
    /// DRM card device path (e.g. `/dev/dri/card0`).
    pub drm_card_path: Option<String>,
}

impl fmt::Display for NouveauPersonality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "nouveau")?;
        let Some(card) = &self.drm_card_path else {
            return Ok(());
        };
        write!(f, " ({card})")?;
        Ok(())
    }
}

impl GpuPersonality for NouveauPersonality {
    fn name(&self) -> &'static str {
        "nouveau"
    }
    fn provides_vfio(&self) -> bool {
        false
    }
    fn drm_card(&self) -> Option<&str> {
        self.drm_card_path.as_deref()
    }
    fn supports_hbm2_training(&self) -> bool {
        true
    }
    fn driver_module(&self) -> &'static str {
        "nouveau"
    }
}

/// AMDGPU personality — AMD kernel driver.
#[derive(Debug, Clone)]
pub struct AmdgpuPersonality {
    /// DRM card device path (e.g. `/dev/dri/card1`).
    pub drm_card_path: Option<String>,
}

impl fmt::Display for AmdgpuPersonality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "amdgpu")?;
        let Some(card) = &self.drm_card_path else {
            return Ok(());
        };
        write!(f, " ({card})")?;
        Ok(())
    }
}

impl GpuPersonality for AmdgpuPersonality {
    fn name(&self) -> &'static str {
        "amdgpu"
    }
    fn provides_vfio(&self) -> bool {
        false
    }
    fn drm_card(&self) -> Option<&str> {
        self.drm_card_path.as_deref()
    }
    fn supports_hbm2_training(&self) -> bool {
        true
    }
    fn driver_module(&self) -> &'static str {
        "amdgpu"
    }
}

/// NVIDIA proprietary personality — closed-source kernel driver.
#[derive(Debug, Clone)]
pub struct NvidiaPersonality {
    /// DRM card device path (e.g. `/dev/dri/card1`).
    pub drm_card_path: Option<String>,
}

impl fmt::Display for NvidiaPersonality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "nvidia")?;
        let Some(card) = &self.drm_card_path else {
            return Ok(());
        };
        write!(f, " ({card})")?;
        Ok(())
    }
}

impl GpuPersonality for NvidiaPersonality {
    fn name(&self) -> &'static str {
        "nvidia"
    }
    fn provides_vfio(&self) -> bool {
        false
    }
    fn drm_card(&self) -> Option<&str> {
        self.drm_card_path.as_deref()
    }
    fn supports_hbm2_training(&self) -> bool {
        true
    }
    fn driver_module(&self) -> &'static str {
        "nvidia"
    }
}

/// NVIDIA open kernel module personality — open-source nvidia.ko (GSP-based).
///
/// Distinguished from `NvidiaPersonality` (closed-source) because the open
/// kernel module uses GSP firmware for falcon management, producing different
/// register write sequences during boot. The kernel module name is the same
/// (`nvidia`), but the personality tracks which variant is loaded for the
/// solution matrix.
#[derive(Debug, Clone)]
pub struct NvidiaOpenPersonality {
    /// DRM card device path (e.g. `/dev/dri/card1`).
    pub drm_card_path: Option<String>,
}

impl fmt::Display for NvidiaOpenPersonality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "nvidia-open")?;
        let Some(card) = &self.drm_card_path else {
            return Ok(());
        };
        write!(f, " ({card})")?;
        Ok(())
    }
}

impl GpuPersonality for NvidiaOpenPersonality {
    fn name(&self) -> &'static str {
        "nvidia-open"
    }
    fn provides_vfio(&self) -> bool {
        false
    }
    fn drm_card(&self) -> Option<&str> {
        self.drm_card_path.as_deref()
    }
    fn supports_hbm2_training(&self) -> bool {
        true
    }
    fn driver_module(&self) -> &'static str {
        "nvidia"
    }
}

/// Intel Xe personality — modern Intel discrete GPU driver (Arc, Battlemage).
#[derive(Debug, Clone)]
pub struct XePersonality {
    /// DRM card device path (e.g. `/dev/dri/card1`).
    pub drm_card_path: Option<String>,
}

impl fmt::Display for XePersonality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "xe")?;
        let Some(card) = &self.drm_card_path else {
            return Ok(());
        };
        write!(f, " ({card})")?;
        Ok(())
    }
}

impl GpuPersonality for XePersonality {
    fn name(&self) -> &'static str {
        "xe"
    }
    fn provides_vfio(&self) -> bool {
        false
    }
    fn drm_card(&self) -> Option<&str> {
        self.drm_card_path.as_deref()
    }
    fn supports_hbm2_training(&self) -> bool {
        false
    }
    fn driver_module(&self) -> &'static str {
        "xe"
    }
}

/// Intel i915 personality — legacy Intel GPU driver (integrated + early discrete).
#[derive(Debug, Clone)]
pub struct I915Personality {
    /// DRM card device path (e.g. `/dev/dri/card0`).
    pub drm_card_path: Option<String>,
}

impl fmt::Display for I915Personality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "i915")?;
        let Some(card) = &self.drm_card_path else {
            return Ok(());
        };
        write!(f, " ({card})")?;
        Ok(())
    }
}

impl GpuPersonality for I915Personality {
    fn name(&self) -> &'static str {
        "i915"
    }
    fn provides_vfio(&self) -> bool {
        false
    }
    fn drm_card(&self) -> Option<&str> {
        self.drm_card_path.as_deref()
    }
    fn supports_hbm2_training(&self) -> bool {
        false
    }
    fn driver_module(&self) -> &'static str {
        "i915"
    }
}

/// BrainChip Akida personality — neuromorphic NPU driver.
#[derive(Debug, Clone)]
pub struct AkidaPersonality;

impl fmt::Display for AkidaPersonality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "akida-pcie")
    }
}

impl GpuPersonality for AkidaPersonality {
    fn name(&self) -> &'static str {
        "akida-pcie"
    }
    fn provides_vfio(&self) -> bool {
        false
    }
    fn drm_card(&self) -> Option<&str> {
        None
    }
    fn supports_hbm2_training(&self) -> bool {
        false
    }
    fn driver_module(&self) -> &'static str {
        "akida-pcie"
    }
}

/// NVIDIA Oracle personality — renamed nvidia module for multi-version coexistence.
#[derive(Debug, Clone)]
pub struct NvidiaOraclePersonality {
    /// DRM card device path (e.g. `/dev/dri/card1`).
    pub drm_card_path: Option<String>,
    /// The oracle module name (e.g. "nvidia_oracle", "nvidia_oracle_535").
    pub module_name: String,
}

impl fmt::Display for NvidiaOraclePersonality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.module_name)?;
        let Some(card) = &self.drm_card_path else {
            return Ok(());
        };
        write!(f, " ({card})")?;
        Ok(())
    }
}

impl GpuPersonality for NvidiaOraclePersonality {
    fn name(&self) -> &'static str {
        "nvidia_oracle"
    }
    fn provides_vfio(&self) -> bool {
        false
    }
    fn drm_card(&self) -> Option<&str> {
        self.drm_card_path.as_deref()
    }
    fn supports_hbm2_training(&self) -> bool {
        true
    }
    fn driver_module(&self) -> &'static str {
        "nvidia_oracle"
    }
}

/// Unbound state — no driver attached.
#[derive(Debug, Clone)]
pub struct UnboundPersonality;

impl fmt::Display for UnboundPersonality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unbound")
    }
}

impl GpuPersonality for UnboundPersonality {
    fn name(&self) -> &'static str {
        "unbound"
    }
    fn provides_vfio(&self) -> bool {
        false
    }
    fn drm_card(&self) -> Option<&str> {
        None
    }
    fn supports_hbm2_training(&self) -> bool {
        false
    }
    fn driver_module(&self) -> &'static str {
        ""
    }
}
