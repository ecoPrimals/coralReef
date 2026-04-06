// SPDX-License-Identifier: AGPL-3.0-or-later
//! Compiler service — shared logic for both JSON-RPC and tarpc transports.
//!
//! Follows wateringHole semantic method naming: `shader.compile.{operation}`.

mod compile;
pub mod types;

pub use compile::{
    handle_compile, handle_compile_spirv, handle_compile_wgsl, handle_compile_wgsl_multi,
};
pub use types::{
    CapabilityListResponse, CompileCapabilitiesResponse, CompileRequest, CompileResponse,
    CompileSpirvRequestTarpc, CompileWgslRequest, F64TranscendentalCapabilities,
    HealthCheckResponse, HealthResponse, IdentityGetResponse, LivenessResponse,
    MultiDeviceCompileRequest, MultiDeviceCompileResponse, ReadinessResponse,
};

use std::collections::BTreeSet;
use std::sync::OnceLock;

use crate::capability::SelfDescription;
use crate::config;
use coral_reef::{AmdArch, NvArch};

static IDENTITY_ADVERTISED: OnceLock<IdentityGetResponse> = OnceLock::new();

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

/// `capability.list` — capability domains this primal serves (wateringHole discovery).
///
/// Includes advertised [`crate::capability::Capability`] ids plus JSON-RPC namespaces
/// exposed by this binary (`health.*`, `identity.get`).
#[must_use]
pub fn handle_capability_list() -> CapabilityListResponse {
    let desc = crate::capability::self_description();
    let mut domains: BTreeSet<String> = desc.provides.iter().map(|c| c.id.to_string()).collect();
    domains.insert("health".into());
    domains.insert("identity".into());
    CapabilityListResponse {
        capabilities: domains.into_iter().collect(),
        version: config::PRIMAL_VERSION.into(),
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
/// Returns true if the process is alive and responsive (no deep checks).
#[must_use]
pub const fn handle_health_liveness() -> LivenessResponse {
    LivenessResponse { alive: true }
}

/// `health.readiness` — readiness probe for accepting work.
///
/// Checks whether the compiler is initialized and ready to serve
/// compilation requests. May return false during startup.
#[must_use]
pub fn handle_health_readiness() -> ReadinessResponse {
    ReadinessResponse {
        ready: true,
        name: config::PRIMAL_NAME.into(),
    }
}

#[cfg(test)]
mod tests;
