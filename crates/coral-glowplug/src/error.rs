// SPDX-License-Identifier: AGPL-3.0-or-later
#![expect(
    missing_docs,
    reason = "error variants are self-describing in Display/JSON-RPC; exhaustive per-variant docs deferred."
)]
//! Typed error hierarchy for `coral-glowplug`.
//!
//! Replaces `String` errors with structured variants that carry context
//! (BDF address, driver name, sysfs path) for diagnostics and machine
//! consumption via JSON-RPC error responses.

use std::fmt;
use std::sync::Arc;

/// Errors from device lifecycle operations.
#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error("unknown personality '{personality}' for {bdf} (known: {known:?})")]
    UnknownPersonality {
        bdf: Arc<str>,
        personality: String,
        known: Vec<&'static str>,
    },

    #[error("VFIO open failed for {bdf}: {reason}")]
    VfioOpen { bdf: Arc<str>, reason: String },

    #[error("driver bind failed for {bdf} → {driver}: {reason}")]
    DriverBind {
        bdf: Arc<str>,
        driver: String,
        reason: String,
    },

    #[error("device {bdf} not managed")]
    NotManaged { bdf: Arc<str> },

    #[error("sysfs I/O error at {path}: {source}")]
    SysfsIo {
        path: String,
        source: std::io::Error,
    },

    #[error(
        "device {bdf} has active DRM consumers — unbinding would crash the kernel. \
         Close all GPU-using applications on this card first."
    )]
    ActiveDrmConsumers { bdf: Arc<str> },
}

/// Sysfs write failures for [`crate::sysfs_ops::SysfsOps::sysfs_write`].
#[derive(Debug, thiserror::Error)]
pub enum SysfsError {
    #[error("sysfs I/O error at {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },

    /// Mutex poison in test doubles (`MockSysfs`).
    #[cfg(test)]
    #[error("mock sysfs writes mutex poisoned: {detail}")]
    MockWritesMutexPoisoned { detail: String },
}

/// Failed to bind the JSON-RPC socket (TCP or Unix).
#[derive(Debug, thiserror::Error)]
pub enum SocketServerError {
    #[error("bind TCP {addr}: {source}")]
    BindTcp {
        addr: String,
        source: std::io::Error,
    },

    #[error("get TCP local addr: {source}")]
    TcpLocalAddr { source: std::io::Error },

    #[error("bind Unix {path}: {source}")]
    BindUnix {
        path: String,
        source: std::io::Error,
    },

    #[error("Unix socket path not supported on this platform; use TCP address (e.g. {fallback})")]
    UnixNotSupported { fallback: String },
}

/// Integer parse failures for CLI helpers (hex or decimal).
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("invalid hex '{input}': {source}")]
    Hex {
        input: String,
        source: std::num::ParseIntError,
    },

    #[error("invalid number '{input}': {source}")]
    Dec {
        input: String,
        source: std::num::ParseIntError,
    },
}

/// No config file in the search list could be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigLoadError {
    /// Paths that were attempted, in order.
    pub paths: Vec<String>,
}

impl fmt::Display for ConfigLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "no valid configuration found (tried: {})",
            self.paths.join(", ")
        )
    }
}

impl std::error::Error for ConfigLoadError {}

/// Blocking CUDA / base64 steps for `device.dispatch`.
#[derive(Debug, thiserror::Error)]
pub enum ComputeDispatchError {
    #[error("base64 decode shader: {0}")]
    ShaderBase64(#[from] base64::DecodeError),

    #[error("base64 decode input: {0}")]
    InputBase64(base64::DecodeError),

    #[error("CUDA compute dispatch requires building coral-glowplug with `--features cuda`")]
    CudaFeatureDisabled,

    #[error("CUDA open for {bdf}: {message}")]
    CudaOpen { bdf: String, message: String },

    #[error("alloc input: {message}")]
    AllocInput { message: String },

    #[error("upload: {message}")]
    Upload { message: String },

    #[error("alloc output: {message}")]
    AllocOutput { message: String },

    #[error("dispatch: {message}")]
    Dispatch { message: String },

    #[error("sync: {message}")]
    Sync { message: String },

    #[error("readback: {message}")]
    Readback { message: String },
}

impl From<ComputeDispatchError> for RpcError {
    fn from(err: ComputeDispatchError) -> Self {
        Self {
            code: RpcErrorCode::DEVICE_ERROR,
            message: err.to_string(),
        }
    }
}

/// Errors from configuration loading and parsing.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config {path}: {source}")]
    ReadFailed {
        path: String,
        source: std::io::Error,
    },

