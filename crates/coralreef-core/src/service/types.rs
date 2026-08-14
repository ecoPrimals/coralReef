// SPDX-License-Identifier: AGPL-3.0-or-later
//! Request and response types for the compiler service.
//!
//! Separated from handler logic for clarity. All types are `Serialize` +
//! `Deserialize` so they work over both JSON-RPC and tarpc transports.
//!
//! Shader source strings use `Arc<str>` for zero-copy sharing across pipeline
//! stages per wateringHole standards.

use crate::capability::{Capability, Transport};
use bytes::Bytes;
use serde::{Deserialize, Deserializer, Serialize};
use std::borrow::Cow;
use std::sync::Arc;

/// Deserialize a string from JSON into `Arc<str>` for zero-copy sharing.
fn deserialize_arc_str<'de, D>(deserializer: D) -> Result<Arc<str>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Ok(Arc::from(s.into_boxed_str()))
}

/// tarpc-only SPIR-V compile request (zero-copy via `Bytes`).
///
/// Uses `bytes::Bytes` so SPIR-V can be shared without copying over the wire.
/// Serializes as base64 when using JSON transport.
#[cfg(feature = "tarpc-transport")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileSpirvRequestTarpc {
    /// Raw SPIR-V bytes (zero-copy).
    pub spirv: Bytes,
    /// Target GPU architecture name (e.g. `sm_70`, `rdna2`).
    pub arch: String,
    /// Optimization level (0-3).
    pub opt_level: u32,
    /// Enable f64 software transcendentals.
    pub fp64_software: bool,
}

/// Request to compile a shader (JSON-RPC wire format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileRequest {
    /// SPIR-V words (JSON array of u32; base64 in tarpc uses [`CompileSpirvRequestTarpc`]).
    pub spirv_words: Vec<u32>,
    /// Target GPU architecture name (e.g. `sm70`, `sm86`, `rdna2`). Optional; defaults to sm70.
    #[serde(default = "default_arch")]
    pub arch: String,
    /// Optimization level (0-3).
    #[serde(default = "default_opt_level")]
    pub opt_level: u32,
    /// Enable f64 software transcendentals.
    #[serde(default)]
    pub fp64_software: bool,
}

/// Request to compile WGSL source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileWgslRequest {
    /// WGSL source code (shared via `Arc<str>` across pipeline stages).
    ///
    /// Canonical wire name: `"wgsl_source"`. Accepts `"source"` as alias
    /// for callers using the shorthand form.
    #[serde(alias = "source", deserialize_with = "deserialize_arc_str")]
    pub wgsl_source: Arc<str>,
    /// Target GPU architecture name (e.g. `sm70`, `sm86`, `rdna2`). Optional; defaults to sm70.
    #[serde(default = "default_arch")]
    pub arch: String,
    /// Optimization level (0-3).
    #[serde(default = "default_opt_level")]
    pub opt_level: u32,
    /// Enable f64 software transcendentals.
    #[serde(default)]
    pub fp64_software: bool,
    /// f64 strategy hint from the caller (e.g. `"software"`, `"native"`).
    /// Optional — defaults to using `fp64_software` if absent.
    #[serde(default)]
    pub fp64_strategy: Option<String>,
    /// FMA fusion policy hint (e.g. `"fused"`, `"separate"`, `"auto"`).
    /// Optional — defaults to `"auto"` (compiler decides).
    #[serde(default)]
    pub fma_policy: Option<String>,
    /// Precision routing advice from the caller's precision routing layer.
    /// Tells the compiler which precision tier was selected and whether
    /// hardware-specific lowering is needed.
    #[serde(default)]
    pub precision_advice: Option<PrecisionAdvice>,
    /// Adapter descriptor for arch-agnostic compilation. When present, the
    /// compiler infers the ISA target from adapter hardware identity rather
    /// than requiring the caller to know the exact architecture string.
    #[serde(default)]
    pub adapter: Option<AdapterDescriptor>,
    /// When `true`, additionally emit portable SPIR-V binary alongside
    /// the native binary. The SPIR-V is returned in `spirv_binary` field
    /// of the response. Defaults to `false`.
    #[serde(default)]
    pub emit_spirv: bool,
    /// SPIR-V version to target as `[major, minor]` (e.g. `[1, 5]`).
    /// Only used when `emit_spirv` is `true`. Defaults to `[1, 3]`.
    #[serde(default)]
    pub spirv_version: Option<[u8; 2]>,
}

