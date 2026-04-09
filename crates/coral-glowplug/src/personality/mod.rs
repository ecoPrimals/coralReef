// SPDX-License-Identifier: AGPL-3.0-or-later
#![expect(
    missing_docs,
    reason = "PersonalityTrait implementations are numerous; trait and registry docs are the primary reference."
)]
//! Trait-based GPU personality system.
//!
//! Evolved from enum Personality per ecosystem P3 review. Each driver
//! personality is a trait implementation allowing extensibility without
//! modifying the core enum. The [`PersonalityRegistry`] enables runtime
//! discovery of supported personalities.

mod impls;

pub use impls::{
    AkidaPersonality, AmdgpuPersonality, I915Personality, NouveauPersonality,
    NvidiaOpenPersonality, NvidiaOraclePersonality, NvidiaPersonality, UnboundPersonality,
    VfioPersonality, XePersonality,
};

use std::fmt;

/// Trait defining a GPU driver personality.
///
/// Each personality manages its driver-specific bind/unbind logic
/// and provides metadata for capability advertisement. Consumed as
/// `dyn GpuPersonality` from [`PersonalityRegistry::create`].
pub trait GpuPersonality: fmt::Display + fmt::Debug + Send + Sync {
    /// Short name for IPC identification (e.g. `"vfio"`, `"nouveau"`, `"amdgpu"`).
    #[must_use]
    fn name(&self) -> &'static str;

    /// Whether this personality provides direct hardware access (VFIO fd).
    #[must_use]
    fn provides_vfio(&self) -> bool;

    /// DRM card path, if applicable (e.g. `/dev/dri/card0`).
    #[must_use]
    fn drm_card(&self) -> Option<&str>;

    /// Whether this personality is suitable for HBM2 training.
    #[must_use]
    fn supports_hbm2_training(&self) -> bool;

    /// The kernel driver module name for sysfs bind/unbind.
    #[must_use]
    fn driver_module(&self) -> &'static str;
}

/// Runtime registry for resolving personality names to trait objects.
///
/// Capabilities are discovered at runtime rather than hardcoded —
/// each primal only knows about the personalities it can manage.
pub struct PersonalityRegistry {
    known: Vec<&'static str>,
}

impl PersonalityRegistry {
    /// Build the default registry with all known driver personalities.
    #[must_use]
    pub fn default_linux() -> Self {
        Self {
            known: vec![
                "vfio",
                "nouveau",
                "nvidia",
                "nvidia-open",
                "nvidia_oracle",
                "amdgpu",
                "xe",
                "i915",
                "akida-pcie",
                "unbound",
            ],
        }
    }

    /// Whether the given personality name is registered.
    #[must_use]
    pub fn supports(&self, name: &str) -> bool {
        self.known.contains(&name)
    }

    /// List all known personality names.
    #[must_use]
    pub fn list(&self) -> &[&'static str] {
        &self.known
    }

    /// Create a boxed personality from a name string.
    ///
    /// Returns `None` for unknown personality names.
    #[must_use]
    pub fn create(&self, name: &str) -> Option<Box<dyn GpuPersonality>> {
        match name {
            "vfio" | "vfio-pci" => Some(Box::new(VfioPersonality { group_id: 0 })),
            "nouveau" => Some(Box::new(NouveauPersonality {
                drm_card_path: None,
            })),
            "nvidia" => Some(Box::new(NvidiaPersonality {
                drm_card_path: None,
            })),
            "nvidia-open" => Some(Box::new(NvidiaOpenPersonality {
                drm_card_path: None,
            })),
            "amdgpu" => Some(Box::new(AmdgpuPersonality {
                drm_card_path: None,
            })),
            "xe" => Some(Box::new(XePersonality {
                drm_card_path: None,
            })),
            "i915" => Some(Box::new(I915Personality {
                drm_card_path: None,
            })),
            "akida-pcie" | "akida" => Some(Box::new(AkidaPersonality)),
            "nvidia_oracle" => Some(Box::new(NvidiaOraclePersonality {
                drm_card_path: None,
                module_name: "nvidia_oracle".to_string(),
            })),
            "unbound" => Some(Box::new(UnboundPersonality)),
            _ => None,
        }
    }
}