    #[error("failed to parse config {path}: {source}")]
    ParseFailed {
        path: String,
        source: toml::de::Error,
    },
}

/// JSON-RPC error codes aligned with the JSON-RPC 2.0 specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RpcErrorCode(i32);

impl RpcErrorCode {
    pub const PARSE_ERROR: Self = Self(-32700);
    pub const INVALID_REQUEST: Self = Self(-32600);
    pub const METHOD_NOT_FOUND: Self = Self(-32601);
    pub const INVALID_PARAMS: Self = Self(-32602);
    pub const INTERNAL_ERROR: Self = Self(-32603);
    pub const DEVICE_ERROR: Self = Self(-32000);
}

impl fmt::Display for RpcErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<RpcErrorCode> for i32 {
    fn from(code: RpcErrorCode) -> Self {
        code.0
    }
}

/// Unified RPC dispatch error carrying a JSON-RPC error code.
#[derive(Debug)]
pub struct RpcError {
    pub code: RpcErrorCode,
    pub message: String,
}

impl RpcError {
    #[must_use]
    pub fn invalid_params(msg: impl Into<String>) -> Self {
        Self {
            code: RpcErrorCode::INVALID_PARAMS,
            message: msg.into(),
        }
    }

    #[must_use]
    pub fn device_error(msg: impl Into<String>) -> Self {
        Self {
            code: RpcErrorCode::DEVICE_ERROR,
            message: msg.into(),
        }
    }

    #[must_use]
    pub fn method_not_found(method: &str) -> Self {
        Self {
            code: RpcErrorCode::METHOD_NOT_FOUND,
            message: format!("method not found: {method}"),
        }
    }

    #[must_use]
    pub fn internal(msg: impl Into<String>) -> Self {
        Self {
            code: RpcErrorCode::INTERNAL_ERROR,
            message: msg.into(),
        }
    }
}

impl fmt::Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for RpcError {}

/// Errors from the `EmberClient` IPC layer.
#[derive(Debug, thiserror::Error)]
pub enum EmberError {
    #[error("ember socket connect failed: {0}")]
    Connect(std::io::Error),

    #[error("ember I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ember JSON-RPC parse error: {0}")]
    Parse(#[from] serde_json::Error),

    #[error("ember RPC error ({code}): {message}")]
    Rpc { code: i32, message: String },

    #[error("SCM_RIGHTS: expected {expected} fds, got {received}")]
    FdCount { expected: usize, received: usize },
}

impl From<DeviceError> for RpcError {
    fn from(err: DeviceError) -> Self {
        Self {
            code: RpcErrorCode::DEVICE_ERROR,
            message: err.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn device_error_display_unknown_personality() {
        let err = DeviceError::UnknownPersonality {
            bdf: Arc::from("0000:01:00.0"),
            personality: "vfio-pci".into(),
            known: vec!["nvidia", "amdgpu"],
        };
        assert_eq!(
            err.to_string(),
            "unknown personality 'vfio-pci' for 0000:01:00.0 (known: [\"nvidia\", \"amdgpu\"])"
        );
    }

    #[test]
    fn device_error_display_vfio_open() {
        let err = DeviceError::VfioOpen {
            bdf: Arc::from("0000:02:00.0"),
            reason: "permission denied".into(),
        };
        assert_eq!(
            err.to_string(),
            "VFIO open failed for 0000:02:00.0: permission denied"
        );
    }

    #[test]
    fn device_error_display_driver_bind() {
        let err = DeviceError::DriverBind {
            bdf: Arc::from("0000:03:00.0"),
            driver: "vfio-pci".into(),
            reason: "device busy".into(),
        };
        assert_eq!(
            err.to_string(),
            "driver bind failed for 0000:03:00.0 → vfio-pci: device busy"
        );
    }

    #[test]
    fn device_error_display_not_managed() {
        let err = DeviceError::NotManaged {
            bdf: Arc::from("0000:04:00.0"),
        };
        assert_eq!(err.to_string(), "device 0000:04:00.0 not managed");
    }

    #[test]
    fn device_error_display_sysfs_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
        let err = DeviceError::SysfsIo {
            path: "/sys/bus/pci/devices/0000:01:00.0/driver".into(),
            source: io_err,
        };
        assert!(
            err.to_string()
                .contains("sysfs I/O error at /sys/bus/pci/devices/0000:01:00.0/driver")
        );
        assert!(err.to_string().contains("no such file"));
    }

    #[test]
    fn config_error_display_read_failed() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "config not found");
        let err = ConfigError::ReadFailed {
            path: "/etc/coral/config.toml".into(),
            source: io_err,
        };
        assert!(
            err.to_string()
                .contains("failed to read config /etc/coral/config.toml")
        );
        assert!(err.to_string().contains("config not found"));
    }