/// Request to compile WGSL source directly to SPIR-V binary (no native ISA).
///
/// Dedicated endpoint for consumers that need portable SPIR-V for driver
/// passthrough (e.g. barracuda's DF64 streaming pipelines). Unlike
/// `CompileWgslRequest` with `emit_spirv: true`, this skips native binary
/// compilation entirely — lower latency when only SPIR-V is needed.
///
/// The `fma_policy` field is critical for DF64 consumers: `"never_fuse"`
/// preserves Dekker 2-sum arithmetic integrity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileWgslToSpirvRequest {
    /// WGSL source code.
    #[serde(alias = "source", deserialize_with = "deserialize_arc_str")]
    pub wgsl_source: Arc<str>,
    /// FMA fusion policy: `"allow_all"`, `"never_fuse"`, `"skip_df64_functions"`.
    ///
    /// - `"allow_all"`: FMA fusion enabled (fastest, not DF64-safe).
    /// - `"never_fuse"`: No FMA fusion anywhere (safest for DF64).
    /// - `"skip_df64_functions"`: Skip fusion in functions matching DF64 naming
    ///   patterns (`df64_*`, `two_sum`, `two_prod`, `split_f32`, etc.).
    #[serde(default = "default_fma_policy_spirv")]
    pub fma_policy: String,
    /// Function names to explicitly exclude from FMA fusion.
    /// Augments pattern-based detection when `fma_policy` is `"skip_df64_functions"`.
    #[serde(default)]
    pub no_fuse_functions: Vec<String>,
    /// SPIR-V version to target as `[major, minor]`. Defaults to `[1, 3]`.
    #[serde(default)]
    pub spirv_version: Option<[u8; 2]>,
    /// Enable f64 software transcendentals in the emitted SPIR-V.
    #[serde(default)]
    pub fp64_software: bool,
}

fn default_fma_policy_spirv() -> String {
    "never_fuse".into()
}

/// Response from WGSL-to-SPIR-V compilation.
///
/// Contains the SPIR-V binary as `Vec<u32>` words (not bytes) for direct
/// consumption by `create_shader_module_passthrough`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileWgslToSpirvResponse {
    /// SPIR-V words (u32 array). Feed directly to Vulkan's
    /// `VkShaderModuleCreateInfo::pCode`.
    pub spirv_words: Vec<u32>,
    /// Compilation status.
    pub status: Cow<'static, str>,
    /// Wall-clock compilation time in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compile_time_ms: Option<f64>,
    /// Entry point names found in the module.
    #[serde(default)]
    pub entry_points: Vec<String>,
    /// FMA policy that was actually applied.
    pub applied_fma_policy: String,
    /// Number of functions where FMA fusion was skipped.
    #[serde(default)]
    pub fma_skipped_functions: u32,
}

/// Precision routing advice carried in compile requests.
///
/// Enables the compiler to make informed decisions about hardware unit
/// targeting (tensor cores vs ALU) and transcendental lowering strategy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrecisionAdvice {
    /// The precision tier selected by the caller (e.g. `"F16"`, `"F64"`, `"DF64"`).
    pub tier: String,
    /// Whether hardware native f64 transcendentals are broken (probed by caller).
    #[serde(default)]
    pub needs_transcendental_lowering: bool,
    /// Whether DF64 (f32-pair) transcendentals are poisoned by naga.
    #[serde(default)]
    pub df64_naga_poisoned: bool,
    /// Physics domain that motivated this compilation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
}

/// Adapter hardware descriptor for architecture-agnostic compilation.
///
/// When provided, the compiler maps the adapter identity to the appropriate
/// ISA target. This avoids encoding GPU generation knowledge in every consumer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterDescriptor {
    /// PCI vendor ID (e.g. `0x10DE` for NVIDIA, `0x1002` for AMD).
    pub vendor_id: u32,
    /// Adapter name as reported by the GPU driver.
    pub device_name: String,
    /// Device type (`"DiscreteGpu"`, `"IntegratedGpu"`, `"Cpu"`).
    #[serde(default)]
    pub device_type: String,
}

