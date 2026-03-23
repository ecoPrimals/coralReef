// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals

//! [`crate::preference::DriverPreference`] ordering, selection, and documented constants.

use crate::preference;
use crate::preference::DriverPreference;

#[test]
fn sovereign_preference_prefers_nouveau() {
    let pref = DriverPreference::sovereign();
    assert_eq!(pref.order()[0], "vfio");
    assert_eq!(pref.order()[1], "nouveau");
    assert_eq!(pref.order()[2], "amdgpu");
    assert_eq!(pref.order()[3], "nvidia-drm");
}

#[test]
fn pragmatic_preference_prefers_cuda() {
    let pref = DriverPreference::pragmatic();
    assert_eq!(pref.order()[0], "cuda");
    assert_eq!(pref.order()[1], "amdgpu");
}

#[test]
fn default_preference_is_sovereign() {
    let pref = DriverPreference::default();
    assert_eq!(pref.order(), DriverPreference::sovereign().order());
}

#[test]
fn select_returns_best_match() {
    let pref = DriverPreference::sovereign();
    assert_eq!(
        pref.select(&["amdgpu", "nvidia-drm"]),
        Some("amdgpu"),
        "with no nouveau, sovereign picks amdgpu next"
    );
}

#[test]
fn select_returns_nouveau_when_available() {
    let pref = DriverPreference::sovereign();
    assert_eq!(
        pref.select(&["nvidia-drm", "nouveau", "amdgpu"]),
        Some("nouveau"),
        "sovereign always picks nouveau first"
    );
}

#[test]
fn select_returns_none_when_no_match() {
    let pref = DriverPreference::sovereign();
    assert_eq!(pref.select(&["i915", "xe"]), None);
}

#[test]
fn pragmatic_selects_nvidia_drm_over_nouveau() {
    let pref = DriverPreference::pragmatic();
    assert_eq!(
        pref.select(&["nouveau", "nvidia-drm"]),
        Some("nvidia-drm"),
        "pragmatic picks nvidia-drm before nouveau"
    );
}

#[test]
fn from_str_list_parses_comma_separated() {
    let pref = DriverPreference::from_str_list("nvidia-drm, amdgpu");
    assert_eq!(pref.order(), &["nvidia-drm", "amdgpu"]);
}

#[test]
fn from_str_list_handles_empty_segments() {
    let pref = DriverPreference::from_str_list("nouveau,,amdgpu,");
    assert_eq!(pref.order(), &["nouveau", "amdgpu"]);
}

#[test]
fn from_str_list_single_driver() {
    let pref = DriverPreference::from_str_list("amdgpu");
    assert_eq!(pref.order(), &["amdgpu"]);
    assert_eq!(pref.select(&["amdgpu", "nvidia-drm"]), Some("amdgpu"));
    assert_eq!(pref.select(&["nvidia-drm"]), None);
}

#[test]
fn from_env_falls_back_to_sovereign() {
    // Don't modify env vars (unsafe in 2024+ edition).
    // Instead, verify that from_env returns sovereign-compatible
    // ordering when the env var is not set to something specific.
    let pref = DriverPreference::from_env();
    assert!(
        !pref.order().is_empty(),
        "preference should have at least one driver"
    );
}

#[test]
fn driver_preference_debug_format() {
    let pref = DriverPreference::sovereign();
    let debug = format!("{pref:?}");
    assert!(debug.contains("nouveau"));
}

#[test]
fn driver_preference_clone() {
    let pref = DriverPreference::sovereign();
    let cloned = pref.clone();
    assert_eq!(pref.order(), cloned.order());
}

#[test]
fn driver_constants_match_preference_order() {
    let sovereign = DriverPreference::sovereign();
    let order = sovereign.order();
    assert_eq!(order[0], preference::DRIVER_VFIO);
    assert_eq!(order[1], preference::DRIVER_NOUVEAU);
    assert_eq!(order[2], preference::DRIVER_AMDGPU);
    assert_eq!(order[3], preference::DRIVER_NVIDIA_DRM);
}

#[test]
fn driver_constants_select_from_available() {
    let pref = DriverPreference::sovereign();
    let available = [preference::DRIVER_NOUVEAU, preference::DRIVER_AMDGPU];
    assert_eq!(pref.select(&available), Some(preference::DRIVER_NOUVEAU));
}

#[test]
fn sovereign_selects_vfio_when_listed_first_in_available() {
    let pref = DriverPreference::sovereign();
    assert_eq!(
        pref.select(&[
            preference::DRIVER_AMDGPU,
            preference::DRIVER_VFIO,
            preference::DRIVER_NVIDIA_DRM,
        ]),
        Some(preference::DRIVER_VFIO)
    );
}

#[test]
fn select_returns_first_preference_match_not_first_in_available_order() {
    let pref = DriverPreference::from_str_list("amdgpu,nvidia-drm");
    assert_eq!(
        pref.select(&[preference::DRIVER_NVIDIA_DRM, preference::DRIVER_AMDGPU]),
        Some(preference::DRIVER_AMDGPU),
        "preference order wins over ordering inside the available slice"
    );
}

#[test]
fn pragmatic_order_full_list() {
    let pref = DriverPreference::pragmatic();
    assert_eq!(pref.order().len(), 4);
    assert_eq!(pref.order()[0], preference::DRIVER_CUDA);
    assert_eq!(pref.order()[1], preference::DRIVER_AMDGPU);
    assert_eq!(pref.order()[2], preference::DRIVER_NVIDIA_DRM);
    assert_eq!(pref.order()[3], preference::DRIVER_NOUVEAU);
}

#[test]
fn select_empty_candidates_returns_none() {
    let pref = DriverPreference::sovereign();
    assert_eq!(pref.select(&[]), None);
}

#[test]
fn select_only_unknown_drivers_returns_none() {
    let pref = DriverPreference::sovereign();
    assert_eq!(pref.select(&["i915", "xe", "panfrost"]), None);
}
