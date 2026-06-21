// SPDX-License-Identifier: AGPL-3.0-or-later
//! Compiler service — shared logic for both JSON-RPC and tarpc transports.
//!
//! Follows wateringHole semantic method naming: `shader.compile.{operation}`.

mod compile;
pub mod types;

pub use compile::{
    handle_compile, handle_compile_gemm, handle_compile_spirv, handle_compile_wgsl,
    handle_compile_wgsl_multi,
};
pub use types::{
    CapabilityListResponse, CompileCapabilitiesResponse, CompileRequest, CompileResponse,
    CompileWgslRequest, F64TranscendentalCapabilities, GemmCompileRequest, HealthCheckResponse,
    HealthResponse, IdentityGetResponse, LivenessResponse, MultiDeviceCompileRequest,
    MultiDeviceCompileResponse, ReadinessResponse, VersionResponse,
};
#[cfg(feature = "tarpc-transport")]
pub use types::{CompileSpirvRequestTarpc, TarpcCompileError};

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::sync::OnceLock;

use crate::capability::SelfDescription;
use crate::config;
use coral_reef::{AmdArch, NvArch};

static IDENTITY_ADVERTISED: OnceLock<IdentityGetResponse> = OnceLock::new();
static STARTUP_INSTANT: OnceLock<std::time::Instant> = OnceLock::new();

/// Store the primal identity for `identity.get` after IPC binds (full transports).
///
/// If not called, [`handle_identity_get`] returns [`IdentityGetResponse::fallback`].
pub fn set_identity_for_ipc(identity: IdentityGetResponse) {
    let _ = IDENTITY_ADVERTISED.set(identity);
}

/// Build identity from a bound [`SelfDescription`] and publish for JSON-RPC.
pub fn set_identity_from_self_description(desc: &SelfDescription) {
    set_identity_for_ipc(IdentityGetResponse {
        name: config::PRIMAL_NAME.into(),
        version: config::PRIMAL_VERSION.into(),
        provides: desc.provides.clone(),
        requires: desc.requires.clone(),
        transports: desc.transports.clone(),
    });
}

/// `identity.get` — return this primal's self-description for ecosystem discovery.
#[must_use]
pub fn handle_identity_get() -> IdentityGetResponse {
    IDENTITY_ADVERTISED
        .get()
        .cloned()
        .unwrap_or_else(IdentityGetResponse::fallback)
}

/// Re-export from config — single source of truth accessible without cfg gates.
pub use crate::config::SERVED_METHODS;

/// `capability.list` — Wire Standard L3 method inventory plus domain discovery.
///
/// Includes advertised [`crate::capability::Capability`] ids plus JSON-RPC namespaces
/// exposed by this binary (`health.*`, `identity.get`), and the flat `methods` list
/// required by `CAPABILITY_WIRE_STANDARD`. L3 adds `protocol` and `transport` fields.
#[must_use]
pub fn handle_capability_list() -> CapabilityListResponse {
    let desc = crate::capability::self_description();
    let mut domains: BTreeSet<String> = desc.provides.iter().map(|c| c.id.to_string()).collect();
    domains.insert("auth".into());
    domains.insert("health".into());
    domains.insert("identity".into());

    let methods = SERVED_METHODS.iter().map(|&s| s.into()).collect();

    let transport: Vec<Cow<'static, str>> = {
        let mut t = vec![Cow::Borrowed("tcp"), Cow::Borrowed("tarpc")];
        #[cfg(unix)]
        t.insert(0, Cow::Borrowed("uds"));
        t
    };

    CapabilityListResponse {
        primal: config::PRIMAL_NAME.into(),
        version: config::PRIMAL_VERSION.into(),
        protocol: "jsonrpc-2.0".into(),
        transport,
        methods,
        capabilities: domains.into_iter().collect(),
    }
}

/// Generate a health response listing all supported architectures.
#[must_use]
pub fn handle_health() -> HealthResponse {
    let mut archs: Vec<String> = NvArch::ALL.iter().map(ToString::to_string).collect();
    archs.extend(AmdArch::ALL.iter().map(ToString::to_string));
    HealthResponse {
        name: config::PRIMAL_NAME.into(),
        version: config::PRIMAL_VERSION.into(),
        status: "operational".into(),
        supported_archs: archs,
    }
}

/// `shader.compile.capabilities` — structured capability report.
///
/// Reports both supported architectures AND f64 transcendental lowering
/// capabilities. Callers use this to decide whether to route transcendental-
/// heavy shaders through the sovereign compiler (polyfill) vs native driver.
#[must_use]
pub fn handle_compile_capabilities() -> CompileCapabilitiesResponse {
    let health = handle_health();
    CompileCapabilitiesResponse {
        supported_archs: health.supported_archs,
        f64_transcendentals: F64TranscendentalCapabilities {
            sin: true,
            cos: true,
            sqrt: true,
            exp2: true,
            log2: true,
            rcp: true,
            exp: true,
            log: true,
            composite_lowering: true,
        },
        math_ops: Some(34),
        sm_target: NvArch::ALL.last().map(ToString::to_string),
        atomics: Some(true),
        subgroup_ops: Some(true),
    }
}

/// `health.check` — full health check per wateringHole standard.
///
/// Probes internal subsystems and returns a detailed health report.
#[must_use]
pub fn handle_health_check() -> HealthCheckResponse {
    let health = handle_health();
    let is_healthy = health.status == "operational";
    HealthCheckResponse {
        name: health.name,
        version: health.version,
        healthy: is_healthy,
        status: health.status,
        supported_archs: health.supported_archs,
        family_id: config::family_id().into(),
    }
}

/// `health.liveness` — lightweight liveness probe.
///
/// Returns `{"status":"alive"}` per `DEPLOYMENT_BEHAVIOR_STANDARD`.
#[must_use]
pub fn handle_health_liveness() -> LivenessResponse {
    LivenessResponse {
        status: "alive".into(),
    }
}

/// `health.readiness` — readiness probe for accepting work.
///
/// Checks whether the compiler is initialized and ready to serve
/// compilation requests. May return false during startup.
#[must_use]
pub fn handle_health_readiness() -> ReadinessResponse {
    let ready = STARTUP_INSTANT.get().is_some();
    ReadinessResponse {
        ready,
        name: config::PRIMAL_NAME.into(),
    }
}

/// `health.version` — build identity for post-upgrade verification.
///
/// Returns session label, build hash, and version so callers can confirm
/// which binary is running without parsing `--version` CLI output.
#[must_use]
pub fn handle_health_version() -> VersionResponse {
    VersionResponse {
        session: config::PRIMAL_SESSION.into(),
        build_hash: config::PRIMAL_BUILD_HASH.into(),
        version: config::PRIMAL_VERSION.into(),
        name: config::PRIMAL_NAME.into(),
    }
}

/// Record the process startup instant for uptime reporting.
pub fn mark_startup() {
    let _ = STARTUP_INSTANT.get_or_init(std::time::Instant::now);
}

/// `health` — standard guideStone health response (bare method).
///
/// Returns `{status, primal, version, uptime_s}` per ecosystem HEALTH-01 schema.
#[must_use]
pub fn handle_health_standard() -> serde_json::Value {
    let uptime_s = STARTUP_INSTANT.get().map_or(0, |t| t.elapsed().as_secs());
    serde_json::json!({
        "status": "alive",
        "primal": config::PRIMAL_NAME,
        "version": config::PRIMAL_VERSION,
        "uptime_s": uptime_s,
    })
}

#[cfg(test)]
mod tests;