/// Response from shader compilation (Compute Trio wire contract).
///
/// Uses `bytes::Bytes` for zero-copy IPC payloads — `Bytes::from(Vec<u8>)`
/// takes ownership of the allocation without copying.
///
/// Includes [`CompilationInfoResponse`] so dispatch and routing callers can
/// construct QMD / dispatch descriptors without re-parsing the binary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileResponse {
    /// Compiled GPU binary, base64-encoded on the wire.
    ///
    /// Canonical wire name: `"binary_b64"`. Accepts `"binary"` as alias
    /// for callers using the legacy field name.
    #[serde(rename = "binary_b64", alias = "binary")]
    pub binary: Bytes,
    /// Size in bytes.
    pub size: usize,
    /// Target architecture the binary was compiled for.
    #[serde(rename = "target", default)]
    pub arch: Option<String>,
    /// Compilation status (e.g. `"success"`, `"partial"`).
    #[serde(default)]
    pub status: Option<Cow<'static, str>>,
    /// Compilation metadata for dispatch descriptor construction.
    ///
    /// Canonical wire name: `"shader_info"`. Accepts `"info"` as alias
    /// for callers using the legacy field name.
    #[serde(rename = "shader_info", alias = "info", default)]
    pub info: Option<CompilationInfoResponse>,
    /// Wall-clock compilation time in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compile_time_ms: Option<f64>,
    /// Dispatch routing hints for the caller's submission layer.
    /// Tells the `compute.dispatch` provider which hardware unit the binary targets.
    #[serde(default)]
    pub dispatch_hints: Option<DispatchHints>,
    /// Sovereign SPIR-V binary (GAP-HS-124), base64-encoded on the wire.
    /// Contains valid SPIR-V (magic `0x07230203`) for Vulkan passthrough
    /// dispatch. Present only for WGSL compile paths; absent for direct
    /// SPIR-V input and GEMM compiles.
    ///
    /// No `skip_serializing_if` — bincode (tarpc transport) is positional
    /// and breaks when fields are conditionally omitted.
    #[serde(default)]
    pub spirv_binary: Option<Bytes>,
    /// Artifact provenance for Dark Forest trust validation.
    ///
    /// Includes content hash, gate identity, and compiler version so
    /// downstream consumers can verify artifact integrity without
    /// re-compiling. Signature field is populated when a crypto-domain
    /// provider is available for BTSP artifact signing.
    #[serde(default)]
    pub provenance: Option<ArtifactProvenance>,
}

/// Provenance metadata for compiled shader artifacts.
///
/// Implements Dark Forest Invariant 3: no unsigned artifacts cross trust
/// boundaries. The `content_hash` field is always populated; the `signature`
/// field requires a crypto-domain provider for BTSP artifact signing.
///
/// The `sporeprint_hash` field carries a BLAKE3 content hash for Nest
/// provenance integration — content-addressed storage can index artifacts
/// by this hash without re-hashing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactProvenance {
    /// SHA-256 hash of the compiled binary (hex-encoded).
    pub content_hash: String,
    /// Algorithm used for the content hash.
    #[serde(default = "default_hash_algorithm")]
    pub hash_algorithm: String,
    /// BLAKE3 hash of the compiled binary (hex-encoded) for Nest provenance.
    ///
    /// Content-addressed storage (CAS) uses BLAKE3 as the canonical hash for
    /// artifact identity. Dual-hashing avoids re-computation at the storage layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sporeprint_hash: Option<String>,
    /// Gate that performed the compilation.
    pub gate_of_compilation: String,
    /// Compiler identity and version.
    pub compiler_version: String,
    /// BTSP signature over the content hash, if crypto-domain signing is available.
    /// Hex-encoded Ed25519 or HMAC signature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// Key identifier used for the signature (for key rotation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
}

fn default_hash_algorithm() -> String {
    "sha256".into()
}

impl CompileResponse {
    /// Attach provenance metadata for cross-gate trust validation.
    ///
    /// Called by the JSON-RPC dispatch layer before sending responses over
    /// trust boundaries. Attempts to sign via a discovered `crypto.sign`
    /// provider; degrades to unsigned provenance if unavailable.
    ///
    /// Not used in the tarpc path (intra-gate, already trusted).
    #[must_use]
    #[allow(
        dead_code,
        reason = "provenance attachment for cross-gate JSON-RPC compile responses"
    )]
    pub fn with_provenance(mut self) -> Self {
        self.provenance = Some(super::provenance::build_provenance(&self.binary));
        self
    }
}

