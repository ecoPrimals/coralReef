// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2025)
//! SM20 memory instruction encoders.

#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::encoder::*;

impl SM20Op for OpSuLdGa {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        assert!(e.sm.sm() >= 30);
        e.set_opcode(SM20Unit::Mem, 0x35);
        e.set_mem_type(5..8, self.mem_type);
        e.set_ld_cache_op(8..10, self.cache_op);
        e.set_dst(14..20, &self.dst);
        e.set_reg_src(20..26, &self.addr);
        assert!(self.format.modifier.is_none());
        match &self.format.reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_reg_src(26..32, &self.format);
                e.set_bit(53, false);
            }
            SrcRef::CBuf(cb) => {
                let CBuf::Binding(idx) = cb.buf else {
                    panic!("Must be a bound constant buffer");
                };
                assert!(cb.offset & 0x3 == 0);
                e.set_field(26..40, cb.offset >> 2);
                e.set_field(40..45, idx);
                e.set_bit(53, true);
            }
            _ => panic!("Invalid format source"),
        }
        e.set_su_ga_offset_mode(45..47, self.offset_mode);
        e.set_field(47..49, 0_u8);
        e.set_pred_src(49..53, &self.out_of_bounds);
    }
}

impl SM20Op for OpSuStGa {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        assert!(e.sm.sm() >= 30);
        e.set_opcode(SM20Unit::Mem, 0x37);
        match self.image_access {
            ImageAccess::Binary(mem_type) => {
                e.set_mem_type(5..8, mem_type);
                e.set_field(54..58, 0_u8);
            }
            ImageAccess::Formatted(channel_mask) => {
                e.set_field(54..58, channel_mask.to_bits());
            }
        }
        e.set_st_cache_op(8..10, self.cache_op);
        e.set_reg_src(14..20, &self.data);
        e.set_reg_src(20..26, &self.addr);
        assert!(self.format.modifier.is_none());
        match &self.format.reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_reg_src(26..32, &self.format);
                e.set_bit(53, false);
            }
            SrcRef::CBuf(cb) => {
                let CBuf::Binding(idx) = cb.buf else {
                    panic!("Must be a bound constant buffer");
                };
                assert!(cb.offset & 0x3 == 0);
                e.set_field(26..40, cb.offset >> 2);
                e.set_field(40..45, idx);
                e.set_bit(53, true);
            }
            _ => panic!("Invalid format source"),
        }
        e.set_su_ga_offset_mode(45..47, self.offset_mode);
        e.set_field(47..49, 0_u8);
        e.set_pred_src(49..53, &self.out_of_bounds);
    }
}

impl SM20Op for OpLd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        assert_eq!(self.stride, OffsetStride::X1);
        match self.access.space {
            MemSpace::Global(addr_type) => {
                e.set_opcode(SM20Unit::Mem, 0x20);
                e.set_field(26..58, self.offset);
                e.set_bit(58, addr_type == MemAddrType::A64);
            }
            MemSpace::Local => {
                e.set_opcode(SM20Unit::Mem, 0x30);
                e.set_bit(56, false);
                e.set_field(26..50, self.offset);
            }
            MemSpace::Shared => {
                e.set_opcode(SM20Unit::Mem, 0x30);
                e.set_bit(56, true);
                e.set_field(26..50, self.offset);
            }
        }
        e.set_mem_type(5..8, self.access.mem_type);
        e.set_ld_cache_op(8..10, self.access.ld_cache_op(e.sm));
        e.set_dst(14..20, &self.dst);
        e.set_reg_src(20..26, &self.addr);
    }
}

impl SM20Op for OpLdc {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use crate::codegen::ir::RegFile;
        b.copy_alu_src_if_not_reg(&mut self.offset, RegFile::GPR, SrcType::GPR);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        assert!(self.cb.is_unmodified());
        let SrcRef::CBuf(cb) = &self.cb.reference else {
            panic!("Not a CBuf source");
        };
        let CBuf::Binding(cb_idx) = cb.buf else {
            panic!("Must be a bound constant buffer");
        };
        e.set_opcode(SM20Unit::Tex, 0x5);
        e.set_mem_type(5..8, self.mem_type);
        e.set_field(
            8..10,
            match self.mode {
                LdcMode::Indexed => 0_u8,
                LdcMode::IndexedLinear => 1_u8,
                LdcMode::IndexedSegmented => 2_u8,
                LdcMode::IndexedSegmentedLinear => 3_u8,
            },
        );
        e.set_dst(14..20, &self.dst);
        e.set_reg_src(20..26, &self.offset);
        e.set_field(26..42, cb.offset);
        e.set_field(42..47, cb_idx);
    }
}

