// SPDX-License-Identifier: AGPL-3.0-or-later
//! Version selection and high-level builders.

use crate::DispatchDims;
use crate::nv::generation::{GenerationProfile, QmdVersion};

use super::field::qmd_set_field_dyn;
use super::types::{QMD_SIZE_WORDS, QmdParams};
use super::v21_v22::{build_qmd_v21, build_qmd_v22};
use super::v23::build_qmd_v23;
use super::v30::build_qmd_v30;
use super::v50::build_qmd_v50;

/// Select the appropriate QMD builder for a given SM architecture.
///
/// Returns a `Vec<u32>` — 64 words (256 bytes) for SM < 100,
/// 96 words (384 bytes) for SM 100+ (Blackwell v5.0).
///
/// Blackwell (SM 100+) requires QMD v5.0, a completely different layout
/// with shifted addresses, separate VALID bits, and SASS_VERSION. NVK
/// (Mesa) confirms v5.0 is mandatory for Blackwell CTS compliance.
#[must_use]
pub fn build_qmd_for_sm(sm: u32, params: &QmdParams) -> Vec<u32> {
    let profile = crate::nv::generation::profile_for_sm(sm);
    build_qmd(profile, params)
}

/// Build a QMD using the generation profile's QMD version.
///
/// Preferred over `build_qmd_for_sm` when a profile is already available.
#[must_use]
pub fn build_qmd(profile: &GenerationProfile, params: &QmdParams) -> Vec<u32> {
    match profile.qmd_version {
        QmdVersion::V21 => build_qmd_v21(params).to_vec(),
        QmdVersion::V22 => build_qmd_v22(params).to_vec(),
        QmdVersion::V23 => build_qmd_v23(params).to_vec(),
        QmdVersion::V30 => build_qmd_v30(params).to_vec(),
        QmdVersion::V50 => build_qmd_v50_with_sm(params, *profile.sm_range.start()),
    }
}

/// Encode the SM version as a SASS_VERSION byte for QMD v5.0.
///
/// NVIDIA uses `(major << 4) | minor` — e.g. SM 8.9 = 0x89, SM 12.0 = 0xC0.
/// Our internal SM numbering is `major * 10 + minor`, so SM 120 → 12.0.
#[must_use]
const fn sm_to_sass_version(sm: u32) -> u64 {
    let major = sm / 10;
    let minor = sm % 10;
    ((major << 4) | minor) as u64
}

/// Build QMD v5.0 with the correct SASS_VERSION for the target SM.
fn build_qmd_v50_with_sm(params: &QmdParams, sm: u32) -> Vec<u32> {
    let mut q = build_qmd_v50(params);
    // SASS_VERSION MW(455:448) — 8 bits
    qmd_set_field_dyn(&mut q, 448, 8, sm_to_sass_version(sm));
    q
}

/// Legacy builder — wraps `build_qmd_v30` with minimal params.
///
/// Preserved for backward compatibility with existing tests.
#[must_use]
pub fn build_compute_qmd(
    shader_va: u64,
    dims: DispatchDims,
    _code_size: u32,
) -> [u32; QMD_SIZE_WORDS] {
    let params = QmdParams::simple(shader_va, dims, 16);
    build_qmd_v30(&params)
}