/// Hints for the dispatch layer about hardware unit targeting.
///
/// Returned alongside the compiled binary so the dispatch caller can route
/// to the correct hardware unit without parsing the binary.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DispatchHints {
    /// Hardware unit the compiled binary targets.
    /// Values: `"compute"`, `"tensor_core"`, `"rt_core"`, `"npu"`, `"cpu"`.
    pub hardware_hint: Cow<'static, str>,
    /// Binary format: `"ptx"`, `"sass"`, `"isa"`, `"cranelift"`, `"dataflow_graph"`.
    #[serde(default)]
    pub binary_format: Option<Cow<'static, str>>,
    /// Execution model: `"simt"` (GPU), `"sequential"` (CPU), `"dataflow"` (NPU).
    #[serde(default)]
    pub execution_model: Option<Cow<'static, str>>,
}

/// Compilation metadata needed by the dispatch layer (`compute.dispatch` provider).
///
/// Maps 1:1 from the compiler's internal `CompilationInfo`. Serialized as
/// part of every `CompileResponse` so callers can build GPU dispatch
/// descriptors (QMD, PM4) without re-analyzing the binary.
///
/// Wire field names follow the Compute Trio contract (`gprs`, `shared_memory`,
/// `barriers`, `workgroup`, `wave_size`, `local_memory`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompilationInfoResponse {
    /// General-purpose registers used by the shader.
    #[serde(rename = "gprs")]
    pub gpr_count: u32,
    /// Instructions emitted.
    pub instr_count: u32,
    /// Shared memory in bytes (from `var<workgroup>`).
    #[serde(rename = "shared_memory")]
    pub shared_mem_bytes: u32,
    /// Number of barriers used.
    #[serde(rename = "barriers")]
    pub barrier_count: u32,
    /// Workgroup dimensions from `@workgroup_size(x, y, z)`.
    #[serde(rename = "workgroup")]
    pub workgroup_size: [u32; 3],
    /// Wave/warp size: 32 for NVIDIA, 32 or 64 for AMD.
    pub wave_size: u32,
    /// Per-thread local (scratch) memory in bytes.
    pub local_memory: u32,
}

/// `capability.list` response — Wire Standard Level 2 compliance.
///
/// Per wateringHole `CAPABILITY_WIRE_STANDARD` v1.0: the response MUST
/// contain `primal`, `version`, and `methods` (flat string array of every
/// callable JSON-RPC method).
///
/// Also includes `capabilities` for backward compatibility with existing
/// ecosystem consumers that expect domain-level discovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityListResponse {
    /// Canonical primal name (lowercase, no spaces).
    pub primal: Cow<'static, str>,
    /// Primal semantic version.
    pub version: Cow<'static, str>,
    /// Wire protocol identifier (Wire Standard L3).
    pub protocol: Cow<'static, str>,
    /// Supported transport layers (Wire Standard L3).
    pub transport: Vec<Cow<'static, str>>,
    /// Every callable JSON-RPC method (Wire Standard L2 routing signal).
    pub methods: Vec<String>,
    /// Capability domain strings (backward compat with domain-level discovery).
    pub capabilities: Vec<String>,
}

/// `identity.get` response — primal self-description for capability-based discovery.
///
/// Per wateringHole `CAPABILITY_BASED_DISCOVERY_STANDARD`: name, version, capability
/// lists, and bound transports after servers listen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityGetResponse {
    /// Primal name (from crate / config).
    pub name: Cow<'static, str>,
    /// Semantic version.
    pub version: Cow<'static, str>,
    /// Capabilities this primal provides.
    pub provides: Vec<Capability>,
    /// Capabilities required from peers.
    pub requires: Vec<Capability>,
    /// IPC transports (populated after bind).
    pub transports: Vec<Transport>,
}

impl IdentityGetResponse {
    /// Minimal identity when full advertisement is not yet available.
    #[must_use]
    pub fn fallback() -> Self {
        let desc = crate::capability::self_description();
        Self {
            name: crate::config::PRIMAL_NAME.into(),
            version: crate::config::PRIMAL_VERSION.into(),
            provides: desc.provides,
            requires: desc.requires,
            transports: Vec::new(),
        }
    }
}

