// SPDX-License-Identifier: AGPL-3.0-or-later
//! Ray query PTX emission for SM75+ (Turing and later RT cores).
//!
//! SM75+ inline ray tracing requires vendor-proprietary RT core ISA that is
//! not publicly documented. All five `RayQueryFunction` variants return
//! `CompileError::NotImplemented` until proper inline RT emission is possible.
//!
//! Shaders that use `rayQuery*` functions will fail at compile time with a
//! clear error rather than silently producing incorrect traversal results.

use crate::error::CompileError;

use super::PtxEmitter;

impl PtxEmitter<'_> {
    pub(super) fn emit_ray_query(
        &self,
        _query: naga::Handle<naga::Expression>,
        fun: &naga::RayQueryFunction,
    ) -> Result<(), CompileError> {
        if self.sm < 75 {
            return Err(CompileError::InvalidInput(
                "RayQuery requires SM75+ (Turing or later) for RT core access".into(),
            ));
        }

        let variant = match *fun {
            naga::RayQueryFunction::Initialize { .. } => "Initialize",
            naga::RayQueryFunction::Proceed { .. } => "Proceed",
            naga::RayQueryFunction::GenerateIntersection { .. } => "GenerateIntersection",
            naga::RayQueryFunction::ConfirmIntersection => "ConfirmIntersection",
            naga::RayQueryFunction::Terminate => "Terminate",
        };

        Err(CompileError::NotImplemented(
            format!(
                "RayQuery::{variant} — SM75+ inline ray tracing requires vendor RT core ISA \
                 that is not yet publicly documented. Use OptiX or Vulkan ray tracing pipelines \
                 for RT workloads until inline RT emission is implemented."
            )
            .into(),
        ))
    }
}
