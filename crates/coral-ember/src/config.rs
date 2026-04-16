// SPDX-License-Identifier: AGPL-3.0-or-later
//! Glowplug config parsing, path resolution, and [`EmberRunOptions`] for the daemon.

use serde::Deserialize;

/// Parsed `glowplug.toml` top-level structure for ember.
#[derive(Deserialize)]
pub struct EmberConfig {
    #[serde(default)]
    /// Devices listed in the glowplug config (BDF and optional metadata).
    pub device: Vec<EmberDeviceConfig>,
}

/// One device entry from `glowplug.toml` (same schema as coral-glowplug).
///
/// ```
/// use coral_ember::parse_glowplug_config;
///
/// let cfg = parse_glowplug_config(
///     r#"[[device]]
/// bdf = "0000:01:00.0"
/// role = "display"
/// "#,
/// )
/// .expect("valid TOML");
/// assert!(cfg.device[0].is_display());
/// assert!(cfg.device[0].is_protected());
/// ```
#[derive(Deserialize)]
pub struct EmberDeviceConfig {
    /// PCI bus/device/function address (e.g. `0000:01:00.0`).
    pub bdf: String,
    #[serde(default)]
    /// Optional human-readable name.
    pub name: Option<String>,
    #[serde(default)]
    /// Boot personality hint.
    pub boot_personality: Option<String>,
    #[serde(default)]
    /// Power policy hint.
    pub power_policy: Option<String>,
    #[serde(default)]
    /// Role hint (e.g. compute).
    pub role: Option<String>,
    #[serde(default)]
    /// Oracle dump path.
    pub oracle_dump: Option<String>,
}

impl EmberDeviceConfig {
    /// Returns `true` if this device has `role = "display"`, meaning it is a
    /// protected display GPU that ember must never touch, unbind, or manage.
    ///
    /// ```
    /// use coral_ember::EmberDeviceConfig;
    ///
    /// let display = EmberDeviceConfig {
    ///     bdf: "0000:01:00.0".into(),
    ///     name: None,
    ///     boot_personality: None,
    ///     power_policy: None,
    ///     role: Some("display".into()),
    ///     oracle_dump: None,
    /// };
    /// assert!(display.is_display());
    /// ```
    #[must_use]
    pub fn is_display(&self) -> bool {
        self.role.as_deref() == Some("display")
    }

    /// Returns `true` if this device has `role = "shared"` — serves both display and compute.
    #[must_use]
    pub fn is_shared(&self) -> bool {
        self.role.as_deref() == Some("shared")
    }

    /// Returns `true` if this device is protected from driver swaps (display or shared).
    #[must_use]
    pub fn is_protected(&self) -> bool {
        self.is_display() || self.is_shared()
    }
}

/// Family ID for multi-instance isolation (from `$BIOMEOS_FAMILY_ID`, default `"default"`).
pub(crate) fn family_id() -> String {
    std::env::var("BIOMEOS_FAMILY_ID").unwrap_or_else(|_| "default".into())
}

/// Default ecosystem namespace per wateringHole `PRIMAL_IPC_PROTOCOL` v3.0.
const ECOSYSTEM_NAMESPACE: &str = "biomeos";

/// Ecosystem namespace for shared directories (sockets, discovery).
pub(crate) fn ecosystem_namespace() -> &'static str {
    use std::sync::OnceLock;
    static NS: OnceLock<String> = OnceLock::new();
    NS.get_or_init(|| {
        std::env::var("BIOMEOS_ECOSYSTEM_NAMESPACE").unwrap_or_else(|_| ECOSYSTEM_NAMESPACE.into())
    })
}

/// Base directory for ecosystem socket/discovery files.
///
/// `$XDG_RUNTIME_DIR/<namespace>` (or `$TMPDIR/<namespace>` when XDG is unset).
/// Shared by both primal socket layout and BTSP security socket discovery.
#[must_use]
pub(crate) fn resolve_socket_dir() -> std::path::PathBuf {
    let base = std::env::var("XDG_RUNTIME_DIR")
        .map_or_else(|_| std::env::temp_dir(), std::path::PathBuf::from);
    base.join(ecosystem_namespace())
}