/// Health check response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    /// Primal name.
    pub name: Cow<'static, str>,
    /// Version.
    pub version: Cow<'static, str>,
    /// Current status.
    pub status: Cow<'static, str>,
    /// Supported architectures.
    pub supported_archs: Vec<String>,
}

/// Structured capability report for `shader.compile.capabilities`.
///
/// Carries architecture support AND f64 transcendental capability metadata,
/// enabling callers to make informed routing decisions (no blind routing).
///
/// Wire field `targets` (renamed from `supported_archs`) satisfies Compute
/// Trio Gate 1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileCapabilitiesResponse {
    /// Supported GPU architectures (e.g. `["sm_70", "sm_86", "rdna2"]`).
    #[serde(rename = "targets")]
    pub supported_archs: Vec<String>,
    /// f64 transcendental lowering capabilities — which ops the sovereign
    /// compiler can polyfill into pure f64 arithmetic (DFMA/DMUL/DADD).
    pub f64_transcendentals: F64TranscendentalCapabilities,
    /// Number of supported math operations in the PTX emitter path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub math_ops: Option<u32>,
    /// Primary SM target for PTX emission (e.g. `"sm_120"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sm_target: Option<String>,
    /// Whether atomic operations are supported in the PTX emitter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atomics: Option<bool>,
    /// Whether subgroup/warp primitives are supported in the PTX emitter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subgroup_ops: Option<bool>,
}

/// Per-operation f64 transcendental capabilities that the sovereign compiler
/// can provide via software lowering.
///
/// When `true`, the compiler can replace the named WGSL built-in with a
/// polynomial/Newton-Raphson software implementation using only basic f64
/// arithmetic, bypassing broken driver JIT (e.g. NVVM) entirely.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "1:1 map of f64 transcendental functions"
)]
pub struct F64TranscendentalCapabilities {
    /// sin(f64) via Cody-Waite range reduction + Chebyshev polynomial
    pub sin: bool,
    /// cos(f64) via Cody-Waite range reduction + Chebyshev polynomial
    pub cos: bool,
    /// sqrt(f64) via Newton-Raphson (DFMA convergence)
    pub sqrt: bool,
    /// exp2(f64) via range reduction + Horner polynomial
    pub exp2: bool,
    /// log2(f64) via range reduction + Horner polynomial
    pub log2: bool,
    /// rcp(f64) via Newton-Raphson (1/x)
    pub rcp: bool,
    /// exp(f64) via exp2(x * log2(e))
    pub exp: bool,
    /// log(f64) via log2(x) * ln(2)
    pub log: bool,
    /// `compile_mode: "f64_polyfill"` — all transcendentals lowered to
    /// pure f64 arithmetic. Use `fp64_strategy: "software"` in compile requests.
    pub composite_lowering: bool,
}

/// A single device target for multi-device compilation.
///
/// Carries an architecture hint and optional `PCIe` group ID so the caller
/// can request compilation for specific GPU slots in a multi-GPU system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceTarget {
    /// Card index (0-based, maps to `/dev/dri/renderD128+N`).
    #[serde(default)]
    pub card_index: u32,
    /// GPU architecture hint (e.g. `"sm_89"`, `"rdna2"`).
    pub arch: String,
    /// Optional `PCIe` group / switch affinity hint.
    #[serde(default)]
    pub pcie_group: Option<u32>,
}

/// Request to compile a single WGSL shader for multiple GPU targets at once.
///
/// Implements the `shader.compile.wgsl.multi` endpoint (ecosystem protocol S144)
/// handoff. Compiles the same shader source to native binaries for each
/// target device in a single request, enabling multi-GPU dispatch preparation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiDeviceCompileRequest {
    /// WGSL source code (shared via `Arc<str>` across all targets).
    ///
    /// Canonical wire name: `"wgsl_source"`. Accepts `"source"` as alias.
    #[serde(alias = "source", deserialize_with = "deserialize_arc_str")]
    pub wgsl_source: Arc<str>,
    /// Target devices to compile for.
    pub targets: Vec<DeviceTarget>,
    /// Optimization level (0-3).
    #[serde(default = "default_opt_level")]
    pub opt_level: u32,
    /// Enable f64 software transcendentals.
    #[serde(default)]
    pub fp64_software: bool,
    /// f64 strategy hint (e.g. `"software"`, `"native"`).
    #[serde(default)]
    pub fp64_strategy: Option<String>,
    /// FMA fusion policy hint (e.g. `"fused"`, `"separate"`, `"auto"`).
    #[serde(default)]
    pub fma_policy: Option<String>,
}

