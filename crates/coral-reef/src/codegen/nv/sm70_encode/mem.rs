// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! SM70 memory, load/store, atomic, and surface instruction encoders.

use super::encoder::*;

impl SM70Encoder<'_> {
    fn set_mem_order(&mut self, order: &MemOrder) {
        if self.sm < 80 {
            let scope = match order {
                MemOrder::Constant => MemScope::System,
                MemOrder::Weak => MemScope::CTA,
                MemOrder::Strong(s) => *s,
            };
            self.set_field(
                77..79,
                match scope {
                    MemScope::CTA => 0_u8,
                    // SM => 1_u8,
                    MemScope::GPU => 2_u8,
                    MemScope::System => 3_u8,
                },
            );
            self.set_field(
                79..81,
                match order {
                    MemOrder::Constant => 0_u8,
                    MemOrder::Weak => 1_u8,
                    MemOrder::Strong(_) => 2_u8,
                    // MMIO => 3_u8,
                },
            );
        } else {
            self.set_field(
                77..81,
                match order {
                    MemOrder::Constant => 0x4_u8,
                    MemOrder::Weak => 0x0_u8,
                    MemOrder::Strong(MemScope::CTA) => 0x5_u8,
                    MemOrder::Strong(MemScope::GPU) => 0x7_u8,
                    MemOrder::Strong(MemScope::System) => 0xa_u8,
                },
            );
        }
    }

    pub(super) fn set_eviction_priority(&mut self, pri: &MemEvictionPriority) {
        self.set_field(
            84..87,
            match pri {
                MemEvictionPriority::First => 0_u8,
                MemEvictionPriority::Normal => 1_u8,
                MemEvictionPriority::Last => 2_u8,
                MemEvictionPriority::LastUse => 3_u8,
                MemEvictionPriority::Unchanged => 4_u8,
                MemEvictionPriority::NoAllocate => 5_u8,
            },
        );
    }

    fn set_mem_type(&mut self, range: Range<usize>, mem_type: MemType) {
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

    fn set_mem_access(&mut self, access: &MemAccess) {
        self.set_field(
            72..73,
            match access.space.addr_type() {
                MemAddrType::A32 => 0_u8,
                MemAddrType::A64 => 1_u8,
            },
        );
        self.set_mem_type(73..76, access.mem_type);
        self.set_mem_order(&access.order);
        self.set_eviction_priority(&access.eviction_priority);
    }
}

impl SM70Op for OpSuLd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);

        // suld.constant doesn't exist on Volta or Turing but it's always safe
        // to silently degrade to suld.weak
        if self.mem_order == MemOrder::Constant && b.sm() < 80 {
            self.mem_order = MemOrder::Weak;
        }
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        match self.image_access {
            ImageAccess::Binary(mem_type) => {
                e.set_opcode(0x99a);
                e.set_mem_type(73..76, mem_type);
            }
            ImageAccess::Formatted(channel_mask) => {
                e.set_opcode(0x998);
                e.set_image_channel_mask(72..76, channel_mask);
            }
        }

        e.set_dst(self.dst());
        e.set_reg_src(24..32, self.coord());
        e.set_reg_src(64..72, self.handle());
        e.set_pred_dst(81..84, self.fault());
        if e.sm >= 120 {
            e.set_ureg_src(48..56, &Src::ZERO); // handle
        }

        e.set_image_dim(61..64, self.image_dim);
        e.set_mem_order(&self.mem_order);
        e.set_eviction_priority(&self.mem_eviction_priority);
    }
}

impl SM70Op for OpSuSt {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        match self.image_access {
            ImageAccess::Binary(mem_type) => {
                e.set_opcode(0x99e);
                e.set_mem_type(73..76, mem_type);
            }
            ImageAccess::Formatted(channel_mask) => {
                e.set_opcode(0x99c);
                e.set_image_channel_mask(72..76, channel_mask);
            }
        }

        e.set_reg_src(24..32, self.coord());
        e.set_reg_src(32..40, self.data());
        e.set_reg_src(64..72, self.handle());
        if e.sm >= 120 {
            e.set_ureg_src(48..56, &Src::ZERO); // handle
        }

        e.set_image_dim(61..64, self.image_dim);
        e.set_mem_order(&self.mem_order);
        e.set_eviction_priority(&self.mem_eviction_priority);
    }
}

impl SM70Op for OpSuAtom {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.dst().is_none() {
            e.set_opcode(0x3a0);
            e.set_atom_op(87..90, self.atom_op);
        } else if let AtomOp::CmpExch(cmp_src) = self.atom_op {
            e.set_opcode(0x396);
            assert!(cmp_src == AtomCmpSrc::Packed);
        } else {
            e.set_opcode(0x394);
            e.set_atom_op(87..91, self.atom_op);
        }