    #[test]
    fn config_error_display_parse_failed() {
        let parse_err = toml::from_str::<toml::Value>("invalid = toml = here").unwrap_err();
        let err = ConfigError::ParseFailed {
            path: "/etc/coral/config.toml".into(),
            source: parse_err,
        };
        assert!(
            err.to_string()
                .contains("failed to parse config /etc/coral/config.toml")
        );
    }

    #[test]
    fn rpc_error_code_constants() {
        assert_eq!(RpcErrorCode::PARSE_ERROR.0, -32700);
        assert_eq!(RpcErrorCode::INVALID_REQUEST.0, -32600);
        assert_eq!(RpcErrorCode::METHOD_NOT_FOUND.0, -32601);
        assert_eq!(RpcErrorCode::INVALID_PARAMS.0, -32602);
        assert_eq!(RpcErrorCode::INTERNAL_ERROR.0, -32603);
        assert_eq!(RpcErrorCode::DEVICE_ERROR.0, -32000);
    }

    #[test]
    fn rpc_error_code_display() {
        assert_eq!(RpcErrorCode::INVALID_PARAMS.to_string(), "-32602");
        assert_eq!(RpcErrorCode::DEVICE_ERROR.to_string(), "-32000");
    }

    #[test]
    fn rpc_error_code_into_i32() {
        assert_eq!(i32::from(RpcErrorCode::INVALID_PARAMS), -32602);
        assert_eq!(i32::from(RpcErrorCode::METHOD_NOT_FOUND), -32601);
    }

    #[test]
    fn rpc_error_invalid_params() {
        let err = RpcError::invalid_params("missing bdf");
        assert_eq!(err.code, RpcErrorCode::INVALID_PARAMS);
        assert_eq!(err.message, "missing bdf");
    }

    #[test]
    fn rpc_error_device_error() {
        let err = RpcError::device_error("VFIO open failed");
        assert_eq!(err.code, RpcErrorCode::DEVICE_ERROR);
        assert_eq!(err.message, "VFIO open failed");
    }

    #[test]
    fn rpc_error_method_not_found() {
        let err = RpcError::method_not_found("bind_device");
        assert_eq!(err.code, RpcErrorCode::METHOD_NOT_FOUND);
        assert_eq!(err.message, "method not found: bind_device");
    }

    #[test]
    fn rpc_error_internal() {
        let err = RpcError::internal("unexpected state");
        assert_eq!(err.code, RpcErrorCode::INTERNAL_ERROR);
        assert_eq!(err.message, "unexpected state");
    }

    #[test]
    fn rpc_error_display() {
        let err = RpcError::invalid_params("bad request");
        assert_eq!(err.to_string(), "[-32602] bad request");
    }