/// Check that `BIOMEOS_INSECURE` and `BIOMEOS_FAMILY_ID` are not both active.
///
/// Per wateringHole `PRIMAL_SELF_KNOWLEDGE_STANDARD` v1.1: a primal must
/// refuse to start when a non-default family ID is set AND insecure mode is
/// requested — you cannot claim a family AND skip authentication.
///
/// # Errors
///
/// Returns [`crate::error::ConfigError::InsecureWithFamily`] if the invariant is violated.
pub fn validate_insecure_guard() -> Result<(), crate::error::ConfigError> {
    let fid = family_id();
    let insecure = std::env::var("BIOMEOS_INSECURE")
        .ok()
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));
    if insecure && fid != "default" {
        return Err(crate::error::ConfigError::InsecureWithFamily { family_id: fid });
    }
    Ok(())
}

/// Environment variable for the optional TCP JSON-RPC listen port (set when `--port` is used).
pub const EMBER_LISTEN_PORT_ENV: &str = "CORALREEF_EMBER_PORT";

/// Options for [`crate::run_with_options`] (UniBin `server` entry).
///
/// ```
/// use coral_ember::EmberRunOptions;
///
/// let opts = EmberRunOptions {
///     config_path: Some("/etc/coralreef/glowplug.toml".into()),
///     listen_port: Some(9000),
/// };
/// assert_eq!(opts.listen_port, Some(9000));
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmberRunOptions {
    /// Path to `glowplug.toml`; when `None`, uses [`find_config`] (XDG then system).
    pub config_path: Option<String>,
    /// When `Some`, also listens on `127.0.0.1:port` for JSON-RPC over TCP.
    pub listen_port: Option<u16>,
}

/// Default socket path for ember IPC. Override with `$CORALREEF_EMBER_SOCKET`.
///
/// ```
/// let path = coral_ember::ember_socket_path();
/// assert!(path.ends_with("ember.sock"));
/// ```
#[must_use]
pub fn ember_socket_path() -> String {
    std::env::var("CORALREEF_EMBER_SOCKET")
        .unwrap_or_else(|_| "/run/coralreef/ember.sock".to_string())
}

/// System-wide glowplug config path (same default and `$CORALREEF_GLOWPLUG_CONFIG` as coral-glowplug).
///
/// ```
/// let path = coral_ember::system_glowplug_config_path();
/// assert!(path.ends_with("glowplug.toml"));
/// ```
#[must_use]
pub fn system_glowplug_config_path() -> String {
    std::env::var("CORALREEF_GLOWPLUG_CONFIG")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/etc/coralreef/glowplug.toml".to_string())
}

/// Parse `glowplug.toml` contents into [`EmberConfig`].
///
/// ```
/// use coral_ember::parse_glowplug_config;
///
/// let cfg = parse_glowplug_config("device = []").expect("parse empty device list");
/// assert!(cfg.device.is_empty());
/// ```
pub fn parse_glowplug_config(config_str: &str) -> Result<EmberConfig, toml::de::Error> {
    toml::from_str(config_str)
}

/// Resolve a glowplug config path: XDG config first, then [`system_glowplug_config_path`].
#[must_use]
pub fn find_config() -> Option<String> {
    let xdg = std::env::var("XDG_CONFIG_HOME")
        .or_else(|_| std::env::var("HOME").map(|h| format!("{h}/.config")))
        .ok()
        .map(|base| format!("{base}/coralreef/glowplug.toml"));

    let system = system_glowplug_config_path();

    let paths: Vec<String> = xdg.into_iter().chain(std::iter::once(system)).collect();

    for path in paths {
        if std::path::Path::new(&path).exists() {
            tracing::info!(path = %path, "found config");
            return Some(path);
        }
    }
    None
}
