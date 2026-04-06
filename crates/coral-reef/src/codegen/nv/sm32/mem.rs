// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2025)
//! SM32 memory instruction encoders.

use super::encoder::*;
use crate::codegen::ir::RegFile;

pub(super) fn legalize_ext_instr(op: &mut impl SrcsAsSlice, _b: &mut LegalizeBuilder) {
    let src_types = op.src_types();
    for (i, src) in op.srcs_as_mut_slice().iter_mut().enumerate() {
        match src_types[i] {
            SrcType::SSA => {
                assert!(src.as_ssa().is_some());
            }
            SrcType::GPR => {
                assert!(src_is_reg(src, RegFile::GPR));
            }
            SrcType::ALU
            | SrcType::F16
            | SrcType::F16v2
            | SrcType::F32
            | SrcType::F64
            | SrcType::I32
            | SrcType::B32 => {
                crate::codegen::ice!("ALU srcs must be legalized explicitly");
            }
            SrcType::Pred => {
                crate::codegen::ice!("Predicates must be legalized explicitly");
            }
            SrcType::Carry => {
                crate::codegen::ice!("Carry values must be legalized explicitly");
            }
            SrcType::Bar => crate::codegen::ice!("Barrier regs are Volta+"),
        }
    }
}

impl SM32Encoder<'_> {
    pub(super) fn set_mem_type(&mut self, range: Range<usize>, mem_type: MemType) {
        assert!(range.len() == 3);
        self.set_field(
            range,
            match mem_type {
                MemType::U8 => 0_u8,
                MemType::I8 => 1_u8,
                MemType::U16 => 2_u8,
                MemType::I16 => 3_u8,
                MemType::B32 => 4_u8,
                MemType::B64 => 5_u8,
                MemType::B128 => 6_u8,
            },
        );
    }

    pub(super) fn set_mem_access(&mut self, range: Range<usize>, access: &MemAccess) {
        self.set_field(
            range.start..range.start + 1,
            match access.space.addr_type() {
                MemAddrType::A32 => 0_u8,
                MemAddrType::A64 => 1_u8,
            },
        );
        self.set_mem_type(range.start + 1..range.end, access.mem_type);
        // order and scope: not present before SM70 (omitted).
    }
}

impl SM32Op for OpLd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        assert_eq!(self.stride, OffsetStride::X1);
        // Missing:
        // 0x7c8 for indirect const load
        match self.access.space {
            MemSpace::Global(_) => {
                e.set_opcode(0xc00, 0);

                e.set_field(23..55, self.offset);
                e.set_mem_access(55..59, &self.access);
                e.set_ld_cache_op(59..61, self.access.ld_cache_op(e.sm));
            }
            MemSpace::Local | MemSpace::Shared => {
                let opc = match self.access.space {
                    MemSpace::Local => 0x7a0,
                    MemSpace::Shared => 0x7a4,
                    MemSpace::Global(_) => unreachable!(),
                };
                e.set_opcode(opc, 2);

                e.set_field(23..47, self.offset);
                e.set_ld_cache_op(47..49, self.access.ld_cache_op(e.sm));
                e.set_mem_access(50..54, &self.access);
            }
        }
        e.set_dst(&self.dst);
        e.set_reg_src(10..18, &self.addr);
    }
}

impl SM32Op for OpLdc {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(self.offset_mut(), GPR, SrcType::GPR);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        assert!(self.cb().modifier.is_none());
        let SrcRef::CBuf(cb) = &self.cb().reference else {
            crate::codegen::ice!("Not a CBuf source");
        };
        let CBuf::Binding(cb_idx) = cb.buf else {
            crate::codegen::ice!("Must be a bound constant buffer");
        };

        e.set_opcode(0x7c8, 2);

        e.set_dst(&self.dst);
        e.set_reg_src(10..18, self.offset());
        e.set_field(23..39, cb.offset);
        e.set_field(39..44, cb_idx);
        e.set_field(
            47..49,
            match self.mode {
                LdcMode::Indexed => 0_u8,
                LdcMode::IndexedLinear => 1_u8,
                LdcMode::IndexedSegmented => 2_u8,
                LdcMode::IndexedSegmentedLinear => 3_u8,
            },
        );
        e.set_mem_type(51..54, self.mem_type);
    }
}