        e.set_dst(self.dst());
        e.set_reg_src(24..32, self.coord());
        e.set_reg_src(32..40, self.data());
        e.set_reg_src(64..72, self.handle());
        e.set_pred_dst(81..84, self.fault());
        if e.sm >= 120 {
            e.set_ureg_src(48..56, &Src::ZERO); // handle
        }

        e.set_image_dim(61..64, self.image_dim);
        e.set_mem_order(&self.mem_order);
        e.set_eviction_priority(&self.mem_eviction_priority);

        e.set_bit(72, false); // .BA
        e.set_atom_type(self.atom_type, true);
    }
}

impl SM70Op for OpLd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        match self.access.space {
            MemSpace::Global(_) => {
                e.set_opcode(0x381);
                assert_eq!(self.stride, OffsetStride::X1);
                e.set_pred_dst(81..84, &Dst::None);
                e.set_mem_access(&self.access);
            }
            MemSpace::Local => {
                assert_eq!(self.stride, OffsetStride::X1);
                e.set_opcode(0x983);
                e.set_field(84..87, 1_u8);

                e.set_mem_type(73..76, self.access.mem_type);
                assert!(self.access.order == MemOrder::Strong(MemScope::CTA));
                assert!(self.access.eviction_priority == MemEvictionPriority::Normal);
            }
            MemSpace::Shared => {
                e.set_opcode(0x984);

                e.set_mem_type(73..76, self.access.mem_type);
                assert!(self.access.order == MemOrder::Strong(MemScope::CTA));
                assert!(self.access.eviction_priority == MemEvictionPriority::Normal);

                assert!(e.sm >= 75 || self.stride == OffsetStride::X1);
                e.set_field(78..80, self.stride.encode_sm75());
                e.set_bit(87, false); // !.ZD - Returns a predicate?
            }
        }

        e.set_dst(&self.dst);
        e.set_reg_src(24..32, &self.addr);
        e.set_field(40..64, self.offset);
    }
}

impl SM70Op for OpLdc {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        b.copy_alu_src_if_not_reg(self.offset_mut(), gpr, SrcType::GPR);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        let SrcRef::CBuf(cb) = &self.cb().reference else {
            crate::codegen::ice!("LDC must take a cbuf source");
        };

        match cb.buf {
            CBuf::Binding(idx) => {
                if self.is_uniform() {
                    if e.sm >= 100 {
                        e.set_opcode(0x7ac);
                        e.set_bit(91, true);
                        e.set_ureg_src(24..32, self.offset());
                    } else {
                        e.set_opcode(0xab9);
                        e.set_bit(91, false);
                        assert!(self.offset().is_zero());
                    }
                    e.set_udst(&self.dst);
                    assert!(self.mode == LdcMode::Indexed);
                } else {
                    e.set_opcode(0xb82);
                    e.set_dst(&self.dst);

                    e.set_reg_src(24..32, self.offset());
                    e.set_field(
                        78..80,
                        match self.mode {
                            LdcMode::Indexed => 0_u8,
                            LdcMode::IndexedLinear => 1_u8,
                            LdcMode::IndexedSegmented => 2_u8,
                            LdcMode::IndexedSegmentedLinear => 3_u8,
                        },
                    );
                    e.set_bit(91, false); // Bound
                }
                e.set_field(54..59, idx);
            }
            CBuf::BindlessUGPR(handle) => {
                if self.is_uniform() {
                    if e.sm >= 100 {
                        e.set_opcode(0xbac);
                    } else {
                        e.set_opcode(0xab9);
                    }
                    e.set_udst(&self.dst);

                    if e.sm >= 120 {
                        e.set_ureg_src(64..72, self.offset());
                    } else if e.sm >= 100 {
                        // Blackwell A adds the source but it has to be zero
                        assert!(self.offset().is_zero());
                        e.set_ureg_src(64..72, self.offset());
                    } else {
                        assert!(self.offset().is_zero());
                    }
                } else {
                    e.set_opcode(0x582);
                    e.set_dst(&self.dst);

                    e.set_reg_src(64..72, self.offset());
                }

                e.set_ureg(24..32, handle);
                assert!(self.mode == LdcMode::Indexed);
                e.set_bit(91, true); // Bindless
            }
            CBuf::BindlessSSA(_) => crate::codegen::ice!("SSA values must be lowered"),
        }

        if e.sm >= 100 && self.is_uniform() {
            e.set_field(37..54, cb.offset);
        } else {
            e.set_field(38..54, cb.offset);
        }
        e.set_mem_type(73..76, self.mem_type);

        if e.sm >= 120 {
            e.set_field(80..82, 0_u8); // tex/hdr_unpack
        } else if e.sm >= 100 {
            e.set_bit(80, false); // tex_unpack
        }
    }
}

