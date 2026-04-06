// SPDX-License-Identifier: AGPL-3.0-or-later
//! Unit tests for crate-root configuration helpers and error display.

use crate::{EmberDeviceConfig, EmberRunOptions, parse_glowplug_config};

fn sample_device(role: Option<&str>) -> EmberDeviceConfig {
    EmberDeviceConfig {
        bdf: "0000:01:00.0".to_string(),
        name: None,
        boot_personality: None,
        power_policy: None,
        role: role.map(|s| s.to_string()),
        oracle_dump: None,
    }
}

#[test]
fn ember_device_config_is_display_only_for_display_role() {
    let mut d = sample_device(None);
    assert!(!d.is_display());
    d.role = Some("compute".to_string());
    assert!(!d.is_display());
    d.role = Some("display".to_string());
    assert!(d.is_display());
}

#[test]
fn ember_device_config_is_shared_only_for_shared_role() {
    let mut d = sample_device(None);
    assert!(!d.is_shared());
    d.role = Some("shared".to_string());
    assert!(d.is_shared());
    d.role = Some("display".to_string());
    assert!(!d.is_shared());
}

#[test]
fn ember_device_config_is_protected_for_display_or_shared() {
    let mut d = sample_device(None);
    assert!(!d.is_protected());
    d.role = Some("compute".to_string());
    assert!(!d.is_protected());
    d.role = Some("display".to_string());
    assert!(d.is_protected());
    d.role = Some("shared".to_string());
    assert!(d.is_protected());
}

#[test]
fn parse_glowplug_config_roles_roundtrip() {
    let toml = r#"
            [[device]]
            bdf = "0000:01:00.0"
            role = "display"

            [[device]]
            bdf = "0000:02:00.0"
            role = "shared"
        "#;
    let cfg = parse_glowplug_config(toml).expect("valid glowplug TOML");
    assert_eq!(cfg.device.len(), 2);
    assert!(cfg.device[0].is_display());
    assert!(cfg.device[1].is_shared());
    assert!(!cfg.device[1].is_display());
}

#[test]
fn parse_glowplug_config_invalid_returns_error() {
    assert!(
        parse_glowplug_config("[[device]]\n bdf =").is_err(),
        "truncated device table must not parse"
    );
}

#[test]
fn parse_glowplug_config_empty_device_list() {
    let cfg = parse_glowplug_config("device = []").expect("valid empty device list");
    assert!(cfg.device.is_empty());
}

#[test]
fn ember_run_options_default_is_empty() {
    assert_eq!(
        EmberRunOptions::default(),
        EmberRunOptions {
            config_path: None,
            listen_port: None,
        }
    );
}

#[test]
fn parse_glowplug_config_device_optional_fields_roundtrip() {
    let toml = r#"
            [[device]]
            bdf = "0000:0a:00.0"
            name = "Test GPU"
            boot_personality = "nouveau"
            power_policy = "power_save"
            role = "compute"
            oracle_dump = "/tmp/oracle.bin"
        "#;
    let cfg = parse_glowplug_config(toml).expect("valid TOML");
    assert_eq!(cfg.device.len(), 1);
    let d = &cfg.device[0];
    assert_eq!(d.bdf, "0000:0a:00.0");
    assert_eq!(d.name.as_deref(), Some("Test GPU"));
    assert_eq!(d.boot_personality.as_deref(), Some("nouveau"));
    assert_eq!(d.power_policy.as_deref(), Some("power_save"));
    assert_eq!(d.role.as_deref(), Some("compute"));
    assert_eq!(d.oracle_dump.as_deref(), Some("/tmp/oracle.bin"));
}

#[test]
fn swap_error_from_string_maps_to_other() {
    let e: crate::error::SwapError = "orchestrator gave up".to_string().into();
    match e {
        crate::error::SwapError::Other(s) => assert_eq!(s, "orchestrator gave up"),
        other => panic!("expected Other, got {other:?}"),
    }
}

#[test]
fn ember_ipc_error_from_string_maps_to_dispatch() {
    let e: crate::error::EmberIpcError = "lock failed".to_string().into();
    match e {
        crate::error::EmberIpcError::Dispatch(s) => assert_eq!(s, "lock failed"),
        other => panic!("expected Dispatch, got {other:?}"),
    }
}

#[test]
fn sysfs_error_write_and_read_display() {
    let w = crate::error::SysfsError::Write {
        path: "/sys/a".into(),
        reason: "busy".into(),
    };
    assert!(w.to_string().contains("sysfs write"));
    assert!(w.to_string().contains("/sys/a"));
    let r = crate::error::SysfsError::Read {
        path: "/sys/b".into(),
        reason: "eof".into(),
    };
    assert!(r.to_string().contains("sysfs read"));
    let d = crate::error::SysfsError::DriverBind {
        bdf: "0000:01:00.0".into(),
        reason: "EEXIST".into(),
    };
    assert!(d.to_string().contains("driver bind"));
    let p = crate::error::SysfsError::PciReset {
        bdf: "0000:02:00.0".into(),
        reason: "reset failed".into(),
    };
    assert!(p.to_string().contains("PCI reset"));
}