/// Concrete personality enum for owned storage in `DeviceSlot`.
///
/// Wraps the trait implementations for zero-allocation state transitions.
/// This is the runtime value; the trait is used for polymorphic dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Personality {
    Vfio {
        group_id: u32,
    },
    Nouveau {
        drm_card: Option<String>,
    },
    Nvidia {
        drm_card: Option<String>,
    },
    NvidiaOpen {
        drm_card: Option<String>,
    },
    NvidiaOracle {
        drm_card: Option<String>,
        module_name: String,
    },
    Amdgpu {
        drm_card: Option<String>,
    },
    Xe {
        drm_card: Option<String>,
    },
    I915 {
        drm_card: Option<String>,
    },
    Akida,
    Unbound,
}

impl Personality {
    /// Short name for IPC/config interchange.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Vfio { .. } => "vfio",
            Self::Nouveau { .. } => "nouveau",
            Self::Nvidia { .. } => "nvidia",
            Self::NvidiaOpen { .. } => "nvidia-open",
            Self::NvidiaOracle { .. } => "nvidia_oracle",
            Self::Amdgpu { .. } => "amdgpu",
            Self::Xe { .. } => "xe",
            Self::I915 { .. } => "i915",
            Self::Akida => "akida-pcie",
            Self::Unbound => "unbound",
        }
    }

    /// Whether this personality provides a VFIO fd.
    #[must_use]
    pub const fn provides_vfio(&self) -> bool {
        matches!(self, Self::Vfio { .. })
    }

    /// Whether this personality supports HBM2 training.
    #[must_use]
    pub const fn supports_hbm2_training(&self) -> bool {
        matches!(
            self,
            Self::Nouveau { .. }
                | Self::Nvidia { .. }
                | Self::NvidiaOpen { .. }
                | Self::NvidiaOracle { .. }
                | Self::Amdgpu { .. }
        )
    }
}

