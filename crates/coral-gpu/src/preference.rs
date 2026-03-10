// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals

/// DRM driver identifiers in preference order.
///
/// coralReef prefers sovereign (open-source) drivers because they force deep
/// understanding and give us full control. But we also want to work on
/// whatever already exists on a deployment target.
///
/// Default preference: `nouveau` > `amdgpu` > `nvidia-drm`
///
/// - **nouveau**: Open-source NVIDIA DRM driver. Forces us to solve deep
///   (our own channel management, QMD, pushbuf). Full sovereignty.
/// - **amdgpu**: Open-source AMD DRM driver. Native Linux citizen. Full
///   dispatch pipeline already working.
/// - **nvidia-drm**: NVIDIA proprietary DRM module. Compatible with existing
///   deployments. Dispatch pending UVM integration.
///
/// Operators can override via `CORALREEF_DRIVER_PREFERENCE` environment
/// variable (comma-separated driver names):
///
/// ```text
/// CORALREEF_DRIVER_PREFERENCE=nouveau,amdgpu,nvidia-drm  # sovereign default
/// CORALREEF_DRIVER_PREFERENCE=nvidia-drm,amdgpu           # pragmatic (use what's installed)
/// CORALREEF_DRIVER_PREFERENCE=amdgpu                       # AMD-only deployment
/// ```
#[derive(Debug, Clone)]
pub struct DriverPreference {
    order: Vec<String>,
}

impl DriverPreference {
    /// Sovereign default: prefer open-source drivers, fall back to proprietary.
    #[must_use]
    pub fn sovereign() -> Self {
        Self {
            order: vec![
                "nouveau".to_string(),
                "amdgpu".to_string(),
                "nvidia-drm".to_string(),
            ],
        }
    }

    /// Pragmatic default: prefer whatever's most likely to work on a typical system.
    #[must_use]
    pub fn pragmatic() -> Self {
        Self {
            order: vec![
                "amdgpu".to_string(),
                "nvidia-drm".to_string(),
                "nouveau".to_string(),
            ],
        }
    }

    /// Parse from a comma-separated string (e.g. `"nouveau,amdgpu,nvidia-drm"`).
    #[must_use]
    pub fn from_str_list(s: &str) -> Self {
        Self {
            order: s
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
        }
    }

    /// Read from `CORALREEF_DRIVER_PREFERENCE` env var, falling back to sovereign default.
    #[must_use]
    pub fn from_env() -> Self {
        match std::env::var("CORALREEF_DRIVER_PREFERENCE") {
            Ok(val) if !val.is_empty() => Self::from_str_list(&val),
            _ => Self::sovereign(),
        }
    }

    /// The ordered list of preferred driver names.
    #[must_use]
    pub fn order(&self) -> &[String] {
        &self.order
    }

    /// Find the best matching driver from a list of available drivers.
    ///
    /// Returns the first driver in our preference order that appears in
    /// the available list. Returns `None` if no match.
    #[must_use]
    pub fn select<'a>(&self, available: &[&'a str]) -> Option<&'a str> {
        for preferred in &self.order {
            if let Some(&matched) = available.iter().find(|&&d| d == preferred) {
                return Some(matched);
            }
        }
        None
    }
}

impl Default for DriverPreference {
    fn default() -> Self {
        Self::sovereign()
    }
}
