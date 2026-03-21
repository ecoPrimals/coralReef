// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals

//! Integration tests for `CORALREEF_DEFAULT_SM`, `CORALREEF_DEFAULT_SM_NOUVEAU`, and
//! `CORALREEF_DRIVER_PREFERENCE` (Rust 2024 `set_var` / `remove_var` are `unsafe`).

use std::sync::{Mutex, OnceLock};

use coral_gpu::{
    DEFAULT_NV_SM, DEFAULT_NV_SM_NOUVEAU, DriverPreference, default_nv_sm, default_nv_sm_nouveau,
};

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn lock_env() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

struct EnvRestore {
    key: &'static str,
    previous: Option<String>,
}

impl EnvRestore {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        // SAFETY: serialized by `ENV_LOCK`; no concurrent environment access.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, previous }
    }

    fn remove(key: &'static str) -> Self {
        let previous = std::env::var(key).ok();
        // SAFETY: serialized by `ENV_LOCK`; no concurrent environment access.
        unsafe {
            std::env::remove_var(key);
        }
        Self { key, previous }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        // SAFETY: serialized by `ENV_LOCK`; no concurrent environment access.
        unsafe {
            match &self.previous {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

#[test]
fn default_nv_sm_without_env_is_ampere_fallback() {
    let _g = lock_env();
    let _r = EnvRestore::remove("CORALREEF_DEFAULT_SM");
    assert_eq!(default_nv_sm(), DEFAULT_NV_SM);
}

#[test]
fn default_nv_sm_nouveau_without_env_is_volta_fallback() {
    let _g = lock_env();
    let _r = EnvRestore::remove("CORALREEF_DEFAULT_SM_NOUVEAU");
    assert_eq!(default_nv_sm_nouveau(), DEFAULT_NV_SM_NOUVEAU);
}

#[test]
fn default_nv_sm_env_override_75() {
    let _g = lock_env();
    let _r = EnvRestore::set("CORALREEF_DEFAULT_SM", "75");
    assert_eq!(default_nv_sm(), 75);
}

#[test]
fn default_nv_sm_nouveau_env_override_75() {
    let _g = lock_env();
    let _r = EnvRestore::set("CORALREEF_DEFAULT_SM_NOUVEAU", "75");
    assert_eq!(default_nv_sm_nouveau(), 75);
}

#[test]
fn default_nv_sm_invalid_env_falls_back() {
    let _g = lock_env();
    let _r = EnvRestore::set("CORALREEF_DEFAULT_SM", "abc");
    assert_eq!(default_nv_sm(), DEFAULT_NV_SM);
}

#[test]
fn default_nv_sm_nouveau_invalid_env_falls_back() {
    let _g = lock_env();
    let _r = EnvRestore::set("CORALREEF_DEFAULT_SM_NOUVEAU", "not-a-number");
    assert_eq!(default_nv_sm_nouveau(), DEFAULT_NV_SM_NOUVEAU);
}

#[test]
fn driver_preference_from_env_custom_list() {
    let _g = lock_env();
    let _r = EnvRestore::set("CORALREEF_DRIVER_PREFERENCE", "amdgpu,nvidia-drm");
    let pref = DriverPreference::from_env();
    assert_eq!(pref.order(), &["amdgpu", "nvidia-drm"]);
}

#[test]
fn driver_preference_from_env_empty_string_falls_back_sovereign() {
    let _g = lock_env();
    let _r = EnvRestore::set("CORALREEF_DRIVER_PREFERENCE", "");
    let pref = DriverPreference::from_env();
    assert_eq!(pref.order(), DriverPreference::sovereign().order());
}