impl SM32Op for OpLdSharedLock {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x774, 2);
        e.set_dst(self.dst());
        e.set_reg_src(10..18, &self.addr);
        e.set_field(23..47, self.offset);

        e.set_pred_dst(48..51, self.locked());
        e.set_mem_type(51..54, self.mem_type);
    }
}

impl SM32Op for OpSt {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        assert_eq!(self.stride, OffsetStride::X1);
        match self.access.space {
            MemSpace::Global(_) => {
                e.set_opcode(0xe00, 0);

                e.set_field(23..55, self.offset);
                e.set_mem_access(55..59, &self.access);
                e.set_st_cache_op(59..61, self.access.st_cache_op(e.sm));
            }
            MemSpace::Local | MemSpace::Shared => {
                let opc = match self.access.space {
                    MemSpace::Local => 0x7a8,
                    MemSpace::Shared => 0x7ac,
                    MemSpace::Global(_) => unreachable!(),
                };
                e.set_opcode(opc, 2);

                e.set_field(23..47, self.offset);
                e.set_st_cache_op(47..49, self.access.st_cache_op(e.sm));
                e.set_mem_access(50..54, &self.access);
            }
        }
        e.set_reg_src(2..10, self.data());
        e.set_reg_src(10..18, self.addr());
    }
}

impl SM32Op for OpStSCheckUnlock {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x784, 2);

        e.set_reg_src(2..10, self.data());
        e.set_reg_src(10..18, self.addr());

        e.set_field(23..47, self.offset);
        e.set_st_cache_op(47..49, StCacheOp::WriteBack);
        e.set_pred_dst(48..51, &self.locked);
        e.set_mem_type(51..54, self.mem_type);
    }
}

pub(super) fn atom_src_as_ssa(b: &mut LegalizeBuilder, src: &Src, atom_type: AtomType) -> SSARef {
    if let Some(ssa) = src.as_ssa() {
        return ssa.clone();
    }

    if atom_type.bits() == 32 {
        let tmp = b.alloc_ssa(RegFile::GPR);
        b.copy_to(tmp.into(), 0.into());
        tmp.into()
    } else {
        debug_assert!(atom_type.bits() == 64);
        let tmp = b.alloc_ssa_vec(RegFile::GPR, 2);
        b.copy_to(tmp[0].into(), 0.into());
        b.copy_to(tmp[1].into(), 0.into());
        tmp
    }
}

impl SM32Encoder<'_> {
    pub(super) fn set_atom_op(&mut self, range: Range<usize>, atom_op: AtomOp) {
        self.set_field(
            range,
            match atom_op {
                AtomOp::Add => 0_u8,
                AtomOp::Min => 1_u8,
                AtomOp::Max => 2_u8,
                AtomOp::Inc => 3_u8,
                AtomOp::Dec => 4_u8,
                AtomOp::And => 5_u8,
                AtomOp::Or => 6_u8,
                AtomOp::Xor => 7_u8,
                AtomOp::Exch => 8_u8,
                AtomOp::CmpExch(_) => crate::codegen::ice!("CmpExch is a separate opcode"),
                // SafeAdd: 0xa_u8 (unused; CmpExch is separate opcode).
            },
        );
    }
}

impl SM32Op for OpAtom {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        if self.atom_op == AtomOp::CmpExch(AtomCmpSrc::Separate) {
            let cmpr = atom_src_as_ssa(b, self.cmpr(), self.atom_type);
            let data = atom_src_as_ssa(b, self.data(), self.atom_type);

            let mut cmpr_data = Vec::new();
            cmpr_data.extend_from_slice(&cmpr);
            cmpr_data.extend_from_slice(&data);
            let cmpr_data = SSARef::try_from(cmpr_data).expect("cmpr+data must form valid SSARef");