    #[test]
    fn from_device_error_to_rpc_error() {
        let dev_err = DeviceError::NotManaged {
            bdf: Arc::from("0000:01:00.0"),
        };
        let rpc_err: RpcError = dev_err.into();
        assert_eq!(rpc_err.code, RpcErrorCode::DEVICE_ERROR);
        assert_eq!(rpc_err.message, "device 0000:01:00.0 not managed");
    }

    #[test]
    fn rpc_error_impls_std_error() {
        fn assert_error<E: std::error::Error>() {}
        assert_error::<RpcError>();
    }

    #[test]
    fn device_error_display_active_drm_consumers() {
        let err = DeviceError::ActiveDrmConsumers {
            bdf: Arc::from("0000:05:00.0"),
        };
        let s = err.to_string();
        assert!(s.contains("0000:05:00.0"));
        assert!(s.contains("DRM"));
    }

    #[test]
    fn rpc_error_code_parse_and_invalid_request_display() {
        assert_eq!(RpcErrorCode::PARSE_ERROR.to_string(), "-32700");
        assert_eq!(RpcErrorCode::INVALID_REQUEST.to_string(), "-32600");
    }

    #[test]
    fn ember_error_display_connect() {
        let io = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        let err = EmberError::Connect(io);
        assert!(err.to_string().contains("ember socket connect"));
        assert!(err.to_string().contains("refused"));
    }

    #[test]
    fn ember_error_display_io() {
        let err = EmberError::Io(std::io::Error::other("disk full"));
        assert!(err.to_string().contains("ember I/O"));
    }

    #[test]
    fn ember_error_display_parse() {
        let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let err = EmberError::Parse(json_err);
        assert!(err.to_string().contains("ember JSON-RPC parse"));
    }

    #[test]
    fn ember_error_display_rpc() {
        let err = EmberError::Rpc {
            code: -32000,
            message: "boom".into(),
        };
        assert!(err.to_string().contains("-32000"));
        assert!(err.to_string().contains("boom"));
    }

    #[test]
    fn ember_error_display_fd_count() {
        let err = EmberError::FdCount {
            expected: 3,
            received: 1,
        };
        assert!(err.to_string().contains("SCM_RIGHTS"));
        assert!(err.to_string().contains('3'));
        assert!(err.to_string().contains('1'));
    }

    #[test]
    fn ember_error_from_io() {
        let io_err = std::io::Error::other("test");
        let err: EmberError = io_err.into();
        assert!(matches!(err, EmberError::Io(_)));
    }

    #[test]
    fn ember_error_from_json() {
        let json_err = serde_json::from_str::<serde_json::Value>("x").unwrap_err();
        let err: EmberError = json_err.into();
        assert!(matches!(err, EmberError::Parse(_)));
    }

    #[test]
    fn sysfs_error_io_display_includes_path_and_source() {
        let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let err = SysfsError::Io {
            path: "/sys/class/foo".into(),
            source: io,
        };
        let s = err.to_string();
        assert!(s.contains("/sys/class/foo"));
        assert!(s.contains("denied"));
    }

    #[test]
    fn sysfs_error_mock_writes_mutex_poisoned_display() {
        let err = SysfsError::MockWritesMutexPoisoned {
            detail: "poison detail".into(),
        };
        let s = err.to_string();
        assert!(s.contains("mock sysfs"));
        assert!(s.contains("poison detail"));
    }

    #[test]
    fn socket_server_error_bind_tcp_display() {
        let err = SocketServerError::BindTcp {
            addr: "127.0.0.1:9".into(),
            source: std::io::Error::other("address already in use"),
        };
        assert!(err.to_string().contains("127.0.0.1:9"));
        assert!(err.to_string().contains("bind TCP"));
    }

    #[test]
    fn socket_server_error_tcp_local_addr_display() {
        let err = SocketServerError::TcpLocalAddr {
            source: std::io::Error::other("not bound"),
        };
        assert!(err.to_string().contains("get TCP local addr"));
    }