impl SM20Op for OpLdSharedLock {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Mem, 0x2a);
        e.set_mem_type(5..8, self.mem_type);
        e.set_dst(14..20, &self.dst);
        e.set_reg_src(20..26, &self.addr);
        e.set_field(26..50, self.offset);
        e.set_pred_dst2(8..10, 58..59, &self.locked);
    }
}

impl SM20Op for OpSt {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        assert_eq!(self.stride, OffsetStride::X1);
        match self.access.space {
            MemSpace::Global(addr_type) => {
                e.set_opcode(SM20Unit::Mem, 0x24);
                e.set_field(26..58, self.offset);
                e.set_bit(58, addr_type == MemAddrType::A64);
            }
            MemSpace::Local => {
                e.set_opcode(SM20Unit::Mem, 0x32);
                e.set_bit(56, false);
                e.set_field(26..50, self.offset);
            }
            MemSpace::Shared => {
                e.set_opcode(SM20Unit::Mem, 0x32);
                e.set_bit(56, true);
                e.set_field(26..50, self.offset);
            }
        }
        e.set_mem_type(5..8, self.access.mem_type);
        e.set_st_cache_op(8..10, self.access.st_cache_op(e.sm));
        e.set_reg_src(14..20, &self.data);
        e.set_reg_src(20..26, &self.addr);
    }
}

impl SM20Op for OpStSCheckUnlock {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Mem, 0x2e);
        e.set_mem_type(5..8, self.mem_type);
        e.set_reg_src(14..20, &self.data);
        e.set_reg_src(20..26, &self.addr);
        e.set_field(26..50, self.offset);
        e.set_pred_dst2(8..10, 58..59, &self.locked);
    }
}

fn atom_src_as_ssa(b: &mut LegalizeBuilder, src: &Src, atom_type: AtomType) -> SSARef {
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

impl SM20Op for OpAtom {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        if self.atom_op == AtomOp::CmpExch(AtomCmpSrc::Separate) {
            let cmpr = atom_src_as_ssa(b, &self.cmpr, self.atom_type);
            let data = atom_src_as_ssa(b, &self.data, self.atom_type);
            let mut cmpr_data = Vec::new();
            cmpr_data.extend_from_slice(&cmpr);
            cmpr_data.extend_from_slice(&data);
            let cmpr_data = SSARef::try_from(cmpr_data).expect("cmpr+data must form valid SSARef");
            self.cmpr = 0.into();
            self.data = cmpr_data.into();
            self.atom_op = AtomOp::CmpExch(AtomCmpSrc::Packed);
        }
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        let MemSpace::Global(addr_type) = self.mem_space else {
            panic!("SM20 only supports global atomics");
        };
        assert!(addr_type == MemAddrType::A64);
        assert_eq!(self.addr_stride, OffsetStride::X1);

        if self.dst.is_none() {
            e.set_opcode(SM20Unit::Mem, 0x1);
        } else {
            e.set_opcode(SM20Unit::Mem, 0x11);
        }

        let op = match self.atom_op {
            AtomOp::Add => 0_u8,
            AtomOp::Min => 1_u8,
            AtomOp::Max => 2_u8,
            AtomOp::Inc => 3_u8,
            AtomOp::Dec => 4_u8,
            AtomOp::And => 5_u8,
            AtomOp::Or => 6_u8,
            AtomOp::Xor => 7_u8,
            AtomOp::Exch => 8_u8,
            AtomOp::CmpExch(_) => 9_u8,
        };
        e.set_field(5..9, op);

        let typ = match self.atom_type {
            AtomType::F16x2 => panic!("Unsupported atomic type"),
            AtomType::U32 => 0x4_u8,
            AtomType::I32 => 0x7_u8,
            AtomType::F32 => 0xd_u8,
            AtomType::U64 | AtomType::I64 | AtomType::F64 => {
                panic!("64-bit atomics are not supported");
            }
        };
        e.set_field(9..10, typ & 0x1);
        e.set_field(59..62, typ >> 1);

        e.set_reg_src(20..26, &self.addr);
        e.set_reg_src(14..20, &self.data);

        if self.dst.is_none() {
            e.set_field(26..58, self.addr_offset);
        } else {
            e.set_dst(43..49, &self.dst);
            e.set_field(26..43, self.addr_offset & 0x1_ffff);
            e.set_field(55..58, self.addr_offset >> 17);
        }

        if let AtomOp::CmpExch(cmp_src) = self.atom_op {
            assert!(cmp_src == AtomCmpSrc::Packed);
            let cmpr_data = self
                .data
                .reference
                .as_reg()
                .expect("CmpExch packed must have register");
            assert!(cmpr_data.comps() % 2 == 0);
            let data_comps = cmpr_data.comps() / 2;
            let data_idx = cmpr_data.base_idx() + u32::from(data_comps);
            let data = RegRef::new(cmpr_data.file(), data_idx, data_comps);
            e.set_reg_src(49..55, &data.into());
        } else if !self.dst.is_none() {
            e.set_reg_src(49..55, &0.into());
        }
    }
}

