// SPDX-License-Identifier: AGPL-3.0-only
//! Capability-based self-description and peer discovery.
//!
//! Each primal starts with zero knowledge of the outside world. It knows only
//! what it *can do* — described as a set of typed capabilities. Discovery
//! happens at runtime through the universal adapter: this primal advertises its
//! capabilities, and requests capabilities it needs from whatever provider is
//! currently available.
//!
//! No primal names, no hardcoded addresses, no 2^n connection matrix.

use coral_reef::{AmdArch, GpuArch};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// A capability this primal can provide or consume.
///
/// Capabilities are namespaced strings (`domain.operation`) that describe
/// *what* without specifying *who*. The universal adapter resolves capabilities
/// to live providers at runtime.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Capability {
    /// Namespaced capability identifier (e.g. `"shader.compile"`).
    pub id: Cow<'static, str>,
    /// Semantic version of this capability's contract.
    pub version: Cow<'static, str>,
    /// Capability-specific metadata (arch support, limits, etc.).
    pub metadata: serde_json::Value,
}

/// What this primal advertises to the universal adapter on startup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescription {
    /// Capabilities this primal provides.
    pub provides: Vec<Capability>,
    /// Capabilities this primal needs from others.
    pub requires: Vec<Capability>,
    /// IPC transports this primal listens on (populated after bind).
    pub transports: Vec<Transport>,
}

/// An IPC transport endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transport {
    /// Protocol identifier (`"jsonrpc"`, `"tarpc"`, etc.).
    pub protocol: Cow<'static, str>,
    /// Bound address (populated at runtime after OS-assigned port).
    pub address: Cow<'static, str>,
}

/// Build this primal's self-description from compiled-in knowledge only.
///
/// No peer names, no external service references — only what this binary
/// knows about itself from its own code and configuration.
#[must_use]
pub fn self_description() -> SelfDescription {
    let mut all_archs: Vec<String> = GpuArch::ALL.iter().map(ToString::to_string).collect();
    all_archs.extend(AmdArch::ALL.iter().map(ToString::to_string));

    SelfDescription {
        provides: vec![
            Capability {
                id: "shader.compile".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                metadata: serde_json::json!({
                    "input_formats": ["spirv", "wgsl", "glsl"],
                    "architectures": all_archs,
                    "fma_policies": ["auto", "fused", "separate"],
                }),
            },
            Capability {
                id: "shader.compile.multi".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                metadata: serde_json::json!({
                    "max_targets": 64,
                    "cross_vendor": true,
                }),
            },
            Capability {
                id: "shader.health".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                metadata: serde_json::Value::Null,
            },
        ],
        requires: vec![Capability {
            id: "gpu.dispatch".into(),
            version: format!(">={}", env!("CARGO_PKG_VERSION")).into(),
            metadata: serde_json::json!({
                "reason": "QMD submission for compiled shaders",
            }),
        }],
        transports: Vec::new(),
    }
}