impl fmt::Display for Personality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Vfio { group_id } => write!(f, "vfio (group {group_id})"),
            Self::NvidiaOracle {
                drm_card,
                module_name,
            } => {
                write!(f, "{module_name}")?;
                let Some(card) = drm_card else {
                    return Ok(());
                };
                write!(f, " ({card})")?;
                Ok(())
            }
            Self::Nouveau { drm_card }
            | Self::Nvidia { drm_card }
            | Self::NvidiaOpen { drm_card }
            | Self::Amdgpu { drm_card }
            | Self::Xe { drm_card }
            | Self::I915 { drm_card } => {
                write!(f, "{}", self.name())?;
                let Some(card) = drm_card else {
                    return Ok(());
                };
                write!(f, " ({card})")?;
                Ok(())
            }
            Self::Akida => write!(f, "akida-pcie"),
            Self::Unbound => write!(f, "unbound"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_personality_names() {
        assert_eq!(Personality::Vfio { group_id: 5 }.name(), "vfio");
        assert_eq!(Personality::Nouveau { drm_card: None }.name(), "nouveau");
        assert_eq!(Personality::Amdgpu { drm_card: None }.name(), "amdgpu");
        assert_eq!(Personality::Unbound.name(), "unbound");
    }

    #[test]
    fn test_vfio_provides_fd() {
        assert!(Personality::Vfio { group_id: 1 }.provides_vfio());
        assert!(!Personality::Nouveau { drm_card: None }.provides_vfio());
    }

    #[test]
    fn test_hbm2_training() {
        assert!(Personality::Nouveau { drm_card: None }.supports_hbm2_training());
        assert!(!Personality::Vfio { group_id: 0 }.supports_hbm2_training());
        assert!(Personality::Amdgpu { drm_card: None }.supports_hbm2_training());
    }

    #[test]
    fn test_display() {
        assert_eq!(
            Personality::Vfio { group_id: 3 }.to_string(),
            "vfio (group 3)"
        );
        assert_eq!(
            Personality::Nouveau {
                drm_card: Some("/dev/dri/card0".into())
            }
            .to_string(),
            "nouveau (/dev/dri/card0)"
        );
        assert_eq!(Personality::Unbound.to_string(), "unbound");
    }

    #[test]
    fn test_registry_supports() {
        let reg = PersonalityRegistry::default_linux();
        assert!(reg.supports("vfio"));
        assert!(reg.supports("nouveau"));
        assert!(reg.supports("amdgpu"));
        assert!(!reg.supports("nvidia-proprietary"));
    }

    #[test]
    fn test_registry_create() {
        let reg = PersonalityRegistry::default_linux();
        let p = reg.create("vfio").unwrap();
        assert_eq!(p.name(), "vfio");
        assert!(p.provides_vfio());

        let n = reg.create("nouveau").unwrap();
        assert!(n.supports_hbm2_training());

        assert!(reg.create("unknown").is_none());
    }

    #[test]
    fn test_trait_display() {
        let vfio = VfioPersonality { group_id: 7 };
        assert_eq!(vfio.to_string(), "vfio (group 7)");

        let nouveau = NouveauPersonality {
            drm_card_path: Some("/dev/dri/card1".into()),
        };
        assert_eq!(nouveau.to_string(), "nouveau (/dev/dri/card1)");

        let amdgpu = AmdgpuPersonality {
            drm_card_path: Some("/dev/dri/card2".into()),
        };
        assert_eq!(amdgpu.to_string(), "amdgpu (/dev/dri/card2)");

        let amdgpu_no_card = AmdgpuPersonality {
            drm_card_path: None,
        };
        assert_eq!(amdgpu_no_card.to_string(), "amdgpu");

        assert_eq!(UnboundPersonality.to_string(), "unbound");
    }

    #[test]
    fn test_amdgpu_personality_trait() {
        let amdgpu = AmdgpuPersonality {
            drm_card_path: Some("/dev/dri/card1".into()),
        };
        assert_eq!(amdgpu.name(), "amdgpu");
        assert!(!amdgpu.provides_vfio());
        assert_eq!(amdgpu.drm_card(), Some("/dev/dri/card1"));
        assert!(amdgpu.supports_hbm2_training());
        assert_eq!(amdgpu.driver_module(), "amdgpu");
    }

    #[test]
    fn test_unbound_personality_trait() {
        let unbound = UnboundPersonality;
        assert_eq!(unbound.name(), "unbound");
        assert!(!unbound.provides_vfio());
        assert!(unbound.drm_card().is_none());
        assert!(!unbound.supports_hbm2_training());
        assert_eq!(unbound.driver_module(), "");
    }

    #[test]
    fn test_registry_list() {
        let reg = PersonalityRegistry::default_linux();
        let list = reg.list();
        assert!(list.contains(&"vfio"));
        assert!(list.contains(&"nouveau"));
        assert!(list.contains(&"amdgpu"));
        assert!(list.contains(&"nvidia"));
        assert!(list.contains(&"xe"));
        assert!(list.contains(&"i915"));
        assert!(list.contains(&"akida-pcie"));
        assert!(list.contains(&"nvidia-open"));
        assert!(list.contains(&"nvidia_oracle"));
        assert!(list.contains(&"unbound"));
        assert_eq!(list.len(), 10);
    }

    #[test]
    fn test_registry_create_vfio_pci_alias() {
        let reg = PersonalityRegistry::default_linux();
        let p = reg.create("vfio-pci").unwrap();
        assert_eq!(p.name(), "vfio");
        assert!(p.provides_vfio());
    }

    #[test]
    fn test_personality_amdgpu_display_with_card() {
        assert_eq!(
            Personality::Amdgpu {
                drm_card: Some("/dev/dri/card1".into())
            }
            .to_string(),
            "amdgpu (/dev/dri/card1)"
        );
    }

    #[test]
    fn test_personality_amdgpu_display_without_card() {
        assert_eq!(Personality::Amdgpu { drm_card: None }.to_string(), "amdgpu");
    }

    #[test]
    fn test_personality_partial_eq() {
        assert_eq!(
            Personality::Vfio { group_id: 1 },
            Personality::Vfio { group_id: 1 }
        );
        assert_ne!(
            Personality::Vfio { group_id: 1 },
            Personality::Vfio { group_id: 2 }
        );
    }

    #[test]
    fn test_personality_enum_names_cover_variants() {
        assert_eq!(Personality::Nvidia { drm_card: None }.name(), "nvidia");
        assert_eq!(Personality::Xe { drm_card: None }.name(), "xe");
        assert_eq!(Personality::I915 { drm_card: None }.name(), "i915");
        assert_eq!(Personality::Akida.name(), "akida-pcie");
    }

    #[test]
    fn test_personality_hbm2_flags_intel_and_akida() {
        assert!(!Personality::Xe { drm_card: None }.supports_hbm2_training());
        assert!(!Personality::I915 { drm_card: None }.supports_hbm2_training());
        assert!(Personality::Nvidia { drm_card: None }.supports_hbm2_training());
        assert!(Personality::NvidiaOpen { drm_card: None }.supports_hbm2_training());
        assert!(!Personality::Akida.supports_hbm2_training());
    }

    #[test]
    fn test_personality_display_nvidia_xe_i915_akida() {
        assert_eq!(Personality::Nvidia { drm_card: None }.to_string(), "nvidia");
        assert_eq!(
            Personality::Xe {
                drm_card: Some("/dev/dri/card2".into())
            }
            .to_string(),
            "xe (/dev/dri/card2)"
        );
        assert_eq!(
            Personality::I915 {
                drm_card: Some("/dev/dri/card0".into())
            }
            .to_string(),
            "i915 (/dev/dri/card0)"
        );
        assert_eq!(Personality::Akida.to_string(), "akida-pcie");
    }

    #[test]
    fn test_registry_create_aliases_and_intel() {
        let reg = PersonalityRegistry::default_linux();
        assert_eq!(reg.create("akida").unwrap().name(), "akida-pcie");
        let nvidia = reg.create("nvidia").unwrap();
        assert_eq!(nvidia.name(), "nvidia");
        assert!(nvidia.supports_hbm2_training());
        let xe = reg.create("xe").unwrap();
        assert_eq!(xe.driver_module(), "xe");
        let i915 = reg.create("i915").unwrap();
        assert_eq!(i915.drm_card(), None);
    }

    #[test]
    fn test_nvidia_xe_i915_akida_traits() {
        let nvidia = NvidiaPersonality {
            drm_card_path: Some("/dev/dri/card1".into()),
        };
        assert_eq!(nvidia.name(), "nvidia");
        assert!(nvidia.supports_hbm2_training());
        assert_eq!(nvidia.drm_card(), Some("/dev/dri/card1"));

        let xe = XePersonality {
            drm_card_path: None,
        };
        assert_eq!(xe.name(), "xe");
        assert!(!xe.provides_vfio());
        assert!(!xe.supports_hbm2_training());

        let i915 = I915Personality {
            drm_card_path: Some("/dev/dri/card0".into()),
        };
        assert_eq!(i915.driver_module(), "i915");

        let akida = AkidaPersonality;
        assert_eq!(akida.name(), "akida-pcie");
        assert_eq!(akida.drm_card(), None);
        assert_eq!(akida.driver_module(), "akida-pcie");
    }
}