    #[test]
    fn socket_server_error_bind_unix_display() {
        let err = SocketServerError::BindUnix {
            path: "/run/coral.sock".into(),
            source: std::io::Error::other("permission denied"),
        };
        assert!(err.to_string().contains("/run/coral.sock"));
        assert!(err.to_string().contains("bind Unix"));
    }

    #[test]
    fn socket_server_error_unix_not_supported_display() {
        let err = SocketServerError::UnixNotSupported {
            fallback: "127.0.0.1:0".into(),
        };
        let s = err.to_string();
        assert!(s.contains("Unix socket path not supported"));
        assert!(s.contains("127.0.0.1:0"));
    }

    #[test]
    fn parse_error_hex_display() {
        let source = u64::from_str_radix("zz", 16).unwrap_err();
        let err = ParseError::Hex {
            input: "0xzz".into(),
            source,
        };
        assert!(err.to_string().contains("invalid hex"));
        assert!(err.to_string().contains("0xzz"));
    }

    #[test]
    fn parse_error_dec_display() {
        let source = "not-a-number".parse::<u64>().unwrap_err();
        let err = ParseError::Dec {
            input: "not-a-number".into(),
            source,
        };
        assert!(err.to_string().contains("invalid number"));
    }

    #[test]
    fn config_load_error_display_joins_paths() {
        let err = ConfigLoadError {
            paths: vec!["/first.toml".into(), "/second.toml".into()],
        };
        assert_eq!(
            err.to_string(),
            "no valid configuration found (tried: /first.toml, /second.toml)"
        );
    }

    #[test]
    fn compute_dispatch_error_shader_base64_display() {
        use base64::Engine;
        let decode_err = base64::engine::general_purpose::STANDARD
            .decode("!!!")
            .unwrap_err();
        let err = ComputeDispatchError::ShaderBase64(decode_err);
        assert!(err.to_string().contains("base64 decode shader"));
    }

    #[test]
    fn compute_dispatch_error_input_base64_display() {
        use base64::Engine;
        let decode_err = base64::engine::general_purpose::STANDARD
            .decode("@@@")
            .unwrap_err();
        let err = ComputeDispatchError::InputBase64(decode_err);
        assert!(err.to_string().contains("base64 decode input"));
    }

    #[test]
    fn compute_dispatch_error_cuda_feature_disabled_display() {
        assert_eq!(
            ComputeDispatchError::CudaFeatureDisabled.to_string(),
            "CUDA compute dispatch requires building coral-glowplug with `--features cuda`"
        );
    }

    #[test]
    fn compute_dispatch_error_message_variants_display() {
        let e = ComputeDispatchError::CudaOpen {
            bdf: "0000:01:00.0".into(),
            message: "no device".into(),
        };
        assert!(e.to_string().contains("CUDA open"));
        assert!(e.to_string().contains("0000:01:00.0"));

        assert!(
            ComputeDispatchError::AllocInput {
                message: "oom".into()
            }
            .to_string()
            .contains("alloc input")
        );
        assert!(
            ComputeDispatchError::Upload {
                message: "fail".into()
            }
            .to_string()
            .contains("upload")
        );
        assert!(
            ComputeDispatchError::AllocOutput {
                message: "fail".into()
            }
            .to_string()
            .contains("alloc output")
        );
        assert!(
            ComputeDispatchError::Dispatch {
                message: "bad".into()
            }
            .to_string()
            .contains("dispatch")
        );
        assert!(
            ComputeDispatchError::Sync {
                message: "timeout".into()
            }
            .to_string()
            .contains("sync")
        );
        assert!(
            ComputeDispatchError::Readback {
                message: "zero".into()
            }
            .to_string()
            .contains("readback")
        );
    }

    #[test]
    fn compute_dispatch_error_converts_to_rpc_error() {
        let rpc: RpcError = ComputeDispatchError::CudaFeatureDisabled.into();
        assert_eq!(rpc.code, RpcErrorCode::DEVICE_ERROR);
        assert!(rpc.message.contains("CUDA"));
    }
}