impl SM70Op for OpSt {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        match self.access.space {
            MemSpace::Global(_) => {
                e.set_opcode(0x386);
                assert_eq!(self.stride, OffsetStride::X1);
                e.set_mem_access(&self.access);
            }
            MemSpace::Local => {
                e.set_opcode(0x387);
                assert_eq!(self.stride, OffsetStride::X1);
                e.set_field(84..87, 1_u8);

                e.set_mem_type(73..76, self.access.mem_type);
                assert!(self.access.order == MemOrder::Strong(MemScope::CTA));
                assert!(self.access.eviction_priority == MemEvictionPriority::Normal);
            }
            MemSpace::Shared => {
                e.set_opcode(0x388);

                e.set_mem_type(73..76, self.access.mem_type);
                assert!(self.access.order == MemOrder::Strong(MemScope::CTA));
                assert!(self.access.eviction_priority == MemEvictionPriority::Normal);

                assert!(e.sm >= 75 || self.stride == OffsetStride::X1);
                e.set_field(78..80, self.stride.encode_sm75());
            }
        }

        e.set_reg_src(24..32, self.addr());
        e.set_reg_src(32..40, self.data());
        e.set_field(40..64, self.offset);
    }
}

impl SM70Encoder<'_> {
    fn set_atom_op(&mut self, range: Range<usize>, atom_op: AtomOp) {
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
            },
        );
    }

    fn set_atom_type(&mut self, atom_type: AtomType, su: bool) {
        if self.sm >= 90 && !su {
            // Float/int is differentiated by opcode
            self.set_field(
                73..77,
                match atom_type {
                    AtomType::F16x2 => 0_u8,
                    // f16x4 => 1
                    // f16x8 => 2
                    // bf16x2 => 3
                    // bf16x4 => 4
                    // bf16x8 => 5
                    AtomType::F32 => 9_u8, // .ftz
                    // f32x2.ftz => 10
                    // f32x4.ftz => 11
                    // f32x1 => 12
                    // f32x2 => 13
                    // f32x4 => 14
                    AtomType::F64 => 15_u8,

                    AtomType::U32 => 0,
                    AtomType::I32 => 1,
                    AtomType::U64 => 2,
                    AtomType::I64 => 3,
                    // u128 => 4,
                },
            );
        } else {
            self.set_field(
                73..76,
                match atom_type {
                    AtomType::U32 => 0_u8,
                    AtomType::I32 => 1_u8,
                    AtomType::U64 => 2_u8,
                    AtomType::F32 => 3_u8,
                    AtomType::F16x2 => 4_u8,
                    AtomType::I64 => 5_u8,
                    AtomType::F64 => 6_u8,
                },
            );
        }
    }
}

impl SM70Op for OpAtom {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        match self.mem_space {
            MemSpace::Global(_) => {
                if self.dst.is_none() {
                    if e.sm >= 90 && self.atom_type.is_float() {
                        e.set_opcode(0x9a6);
                    } else {
                        e.set_opcode(0x98e);
                    }

                    e.set_reg_src(32..40, self.data());
                    e.set_atom_op(87..90, self.atom_op);
                } else if let AtomOp::CmpExch(cmp_src) = self.atom_op {
                    e.set_opcode(0x3a9);

                    assert!(cmp_src == AtomCmpSrc::Separate);
                    e.set_reg_src(32..40, self.cmpr());
                    e.set_reg_src(64..72, self.data());
                    e.set_pred_dst(81..84, &Dst::None);
                } else {
                    if e.sm >= 90 && self.atom_type.is_float() {
                        e.set_opcode(0x3a3);
                    } else {
                        e.set_opcode(0x3a8);
                    }

                    e.set_reg_src(32..40, self.data());
                    e.set_pred_dst(81..84, &Dst::None);
                    e.set_atom_op(87..91, self.atom_op);
                }

                e.set_field(
                    72..73,
                    match self.mem_space.addr_type() {
                        MemAddrType::A32 => 0_u8,
                        MemAddrType::A64 => 1_u8,
                    },
                );

                e.set_mem_order(&self.mem_order);
                e.set_eviction_priority(&self.mem_eviction_priority);
                assert_eq!(self.addr_stride, OffsetStride::X1);
            }
            MemSpace::Local => crate::codegen::ice!("Atomics do not support local"),
            MemSpace::Shared => {
                if let AtomOp::CmpExch(cmp_src) = self.atom_op {
                    e.set_opcode(0x38d);

                    assert!(cmp_src == AtomCmpSrc::Separate);
                    e.set_reg_src(32..40, self.cmpr());
                    e.set_reg_src(64..72, self.data());
                } else {
                    e.set_opcode(0x38c);

                    e.set_reg_src(32..40, self.data());
                    assert!(
                        self.atom_type != AtomType::U64 || self.atom_op == AtomOp::Exch,
                        "64-bit Shared atomics only support CmpExch or Exch"
                    );
                    assert!(
                        !self.atom_type.is_float(),
                        "Shared atomics don't support float"
                    );
                    e.set_atom_op(87..91, self.atom_op);
                }

                assert!(e.sm >= 75 || self.addr_stride == OffsetStride::X1);
                e.set_field(78..80, self.addr_stride.encode_sm75());

                assert!(self.mem_order == MemOrder::Strong(MemScope::CTA));
                assert!(self.mem_eviction_priority == MemEvictionPriority::Normal);
            }
        }

