// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals

use super::*;

#[test]
fn identity_get_fallback_matches_package() {
    let r = IdentityGetResponse::fallback();
    assert_eq!(r.name.as_ref(), env!("CARGO_PKG_NAME"));
    assert_eq!(r.version.as_ref(), env!("CARGO_PKG_VERSION"));
    assert!(r.transports.is_empty());
    assert!(!r.provides.is_empty());
}

#[test]
fn default_arch_matches_gpu_arch_default() {
    let arch = default_arch();
    assert_eq!(arch, coral_reef::GpuArch::default().to_string());
    assert!(!arch.is_empty(), "default arch must not be empty");
}

#[test]
fn default_opt_level_is_valid() {
    let level = default_opt_level();
    assert!(level <= 3, "opt level must be 0-3, got {level}");
}

#[test]
fn capability_list_response_serde_roundtrip() {
    let r = CapabilityListResponse {
        primal: env!("CARGO_PKG_NAME").into(),
        version: env!("CARGO_PKG_VERSION").into(),
        protocol: "jsonrpc-2.0".into(),
        transport: vec!["uds".into(), "tcp".into()],
        methods: vec!["health.check".to_owned(), "capability.list".to_owned()],
        capabilities: vec!["health".to_owned(), "shader.compile".to_owned()],
    };
    let json = serde_json::to_string(&r).expect("serialize");
    let roundtrip: CapabilityListResponse = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(roundtrip.primal.as_ref(), r.primal.as_ref());
    assert_eq!(roundtrip.version.as_ref(), r.version.as_ref());
    assert_eq!(roundtrip.methods, r.methods);
    assert_eq!(roundtrip.capabilities, r.capabilities);
}