#[test]
fn sysfs_error_pci_and_bridge_variants_display() {
    let e = crate::error::SysfsError::BridgeNotFound {
        bdf: "0000:01:00.0".into(),
    };
    assert!(e.to_string().contains("parent PCI bridge"));
    let e2 = crate::error::SysfsError::BridgeResetMissing {
        bdf: "0000:01:00.0".into(),
        bridge_bdf: "0000:00:01.0".into(),
    };
    assert!(e2.to_string().contains("bridge"));
    let e3 = crate::error::SysfsError::DeviceNotReappeared {
        bdf: "0000:02:00.0".into(),
    };
    assert!(e3.to_string().contains("re-appear"));
    let e4 = crate::error::SysfsError::PmCycleD3cold {
        bdf: "0000:03:00.0".into(),
    };
    assert!(e4.to_string().contains("D3cold"));
}

#[test]
fn swap_error_displays_preflight_drm_external_vfio_and_reset_method() {
    let p = crate::error::SwapError::Preflight {
        bdf: "0000:01:00.0".into(),
        reason: "nvidia_drm".into(),
    };
    assert!(p.to_string().contains("preflight"));
    let d = crate::error::SwapError::DrmIsolation("modeset active".into());
    assert!(d.to_string().contains("DRM isolation"));
    let x = crate::error::SwapError::ExternalVfioHolders {
        bdf: "0000:01:00.0".into(),
        count: 2,
    };
    assert!(x.to_string().contains("2 holders"));
    let u = crate::error::SwapError::UnknownTarget("fictional".into());
    assert!(u.to_string().contains("unknown target"));
    let t = crate::error::SwapError::Trace("mmiotrace busy".into());
    assert!(t.to_string().contains("trace"));
    let v = crate::error::SwapError::VerifyHealth {
        bdf: "0000:01:00.0".into(),
        detail: "no temp sensor".into(),
    };
    assert!(v.to_string().contains("post-bind verification"));
    let a = crate::error::SwapError::ActiveDisplayGpu {
        bdf: "0000:01:00.0".into(),
    };
    assert!(a.to_string().contains("display GPU"));
    let r = crate::error::SwapError::VfioReacquire {
        bdf: "0000:01:00.0".into(),
        reason: "open failed".into(),
    };
    assert!(r.to_string().contains("VFIO reacquire"));
    let i = crate::error::SwapError::InvalidResetMethod("kitten_reset".into());
    assert!(i.to_string().contains("kitten_reset"));
}

#[test]
fn swap_error_from_sysfs_uses_from_trait() {
    let inner = crate::error::SysfsError::PciReset {
        bdf: "0000:01:00.0".into(),
        reason: "no reset".into(),
    };
    let s: crate::error::SwapError = inner.into();
    match s {
        crate::error::SwapError::Sysfs(e) => {
            assert!(e.to_string().contains("PCI reset"));
        }
        other => panic!("expected Sysfs wrapper, got {other:?}"),
    }
}

#[test]
fn trace_error_displays_enable_disable_capture() {
    let e = crate::error::TraceError::Enable("busy".into());
    assert!(e.to_string().contains("mmiotrace enable"));
    let e2 = crate::error::TraceError::Disable("failed".into());
    assert!(e2.to_string().contains("mmiotrace disable"));
    let e3 = crate::error::TraceError::Capture {
        bdf: "0000:01:00.0".into(),
        reason: "disk full".into(),
    };
    assert!(e3.to_string().contains("trace capture"));
}

#[test]
fn ember_ipc_error_invalid_request_io_utf8_lock_json_send_display() {
    assert_eq!(
        crate::error::EmberIpcError::InvalidRequest("empty body").to_string(),
        "invalid request: empty body"
    );
    let io: crate::error::EmberIpcError =
        std::io::Error::new(std::io::ErrorKind::NotFound, "nope").into();
    assert!(io.to_string().contains("I/O error"));
    let invalid_utf8 = vec![0xff_u8];
    let utf8_err = std::str::from_utf8(&invalid_utf8).unwrap_err();
    let u: crate::error::EmberIpcError = utf8_err.into();
    assert!(u.to_string().contains("UTF-8"));
    assert_eq!(
        crate::error::EmberIpcError::LockPoisoned.to_string(),
        "RwLock poisoned"
    );
    let j = crate::error::EmberIpcError::JsonSerialize("bad".into());
    assert!(j.to_string().contains("JSON serialization"));
    let s = crate::error::EmberIpcError::SendMsg("e".into());
    assert!(s.to_string().contains("sendmsg"));
}