        e.set_dst(&self.dst);
        e.set_reg_src(24..32, self.addr());
        e.set_field(40..64, self.addr_offset);
        e.set_atom_type(self.atom_type, false);
    }
}

impl SM70Op for OpAL2P {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.set_opcode(0x920);

        e.set_dst(&self.dst);
        e.set_reg_src(24..32, &self.offset);

        e.set_field(40..50, self.addr);
        e.set_field(74..76, 0_u8); // comps
        e.set_bit(79, self.output);
    }
}

impl SM70Op for OpALd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.set_opcode(0x321);

        e.set_dst(&self.dst);
        e.set_reg_src(32..40, self.vtx());
        e.set_reg_src(24..32, self.offset());

        e.set_field(40..50, self.addr);
        e.set_field(74..76, self.comps - 1);
        e.set_field(76..77, self.patch);
        e.set_field(77..78, self.phys);
        e.set_field(79..80, self.output);
    }
}

impl SM70Op for OpASt {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.set_opcode(0x322);

        e.set_reg_src(32..40, self.data());
        e.set_reg_src(64..72, self.vtx());
        e.set_reg_src(24..32, self.offset());

        e.set_field(40..50, self.addr);
        e.set_field(74..76, self.comps - 1);
        e.set_field(76..77, self.patch);
        e.set_field(77..78, self.phys);
    }
}

impl SM70Op for OpIpa {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.set_opcode(0x326);

        e.set_dst(&self.dst);

        assert!(self.addr % 4 == 0);
        e.set_field(64..72, self.addr >> 2);

        e.set_field(
            76..78,
            match self.loc {
                InterpLoc::Default => 0_u8,
                InterpLoc::Centroid => 1_u8,
                InterpLoc::Offset => 2_u8,
            },
        );
        e.set_field(
            78..80,
            match self.freq {
                InterpFreq::Pass => 0_u8,
                InterpFreq::Constant => 1_u8,
                InterpFreq::State => 2_u8,
                InterpFreq::PassMulW => {
                    crate::codegen::ice!("InterpFreq::PassMulW is invalid on SM70+");
                }
            },
        );

        assert!(self.inv_w().is_zero());
        e.set_reg_src(32..40, self.offset());

        // pred_dst: none for interpolation (no predicate destination).
        e.set_pred_dst(81..84, &Dst::None);
    }
}

impl SM70Op for OpLdTram {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.set_opcode(0x3ad);
        e.set_dst(&self.dst);
        e.set_ureg(24..32, e.zero_reg(RegFile::UGPR));

        assert!(self.addr % 4 == 0);
        e.set_field(64..72, self.addr >> 2);

        e.set_bit(72, self.use_c);

        // Unknown but required
        e.set_bit(91, true);
    }
}

impl SM70Op for OpCCtl {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        assert!(matches!(self.mem_space, MemSpace::Global(_)));
        e.set_opcode(0x98f);

        e.set_reg_src(24..32, &self.addr);
        e.set_field(32..64, self.addr_offset);

        e.set_field(
            87..91,
            match self.op {
                CCtlOp::PF1 => 0_u8,
                CCtlOp::PF2 => 1_u8,
                CCtlOp::WB => 2_u8,
                CCtlOp::IV => 3_u8,
                CCtlOp::IVAll => 4_u8,
                CCtlOp::RS => 5_u8,
                CCtlOp::IVAllP => 6_u8,
                CCtlOp::WBAll => 7_u8,
                CCtlOp::WBAllP => 8_u8,
                op => crate::codegen::ice!("Unsupported cache control {op:?}"),
            },
        );
    }
}

impl SM70Op for OpMemBar {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.set_opcode(0x992);

        e.set_bit(72, false); // !.MMIO
        e.set_field(
            76..79,
            match self.scope {
                MemScope::CTA => 0_u8,
                // SM => 1_u8,
                MemScope::GPU => 2_u8,
                MemScope::System => 3_u8,
            },
        );
        e.set_bit(80, false); // .SC
    }
}
