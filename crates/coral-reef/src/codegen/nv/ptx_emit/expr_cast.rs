// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt::Write as _;

use crate::error::CompileError;

use super::PtxEmitter;
use super::types::PtxVal;

impl PtxEmitter<'_> {
    pub(super) fn eval_cast(
        &mut self,
        val: &PtxVal,
        kind: naga::ScalarKind,
        convert: Option<u8>,
        inner_handle: naga::Handle<naga::Expression>,
    ) -> Result<PtxVal, CompileError> {
        let src_scalar = self.scalar_of(inner_handle);
        let dst_width = convert.unwrap_or(src_scalar.width);
        let dst_scalar = naga::Scalar {
            kind,
            width: dst_width,
        };
        let src_ts = Self::ptx_type_suffix(src_scalar);
        let dst_ts = Self::ptx_type_suffix(dst_scalar);

        if src_ts == dst_ts {
            return Ok(val.clone());
        }

        if let PtxVal::Vec(components) = val {
            let mut results = Vec::with_capacity(components.len());
            for c in components {
                let dst = self.alloc_for_scalar(dst_scalar);
                writeln!(
                    self.body,
                    "    cvt.{dst_ts}.{src_ts} {}, {};",
                    dst.fmt_operand(),
                    c.fmt_operand(),
                )
                .expect("write to String");
                results.push(dst);
            }
            return Ok(PtxVal::Vec(results));
        }

        let dst = self.alloc_for_scalar(dst_scalar);
        writeln!(
            self.body,
            "    cvt.{dst_ts}.{src_ts} {}, {};",
            dst.fmt_operand(),
            val.fmt_operand(),
        )
        .expect("write to String");
        Ok(dst)
    }

    pub(super) fn ensure_pred(&mut self, val: &PtxVal) -> Result<PtxVal, CompileError> {
        match val {
            PtxVal::Pred(_) => Ok(val.clone()),
            PtxVal::R32(_) => {
                let p = self.alloc_pred();
                writeln!(
                    self.body,
                    "    setp.ne.u32 {}, {}, 0;",
                    p.fmt_operand(),
                    val.fmt_operand(),
                )
                .expect("write to String");
                Ok(p)
            }
            _ => Err(CompileError::NotImplemented(
                "PTX: cannot convert to predicate".into(),
            )),
        }
    }
}