/// Attach bound transport addresses to a self-description.
///
/// Called after IPC servers bind to OS-assigned ports, before
/// advertising to the universal adapter.
#[must_use]
pub fn with_transports(mut desc: SelfDescription, transports: Vec<Transport>) -> SelfDescription {
    desc.transports = transports;
    desc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_description_provides_compile() {
        let desc = self_description();
        assert!(
            desc.provides.iter().any(|c| c.id == "shader.compile"),
            "must advertise shader.compile capability"
        );
    }

    #[test]
    fn self_description_no_peer_names() {
        let desc = self_description();
        let json = serde_json::to_string(&desc).unwrap();
        let json_lower = json.to_lowercase();

        for name in ["toadstool", "barracuda", "songbird", "nestgate", "squirrel"] {
            assert!(
                !json_lower.contains(name),
                "self-description must not contain peer name: {name}"
            );
        }
    }

    #[test]
    fn self_description_no_hardcoded_addresses() {
        let desc = self_description();
        let json = serde_json::to_string(&desc).unwrap();

        assert!(
            !json.contains("127.0.0.1"),
            "no hardcoded addresses in capabilities"
        );
        assert!(desc.transports.is_empty(), "transports empty before bind");
    }

    #[test]
    fn self_description_requires_dispatch() {
        let desc = self_description();
        assert!(
            desc.requires.iter().any(|c| c.id == "gpu.dispatch"),
            "must require gpu.dispatch capability"
        );
    }

    #[test]
    fn with_transports_populates() {
        let desc = self_description();
        let desc = with_transports(
            desc,
            vec![Transport {
                protocol: "jsonrpc".into(),
                address: "127.0.0.1:12345".into(),
            }],
        );
        assert_eq!(desc.transports.len(), 1);
        assert_eq!(desc.transports[0].protocol, "jsonrpc");
    }

    #[test]
    fn self_description_archs_match_all_vendors() {
        let desc = self_description();
        let compile_cap = desc
            .provides
            .iter()
            .find(|c| c.id == "shader.compile")
            .unwrap();
        let archs = compile_cap.metadata["architectures"].as_array().unwrap();
        assert_eq!(archs.len(), GpuArch::ALL.len() + AmdArch::ALL.len());
    }

    #[test]
    fn self_description_advertises_multi_compile() {
        let desc = self_description();
        assert!(
            desc.provides.iter().any(|c| c.id == "shader.compile.multi"),
            "must advertise shader.compile.multi capability"
        );
        let multi_cap = desc
            .provides
            .iter()
            .find(|c| c.id == "shader.compile.multi")
            .unwrap();
        assert!(multi_cap.metadata["cross_vendor"].as_bool().unwrap());
    }

    #[test]
    fn self_description_advertises_fma_policies() {
        let desc = self_description();
        let compile_cap = desc
            .provides
            .iter()
            .find(|c| c.id == "shader.compile")
            .unwrap();
        let policies = compile_cap.metadata["fma_policies"].as_array().unwrap();
        assert_eq!(policies.len(), 3);
        assert!(policies.iter().any(|p| p == "separate"));
    }

    #[test]
    fn with_transports_produces_correct_self_description() {
        let desc = self_description();
        let transports = vec![
            Transport {
                protocol: "jsonrpc".into(),
                address: "127.0.0.1:12345".into(),
            },
            Transport {
                protocol: "tarpc".into(),
                address: "unix:///run/coralreef.sock".into(),
            },
        ];
        let desc = with_transports(desc, transports);
        assert_eq!(desc.transports.len(), 2);
        assert_eq!(desc.transports[0].protocol, "jsonrpc");
        assert_eq!(desc.transports[0].address, "127.0.0.1:12345");
        assert_eq!(desc.transports[1].protocol, "tarpc");
        assert_eq!(desc.transports[1].address, "unix:///run/coralreef.sock");
        assert!(desc.provides.iter().any(|c| c.id == "shader.compile"));
        assert!(desc.requires.iter().any(|c| c.id == "gpu.dispatch"));
    }

    #[test]
    fn capabilities_roundtrip_serde_json() {
        let desc = self_description();
        let json = serde_json::to_string(&desc).unwrap();
        let roundtrip: SelfDescription = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.provides.len(), desc.provides.len());
        assert_eq!(roundtrip.requires.len(), desc.requires.len());
        for (a, b) in roundtrip.provides.iter().zip(desc.provides.iter()) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.version, b.version);
        }
        for (a, b) in roundtrip.requires.iter().zip(desc.requires.iter()) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.version, b.version);
        }
    }

    #[test]
    fn capability_equality_and_hash() {
        use std::collections::HashSet;

        let c1 = Capability {
            id: "test.cap".into(),
            version: "1.0".into(),
            metadata: serde_json::json!({"key": "value"}),
        };
        let c2 = Capability {
            id: "test.cap".into(),
            version: "1.0".into(),
            metadata: serde_json::json!({"key": "value"}),
        };
        assert_eq!(c1, c2);

        let mut set = HashSet::new();
        set.insert(c1);
        set.insert(c2);
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn with_transports_empty_preserves_rest() {
        let desc = self_description();
        let desc = with_transports(desc, vec![]);
        assert!(desc.transports.is_empty());
        assert!(!desc.provides.is_empty());
        assert!(!desc.requires.is_empty());
    }

    #[test]
    fn transport_serialization() {
        let t = Transport {
            protocol: "tarpc".into(),
            address: "unix:///tmp/sock".into(),
        };
        let json = serde_json::to_string(&t).unwrap();
        let roundtrip: Transport = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.protocol, t.protocol);
        assert_eq!(roundtrip.address, t.address);
    }

    #[test]
    fn self_description_shader_health_capability() {
        let desc = self_description();
        assert!(
            desc.provides.iter().any(|c| c.id == "shader.health"),
            "must advertise shader.health capability"
        );
    }
}