/// Result of compiling for a single device in a multi-device request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCompileResult {
    /// Card index this result corresponds to.
    pub card_index: u32,
    /// Architecture compiled for.
    pub arch: String,
    /// Compiled binary, base64-encoded on the wire, or `None` on failure.
    #[serde(rename = "binary_b64", skip_serializing_if = "Option::is_none")]
    pub binary: Option<Bytes>,
    /// Binary size in bytes (0 on failure).
    pub size: usize,
    /// Error message if compilation failed for this target.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Compilation metadata, or `None` on failure.
    #[serde(
        rename = "shader_info",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub info: Option<CompilationInfoResponse>,
}

/// Response from multi-device compilation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiDeviceCompileResponse {
    /// Per-device compilation results (same order as request `targets`).
    pub results: Vec<DeviceCompileResult>,
    /// Number of targets that compiled successfully.
    pub success_count: usize,
    /// Total number of targets requested.
    pub total_count: usize,
}

/// A single compilation job in a batch request.
///
/// Each job carries its own input source (WGSL, SPIR-V, or GLSL) and target
/// architecture, enabling mixed-input batch compilation in a single RPC call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchCompileJob {
    /// Input language: `"wgsl"`, `"spirv"`, or `"glsl"`.
    pub input_type: String,
    /// Source code (for WGSL / GLSL) or base64-encoded bytes (for SPIR-V).
    #[serde(deserialize_with = "deserialize_arc_str")]
    pub source: Arc<str>,
    /// Target GPU architecture (e.g. `"sm_70"`, `"sm_120"`, `"rdna2"`).
    #[serde(default = "default_arch")]
    pub arch: String,
    /// Optimization level (0-3).
    #[serde(default = "default_opt_level")]
    pub opt_level: u32,
    /// Enable f64 software transcendentals.
    #[serde(default)]
    pub fp64_software: bool,
    /// FMA fusion policy hint (e.g. `"fused"`, `"separate"`, `"auto"`).
    #[serde(default)]
    pub fma_policy: Option<String>,
    /// Caller-provided label for correlation (returned in the response).
    #[serde(default)]
    pub label: Option<String>,
}

/// Request for batch compilation of mixed-input shaders.
///
/// Implements `shader.compile.multi` — the generic batch compilation method.
/// Unlike `shader.compile.wgsl.multi` (same WGSL to multiple targets), this
/// accepts an array of independent jobs, each with its own input type, source,
/// and target architecture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchCompileRequest {
    /// Array of compilation jobs.
    pub jobs: Vec<BatchCompileJob>,
}

/// Result of a single job in a batch compilation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchCompileJobResult {
    /// Zero-based index of this job in the request.
    pub index: usize,
    /// Caller-provided label, echoed back for correlation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Compiled binary (base64-encoded on the wire), or absent on failure.
    #[serde(rename = "binary_b64", skip_serializing_if = "Option::is_none")]
    pub binary: Option<Bytes>,
    /// Binary size in bytes (0 on failure).
    pub size: usize,
    /// Target architecture compiled for.
    pub arch: String,
    /// Input type that was compiled.
    pub input_type: String,
    /// Error message if this job failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Compilation metadata for dispatch descriptor construction.
    #[serde(
        rename = "shader_info",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub info: Option<CompilationInfoResponse>,
    /// Wall-clock compilation time in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compile_time_ms: Option<f64>,
}

/// Response from batch compilation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchCompileResponse {
    /// Per-job compilation results (same order as request `jobs`).
    pub results: Vec<BatchCompileJobResult>,
    /// Number of jobs that compiled successfully.
    pub success_count: usize,
    /// Total number of jobs in the request.
    pub total_count: usize,
}

