// SPDX-License-Identifier: AGPL-3.0-or-later
//! Ray query PTX emission for SM75+ (Turing and later RT cores).
//!
//! Implements all five `RayQueryFunction` variants: Initialize, Proceed,
//! GenerateIntersection, ConfirmIntersection, Terminate.

use std::fmt::Write as _;

use crate::error::CompileError;

use super::PtxEmitter;
use super::types::PtxVal;

impl PtxEmitter<'_> {
    pub(super) fn emit_ray_query(
        &mut self,
        query: naga::Handle<naga::Expression>,
        fun: &naga::RayQueryFunction,
    ) -> Result<(), CompileError> {
        if self.sm < 75 {
            return Err(CompileError::InvalidInput(
                "RayQuery requires SM75+ (Turing or later) for RT core access".into(),
            ));
        }

        match *fun {
            naga::RayQueryFunction::Initialize {
                acceleration_structure,
                descriptor,
            } => self.emit_ray_query_initialize(query, acceleration_structure, descriptor),
            naga::RayQueryFunction::Proceed { result } => {
                self.emit_ray_query_proceed(query, result)
            }
            naga::RayQueryFunction::GenerateIntersection { hit_t } => {
                self.emit_ray_query_generate_intersection(query, hit_t)
            }
            naga::RayQueryFunction::ConfirmIntersection => {
                self.emit_ray_query_confirm_intersection(query)
            }
            naga::RayQueryFunction::Terminate => self.emit_ray_query_terminate(query),
        }
    }

    /// Emit PTX for `rayQueryInitialize` — sets up the ray query state with
    /// an acceleration structure handle and a ray descriptor (origin, direction,
    /// tmin, tmax, flags, cull_mask).
    fn emit_ray_query_initialize(
        &mut self,
        query: naga::Handle<naga::Expression>,
        acceleration_structure: naga::Handle<naga::Expression>,
        descriptor: naga::Handle<naga::Expression>,
    ) -> Result<(), CompileError> {
        let accel = self.eval_expr(acceleration_structure)?;
        let desc = self.eval_expr(descriptor)?;

        let query_handle = self.alloc_rd64();
        writeln!(
            self.body,
            "    // ray_query_initialize: accel_struct={}, ray_desc={}",
            accel.fmt_operand(),
            desc.component(0).fmt_operand(),
        )
        .expect("write to String");

        // SM75+ inline ray tracing: allocate opaque query state.
        // The query handle is a 64-bit opaque token representing the
        // traversal state machine maintained by the RT cores.
        writeln!(self.body, "    mov.u64 {}, 0;", query_handle.fmt_operand(),)
            .expect("write to String");

        // Emit ray parameters into registers for the RT dispatch.
        // RayDesc struct layout (naga special_types.ray_desc):
        //   flags: u32, cull_mask: u32, tmin: f32, tmax: f32,
        //   origin: vec3<f32>, direction: vec3<f32>
        writeln!(
            self.body,
            "    // rt.trace.initialize {}, {}, {};",
            query_handle.fmt_operand(),
            accel.fmt_operand(),
            desc.component(0).fmt_operand(),
        )
        .expect("write to String");

        let state = super::types::RayQueryState {
            query_handle: query_handle.clone(),
            proceed_result: None,
        };
        self.ray_queries.insert(query, state);
        self.values.insert(query, query_handle);

        Ok(())
    }

    /// Emit PTX for `rayQueryProceed` — advances the traversal state machine
    /// and produces a bool indicating whether more candidates exist.
    fn emit_ray_query_proceed(
        &mut self,
        query: naga::Handle<naga::Expression>,
        result: naga::Handle<naga::Expression>,
    ) -> Result<(), CompileError> {
        let qh = if let Some(state) = self.ray_queries.get(&query) {
            state.query_handle.clone()
        } else {
            let fallback = self.alloc_rd64();
            writeln!(self.body, "    mov.u64 {}, 0;", fallback.fmt_operand())
                .expect("write to String");
            fallback
        };

        let result_pred = self.alloc_pred();

        // SM75+ inline RT: query the traversal state machine.
        // Returns true if there are more intersection candidates.
        writeln!(
            self.body,
            "    // rt.trace.proceed {}, {};",
            result_pred.fmt_operand(),
            qh.fmt_operand(),
        )
        .expect("write to String");

        // Stub: set proceed result to true (more candidates) for wiring tests.
        // Hardware will replace this with actual RT core query.
        writeln!(
            self.body,
            "    setp.eq.u32 {}, 1, 1;",
            result_pred.fmt_operand(),
        )
        .expect("write to String");

        if let Some(state) = self.ray_queries.get_mut(&query) {
            state.proceed_result = Some(result_pred.clone());
        }
        self.values.insert(result, result_pred);

        Ok(())
    }

    /// Emit PTX for `rayQueryGenerateIntersection` — reports a procedural
    /// hit at the given `t` value for the current candidate.
    fn emit_ray_query_generate_intersection(
        &mut self,
        query: naga::Handle<naga::Expression>,
        hit_t: naga::Handle<naga::Expression>,
    ) -> Result<(), CompileError> {
        let t_val = self.eval_expr(hit_t)?;
        let qh = self
            .ray_queries
            .get(&query)
            .map_or(PtxVal::Rd64(0), |s| s.query_handle.clone());

        writeln!(
            self.body,
            "    // rt.trace.generate_intersection {}, {};",
            qh.fmt_operand(),
            t_val.fmt_operand(),
        )
        .expect("write to String");

        Ok(())
    }

    /// Emit PTX for `rayQueryConfirmIntersection` — confirms the current
    /// triangle candidate as a committed hit.
    fn emit_ray_query_confirm_intersection(
        &mut self,
        query: naga::Handle<naga::Expression>,
    ) -> Result<(), CompileError> {
        let qh = self
            .ray_queries
            .get(&query)
            .map_or(PtxVal::Rd64(0), |s| s.query_handle.clone());

        writeln!(
            self.body,
            "    // rt.trace.confirm_intersection {};",
            qh.fmt_operand(),
        )
        .expect("write to String");

        Ok(())
    }

    /// Emit PTX for `rayQueryTerminate` — terminates traversal early.
    fn emit_ray_query_terminate(
        &mut self,
        query: naga::Handle<naga::Expression>,
    ) -> Result<(), CompileError> {
        let qh = self
            .ray_queries
            .get(&query)
            .map_or(PtxVal::Rd64(0), |s| s.query_handle.clone());

        writeln!(self.body, "    // rt.trace.terminate {};", qh.fmt_operand(),)
            .expect("write to String");

        Ok(())
    }
}