            *self.cmpr_mut() = 0.into();
            *self.data_mut() = cmpr_data.into();
            self.atom_op = AtomOp::CmpExch(AtomCmpSrc::Packed);
        }
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        assert_eq!(self.addr_stride, OffsetStride::X1);
        match self.mem_space {
            MemSpace::Global(addr_type) => {
                if let AtomOp::CmpExch(cmp_src) = self.atom_op {
                    assert!(!self.dst.is_none());
                    e.set_opcode(0x778, 2);
                    e.set_dst(&self.dst);

                    // CmpExch: separate layout in disassembler; only packed layout
                    // appears supported by real hardware (same as SM50).
                    let (data_src, data_layout) = match cmp_src {
                        AtomCmpSrc::Separate => {
                            if self.data().is_zero() {
                                (self.cmpr(), 1_u8)
                            } else {
                                assert!(self.cmpr().is_zero());
                                (self.data(), 2_u8)
                            }
                        }
                        AtomCmpSrc::Packed => (self.data(), 0_u8),
                    };
                    e.set_reg_src(23..31, data_src);
                    let data_type = match self.atom_type {
                        AtomType::U32 => 0_u8,
                        AtomType::U64 => 1_u8,
                        _ => crate::codegen::ice!("Unsupported data type"),
                    };
                    e.set_field(52..53, data_type);
                    e.set_field(53..55, data_layout);
                } else {
                    e.set_opcode(0x680, 2);
                    e.set_dst(&self.dst);
                    e.set_reg_src(23..31, self.data());

                    let data_type = match self.atom_type {
                        AtomType::U32 => 0_u8,
                        AtomType::I32 => 1_u8,
                        AtomType::U64 => 2_u8,
                        AtomType::F32 => 3_u8,
                        // NOTE: U128 => 4_u8,
                        AtomType::I64 => 5_u8,
                        _ => crate::codegen::ice!("Unsupported data type"),
                    };
                    e.set_field(52..55, data_type);

                    e.set_atom_op(55..59, self.atom_op);
                }
                // mem_order: encoding not present before SM70 (omitted).

                e.set_reg_src(10..18, self.addr());
                e.set_field(31..51, self.addr_offset);

                e.set_field(
                    51..52,
                    match addr_type {
                        MemAddrType::A32 => 0_u8,
                        MemAddrType::A64 => 1_u8,
                    },
                );
            }
            MemSpace::Local => crate::codegen::ice!("Atomics do not support local"),
            MemSpace::Shared => {
                crate::codegen::ice!(
                    "Shared atomics should be lowered into ld-locked and st-locked"
                )
            }
        }
    }
}

impl SM32Op for OpAL2P {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x7d0, 2);

        e.set_dst(&self.dst);
        e.set_reg_src(10..18, &self.offset);
        e.set_field(23..34, self.addr);
        e.set_bit(35, self.output);

        assert!(self.comps == 1);
        e.set_field(50..52, 0_u8); // comps
    }
}

impl SM32Op for OpALd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x7ec, 2);

        e.set_dst(&self.dst);
        e.set_reg_src(10..18, self.offset());
        e.set_field(23..34, self.addr);

        if self.phys {
            assert!(!self.patch);
            assert!(self.offset().reference.as_reg().is_some());
        } else if !self.patch {
            assert!(self.offset().is_zero());
        }
        e.set_bit(34, self.patch);
        e.set_bit(35, self.output);
        e.set_reg_src(42..50, self.vtx());

        e.set_field(50..52, self.comps - 1);
    }
}

impl SM32Op for OpASt {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x7f0, 2);

        e.set_reg_src(2..10, self.data());
        e.set_reg_src(10..18, self.offset());
        e.set_field(23..34, self.addr);

        if self.phys {
            assert!(!self.patch);
            assert!(self.offset().reference.as_reg().is_some());
        } else if !self.patch {
            assert!(self.offset().is_zero());
        }
        e.set_bit(34, self.patch);
        e.set_reg_src(42..50, self.vtx());

        e.set_field(50..52, self.comps - 1);
    }
}

impl SM32Op for OpIpa {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x748, 2);

        e.set_dst(&self.dst);
        e.set_reg_src(42..50, self.offset());
        e.set_reg_src(23..31, self.inv_w());

        assert!(self.addr % 4 == 0);
        e.set_field(31..42, self.addr);

        e.set_reg_src_ref(10..18, &SrcRef::Zero); // indirect addr

        e.set_bit(50, false); // .SAT
        e.set_field(
            51..53,
            match self.loc {
                InterpLoc::Default => 0_u8,
                InterpLoc::Centroid => 1_u8,
                InterpLoc::Offset => 2_u8,
            },
        );
        e.set_field(
            53..55,
            match self.freq {
                InterpFreq::Pass => 0_u8,
                InterpFreq::PassMulW => 1_u8,
                InterpFreq::Constant => 2_u8,
                InterpFreq::State => 3_u8,
            },
        );
    }
}

#[cfg(test)]
#[path = "mem_tests.rs"]
mod tests;