/// `health.check` response per wateringHole `PRIMAL_IPC_PROTOCOL` v3.0.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResponse {
    /// Primal name (self-knowledge only).
    pub name: Cow<'static, str>,
    /// Primal version.
    pub version: Cow<'static, str>,
    /// Whether the primal is healthy.
    pub healthy: bool,
    /// Human-readable status.
    pub status: Cow<'static, str>,
    /// Supported GPU architectures.
    pub supported_archs: Vec<String>,
    /// Family ID for multi-instance disambiguation.
    pub family_id: Cow<'static, str>,
}

/// `health.liveness` response — lightweight alive check.
///
/// Returns `{"status":"alive"}` per `DEPLOYMENT_BEHAVIOR_STANDARD`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LivenessResponse {
    /// Liveness status string. Always `"alive"` when the process is responsive.
    pub status: Cow<'static, str>,
}

/// `health.readiness` response — ready to accept work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadinessResponse {
    /// `true` if the primal is ready to serve requests.
    pub ready: bool,
    /// Primal name (self-knowledge).
    pub name: Cow<'static, str>,
}

/// `health.version` response — build identity for upgrade verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionResponse {
    /// Build session label (e.g., sprint identifier or release tag).
    pub session: Cow<'static, str>,
    /// Git commit hash or `"dev"` for local builds.
    pub build_hash: Cow<'static, str>,
    /// Semantic version from Cargo.toml.
    pub version: Cow<'static, str>,
    /// Primal name (self-knowledge).
    pub name: Cow<'static, str>,
}

/// `shader.compile.gemm` request — tensor-core GEMM kernel generation.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GemmCompileRequest {
    /// Matrix rows (M dimension).
    pub m: u32,
    /// Matrix columns (N dimension).
    pub n: u32,
    /// Inner/reduction dimension (K dimension).
    pub k: u32,
    /// Precision: "f16", "f16f32", or "tf32". Defaults to "f16f32".
    #[serde(default = "default_gemm_precision")]
    pub precision: String,
    /// Target GPU architecture (e.g., `sm_80`, `sm_120`).
    #[serde(default = "default_arch")]
    pub arch: String,
    /// Tiling strategy: "auto" (default), "global" (Phase 1), or "smem" (Phase 2).
    ///
    /// - `"auto"`: uses shared-memory tiling when dimensions are aligned to block
    ///   tile boundaries (M%64==0, N%16==0), otherwise falls back to global memory.
    /// - `"global"`: single-warp Phase 1 kernel (32 threads, no shared memory).
    /// - `"smem"`: multi-warp Phase 2 kernel (128 threads, ldmatrix + bar.sync).
    ///   Requires M%64==0 and N%16==0.
    #[serde(default = "default_gemm_tiling")]
    pub tiling: String,
}

/// Default GEMM tiling strategy for serde deserialization.
#[must_use]
fn default_gemm_tiling() -> String {
    "auto".into()
}

/// Default GEMM precision for serde deserialization.
#[must_use]
fn default_gemm_precision() -> String {
    "f16f32".into()
}

/// Default GPU architecture string for serde deserialization.
#[must_use]
pub fn default_arch() -> String {
    coral_reef::GpuArch::default().to_string()
}

/// Default optimization level for compilation requests.
#[must_use]
pub const fn default_opt_level() -> u32 {
    2
}

/// Serializable compilation error for tarpc transport.
///
/// `CompileError` (in `coral-reef`) does not derive `Serialize`/`Deserialize`
/// because it uses `Cow<'static, str>` and is a library error type.
/// This wrapper preserves the error message across the bincode wire while
/// providing a typed error rather than raw `String`.
#[cfg(feature = "tarpc-transport")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TarpcCompileError {
    /// Human-readable error message (from `CompileError::to_string()`).
    pub message: String,
}

#[cfg(feature = "tarpc-transport")]
impl std::fmt::Display for TarpcCompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

#[cfg(feature = "tarpc-transport")]
impl std::error::Error for TarpcCompileError {}

#[cfg(feature = "tarpc-transport")]
impl TarpcCompileError {
    /// Wrap any error into a tarpc-transportable error.
    pub fn from_error(e: impl std::fmt::Display) -> Self {
        Self {
            message: e.to_string(),
        }
    }
}

#[cfg(test)]
#[path = "types_identity_tests.rs"]
mod identity_tests;