impl SM20Op for OpAL2P {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Tex, 0x3);
        e.set_field(5..7, self.comps.ilog2());
        e.set_bit(9, self.output);
        e.set_dst(14..20, &self.dst);
        e.set_reg_src(20..26, &self.offset);
        e.set_field(32..43, self.addr);
    }
}

impl SM20Op for OpALd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Tex, 0x1);
        e.set_field(5..7, self.comps - 1);
        if self.phys {
            assert!(!self.patch);
            assert!(self.offset.reference.as_reg().is_some());
        } else if !self.patch {
            assert!(self.offset.is_zero());
        }
        e.set_bit(8, self.patch);
        e.set_bit(9, self.output);
        e.set_dst(14..20, &self.dst);
        e.set_reg_src(20..26, &self.offset);
        e.set_reg_src(26..32, &self.vtx);
        e.set_field(32..42, self.addr);
    }
}

impl SM20Op for OpASt {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Tex, 0x2);
        e.set_field(5..7, self.comps - 1);
        e.set_bit(8, self.patch);
        assert!(!self.phys);
        e.set_reg_src(20..26, &self.offset);
        e.set_reg_src(26..32, &self.data);
        e.set_field(32..42, self.addr);
        e.set_reg_src(49..55, &self.vtx);
    }
}

impl SM20Op for OpIpa {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Float, 0x30);
        e.set_bit(5, false);
        e.set_field(
            6..8,
            match self.freq {
                InterpFreq::Pass => 0_u8,
                InterpFreq::PassMulW => 1_u8,
                InterpFreq::Constant => 2_u8,
                InterpFreq::State => 3_u8,
            },
        );
        e.set_field(
            8..10,
            match self.loc {
                InterpLoc::Default => 0_u8,
                InterpLoc::Centroid => 1_u8,
                InterpLoc::Offset => 2_u8,
            },
        );
        e.set_dst(14..20, &self.dst);
        e.set_reg_src(20..26, &0.into());
        e.set_reg_src(26..32, &self.inv_w);
        e.set_reg_src(49..55, &self.offset);
        e.set_field(32..42, self.addr);
    }
}

impl SM20Op for OpCCtl {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        let op = match self.mem_space {
            MemSpace::Global(MemAddrType::A32) => 0x26,
            MemSpace::Global(MemAddrType::A64) => 0x27,
            MemSpace::Local => panic!("cctl does not support local"),
            MemSpace::Shared => 0x34,
        };
        e.set_opcode(SM20Unit::Mem, op);
        e.set_field(
            5..10,
            match self.op {
                CCtlOp::Qry1 => 0_u8,
                CCtlOp::PF1 => 1_u8,
                CCtlOp::PF1_5 => 2_u8,
                CCtlOp::PF2 => 3_u8,
                CCtlOp::WB => 4_u8,
                CCtlOp::IV => 5_u8,
                CCtlOp::IVAll => 6_u8,
                CCtlOp::RS => 7_u8,
                CCtlOp::WBAll => 8_u8,
                CCtlOp::RSLB => 9_u8,
                CCtlOp::IVAllP | CCtlOp::WBAllP => {
                    panic!("cctl{} is not supported on SM20", self.op);
                }
            },
        );
        e.set_dst(14..20, &Dst::None);
        e.set_reg_src(20..26, &self.addr);
        e.set_field(26..28, 0);
        assert!(self.addr_offset % 4 == 0);
        if matches!(self.mem_space, MemSpace::Global(_)) {
            e.set_field(28..58, self.addr_offset / 4);
        } else {
            e.set_field(28..50, self.addr_offset / 4);
        }
    }
}

impl SM20Op for OpMemBar {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {}

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Mem, 0x38);
        e.set_field(
            5..7,
            match self.scope {
                MemScope::CTA => 0_u8,
                MemScope::GPU => 1_u8,
                MemScope::System => 2_u8,
            },
        );
    }
}
