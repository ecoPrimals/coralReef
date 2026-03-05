// Copyright © 2023 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! SM50 memory instruction encoders.

#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::super::ir::RegFile;
use super::encoder::*;

impl SM50Op for OpSuLd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0xeb00);

        match self.image_access {
            ImageAccess::Binary(mem_type) => {
                e.set_bit(52, true); // .B
                e.set_mem_type(20..23, mem_type);
            }
            ImageAccess::Formatted(channel_mask) => {
                e.set_bit(52, false); // .P
                e.set_image_channel_mask(20..24, channel_mask);
            }
        }
        e.set_image_dim(33..36, self.image_dim);

        let cache_op = LdCacheOp::select(
            e.sm,
            MemSpace::Global(MemAddrType::A64),
            self.mem_order,
            self.mem_eviction_priority,
        );
        e.set_ld_cache_op(24..26, cache_op);

        e.set_dst(&self.dst);

        e.set_reg_src(8..16, &self.coord);
        e.set_reg_src(39..47, &self.handle);
    }
}

impl SM50Encoder<'_> {
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
            45..46,
            match access.space.addr_type() {
                MemAddrType::A32 => 0_u8,
                MemAddrType::A64 => 1_u8,
            },
        );
        self.set_mem_type(48..51, access.mem_type);
    }

    fn set_ld_cache_op(&mut self, range: Range<usize>, op: LdCacheOp) {
        let cache_op = match op {
            LdCacheOp::CacheAll => 0_u8,
            LdCacheOp::CacheGlobal => 1_u8,
            LdCacheOp::CacheIncoherent => 2_u8,
            LdCacheOp::CacheInvalidate => 3_u8,
            LdCacheOp::CacheStreaming => panic!("Unsupported cache op: ld{op}"),
        };
        self.set_field(range, cache_op);
    }

    fn set_st_cache_op(&mut self, range: Range<usize>, op: StCacheOp) {
        let cache_op = match op {
            StCacheOp::WriteBack => 0_u8,
            StCacheOp::CacheGlobal => 1_u8,
            StCacheOp::CacheStreaming => 2_u8,
            StCacheOp::WriteThrough => 3_u8,
        };
        self.set_field(range, cache_op);
    }

    fn set_image_dim(&mut self, range: Range<usize>, dim: ImageDim) {
        assert!(range.len() == 3);
        self.set_field(
            range,
            match dim {
                ImageDim::_1D => 0_u8,
                ImageDim::_1DBuffer => 1_u8,
                ImageDim::_1DArray => 2_u8,
                ImageDim::_2D => 3_u8,
                ImageDim::_2DArray => 4_u8,
                ImageDim::_3D => 5_u8,
            },
        );
    }

    fn set_image_channel_mask(&mut self, range: Range<usize>, channel_mask: ChannelMask) {
        assert!(
            channel_mask.to_bits() == 0x1
                || channel_mask.to_bits() == 0x3
                || channel_mask.to_bits() == 0xf
        );
        self.set_field(range, channel_mask.to_bits());
    }
}

impl SM50Op for OpSuSt {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0xeb20);

        match self.image_access {
            ImageAccess::Binary(mem_type) => {
                e.set_bit(52, true); // .B
                e.set_mem_type(20..23, mem_type);
            }
            ImageAccess::Formatted(channel_mask) => {
                e.set_bit(52, false); // .P
                e.set_image_channel_mask(20..24, channel_mask);
            }
        }

        e.set_reg_src(8..16, &self.coord);
        e.set_reg_src(0..8, &self.data);
        e.set_reg_src(39..47, &self.handle);

        let cache_op = StCacheOp::select(
            e.sm,
            MemSpace::Global(MemAddrType::A64),
            self.mem_order,
            self.mem_eviction_priority,
        );
        e.set_st_cache_op(24..26, cache_op);

        e.set_image_dim(33..36, self.image_dim);
    }
}

impl SM50Encoder<'_> {
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
                AtomOp::CmpExch(_) => panic!("CmpExch is a separate opcode"),
            },
        );
    }
}

impl SM50Op for OpSuAtom {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }
    fn encode(&self, e: &mut SM50Encoder<'_>) {
        if let AtomOp::CmpExch(cmp_src) = self.atom_op {
            e.set_opcode(0xeac0);
            assert!(cmp_src == AtomCmpSrc::Packed);
        } else {
            e.set_opcode(0xea60);
            e.set_atom_op(29..33, self.atom_op);
        }

        let atom_type: u8 = match self.atom_type {
            AtomType::U32 => 0,
            AtomType::I32 => 1,
            AtomType::F32 => 3,
            AtomType::U64 => 2,
            AtomType::I64 => 5,
            _ => panic!("Unsupported atom type {}", self.atom_type),
        };

        e.set_image_dim(33..36, self.image_dim);
        e.set_field(36..39, atom_type);

        // The hardware requires that we set .D on atomics.  This is safe to do
        // in in the emit code because it only affects format conversion, not
        // surface coordinates and atomics are required to be performed with
        // image formats that that exactly match the shader data type.  So, for
        // instance, a uint32_t atomic has to happen on an R32_UINT or R32_SINT
        // image.
        e.set_bit(52, true); // .D

        e.set_dst(&self.dst);

        e.set_reg_src(20..28, &self.data);
        e.set_reg_src(8..16, &self.coord);
        e.set_reg_src(39..47, &self.handle);
    }
}

impl SM50Op for OpLd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }
    fn encode(&self, e: &mut SM50Encoder<'_>) {
        assert_eq!(self.stride, OffsetStride::X1);
        e.set_opcode(match self.access.space {
            MemSpace::Global(_) => 0xeed0,
            MemSpace::Local => 0xef40,
            MemSpace::Shared => 0xef48,
        });

        e.set_dst(&self.dst);
        e.set_reg_src(8..16, &self.addr);
        e.set_field(20..44, self.offset);

        e.set_mem_access(&self.access);
        e.set_ld_cache_op(46..48, self.access.ld_cache_op(e.sm));
    }
}

impl SM50Op for OpLdc {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.offset, GPR, SrcType::GPR);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        assert!(self.cb.is_unmodified());
        let SrcRef::CBuf(cb) = &self.cb.src_ref else {
            panic!("Not a CBuf source");
        };
        let CBuf::Binding(cb_idx) = cb.buf else {
            panic!("Must be a bound constant buffer");
        };

        e.set_opcode(0xef90);

        e.set_dst(&self.dst);
        e.set_reg_src(8..16, &self.offset);
        e.set_field(20..36, cb.offset);
        e.set_field(36..41, cb_idx);
        e.set_field(
            44..46,
            match self.mode {
                LdcMode::Indexed => 0_u8,
                LdcMode::IndexedLinear => 1_u8,
                LdcMode::IndexedSegmented => 2_u8,
                LdcMode::IndexedSegmentedLinear => 3_u8,
            },
        );
        e.set_mem_type(48..51, self.mem_type);
    }
}

impl SM50Op for OpSt {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }
    fn encode(&self, e: &mut SM50Encoder<'_>) {
        assert_eq!(self.stride, OffsetStride::X1);
        e.set_opcode(match self.access.space {
            MemSpace::Global(_) => 0xeed8,
            MemSpace::Local => 0xef50,
            MemSpace::Shared => 0xef58,
        });

        e.set_reg_src(0..8, &self.data);
        e.set_reg_src(8..16, &self.addr);
        e.set_field(20..44, self.offset);
        e.set_mem_access(&self.access);
        e.set_st_cache_op(46..48, self.access.st_cache_op(e.sm));
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

impl SM50Op for OpAtom {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        if self.atom_op == AtomOp::CmpExch(AtomCmpSrc::Separate) {
            let cmpr = atom_src_as_ssa(b, &self.cmpr, self.atom_type);
            let data = atom_src_as_ssa(b, &self.data, self.atom_type);

            let mut cmpr_data = Vec::new();
            cmpr_data.extend_from_slice(&cmpr);
            cmpr_data.extend_from_slice(&data);
            let cmpr_data = SSARef::try_from(cmpr_data).unwrap();

            self.cmpr = 0.into();
            self.data = cmpr_data.into();
            self.atom_op = AtomOp::CmpExch(AtomCmpSrc::Packed);
        }
        legalize_ext_instr(self, b);
    }
    fn encode(&self, e: &mut SM50Encoder<'_>) {
        assert_eq!(self.addr_stride, OffsetStride::X1);
        match self.mem_space {
            MemSpace::Global(addr_type) => {
                if self.dst.is_none() {
                    e.set_opcode(0xebf8);

                    e.set_reg_src(0..8, &self.data);

                    let data_type = match self.atom_type {
                        AtomType::U32 => 0_u8,
                        AtomType::I32 => 1_u8,
                        AtomType::U64 => 2_u8,
                        AtomType::F32 => 3_u8,
                        // NOTE: U128 => 4_u8,
                        AtomType::I64 => 5_u8,
                        _ => panic!("Unsupported data type"),
                    };
                    e.set_field(20..23, data_type);
                    e.set_atom_op(23..26, self.atom_op);
                } else if let AtomOp::CmpExch(cmp_src) = self.atom_op {
                    e.set_opcode(0xee00);

                    e.set_dst(&self.dst);

                    // TODO: These are all supported by the disassembler but
                    // only the packed layout appears to be supported by real
                    // hardware
                    let (data_src, data_layout) = match cmp_src {
                        AtomCmpSrc::Separate => {
                            if self.data.is_zero() {
                                (&self.cmpr, 1_u8)
                            } else {
                                assert!(self.cmpr.is_zero());
                                (&self.data, 2_u8)
                            }
                        }
                        AtomCmpSrc::Packed => (&self.data, 0_u8),
                    };
                    e.set_reg_src(20..28, data_src);

                    let data_type = match self.atom_type {
                        AtomType::U32 => 0_u8,
                        AtomType::U64 => 1_u8,
                        _ => panic!("Unsupported data type"),
                    };
                    e.set_field(49..50, data_type);
                    e.set_field(50..52, data_layout);
                    e.set_field(52..56, 15_u8); // subOp
                } else {
                    e.set_opcode(0xed00);

                    e.set_dst(&self.dst);
                    e.set_reg_src(20..28, &self.data);

                    let data_type = match self.atom_type {
                        AtomType::U32 => 0_u8,
                        AtomType::I32 => 1_u8,
                        AtomType::U64 => 2_u8,
                        AtomType::F32 => 3_u8,
                        // NOTE: U128 => 4_u8,
                        AtomType::I64 => 5_u8,
                        _ => panic!("Unsupported data type"),
                    };
                    e.set_field(49..52, data_type);
                    e.set_atom_op(52..56, self.atom_op);
                }

                e.set_reg_src(8..16, &self.addr);
                e.set_field(28..48, self.addr_offset);
                e.set_field(
                    48..49,
                    match addr_type {
                        MemAddrType::A32 => 0_u8,
                        MemAddrType::A64 => 1_u8,
                    },
                );
            }
            MemSpace::Local => panic!("Atomics do not support local"),
            MemSpace::Shared => {
                if let AtomOp::CmpExch(cmp_src) = self.atom_op {
                    e.set_opcode(0xee00);

                    assert!(cmp_src == AtomCmpSrc::Packed);
                    assert!(self.cmpr.is_zero());
                    e.set_reg_src(20..28, &self.data);

                    let subop = match self.atom_type {
                        AtomType::U32 => 4_u8,
                        AtomType::U64 => 5_u8,
                        _ => panic!("Unsupported data type"),
                    };
                    e.set_field(52..56, subop);
                } else {
                    e.set_opcode(0xec00);

                    e.set_reg_src(20..28, &self.data);

                    let data_type = match self.atom_type {
                        AtomType::U32 => 0_u8,
                        AtomType::I32 => 1_u8,
                        AtomType::U64 => 2_u8,
                        AtomType::I64 => 3_u8,
                        _ => panic!("Unsupported data type"),
                    };
                    e.set_field(28..30, data_type);
                    assert!(
                        self.atom_type != AtomType::U64 || self.atom_op == AtomOp::Exch,
                        "64-bit Shared atomics only support CmpExch or Exch"
                    );
                    e.set_atom_op(52..56, self.atom_op);
                }

                e.set_dst(&self.dst);
                e.set_reg_src(8..16, &self.addr);
                assert_eq!(self.addr_offset % 4, 0);
                e.set_field(30..52, self.addr_offset / 4);
            }
        }
    }
}

impl SM50Op for OpAL2P {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }
    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0xefa0);

        e.set_dst(&self.dst);
        e.set_reg_src(8..16, &self.offset);

        e.set_field(20..31, self.addr);
        e.set_bit(32, self.output);

        e.set_field(47..49, 0_u8); // comps
        e.set_pred_dst(44..47, &Dst::None);
    }
}

impl SM50Op for OpALd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }
    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0xefd8);

        e.set_dst(&self.dst);
        if self.phys {
            assert!(!self.patch);
            assert!(self.offset.src_ref.as_reg().is_some());
        } else if !self.patch {
            assert!(self.offset.is_zero());
        }
        e.set_reg_src(8..16, &self.offset);
        e.set_reg_src(39..47, &self.vtx);

        e.set_field(20..30, self.addr);
        e.set_bit(31, self.patch);
        e.set_bit(32, self.output);
        e.set_field(47..49, self.comps - 1);
    }
}

impl SM50Op for OpASt {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }
    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0xeff0);

        e.set_reg_src(0..8, &self.data);
        e.set_reg_src(8..16, &self.offset);
        e.set_reg_src(39..47, &self.vtx);

        assert!(!self.phys);
        e.set_field(20..30, self.addr);
        e.set_bit(31, self.patch);
        e.set_bit(32, true); // output
        e.set_field(47..49, self.comps - 1);
    }
}

impl SM50Op for OpIpa {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }
    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0xe000);

        e.set_dst(&self.dst);
        e.set_reg_src(8..16, &0.into()); // addr
        e.set_reg_src(20..28, &self.inv_w);
        e.set_reg_src(39..47, &self.offset);

        assert!(self.addr % 4 == 0);
        e.set_field(28..38, self.addr);
        e.set_bit(38, false); // .IDX
        e.set_pred_dst(47..50, &Dst::None); // TODO: What is this for?
        e.set_bit(51, false); // .SAT
        e.set_field(
            52..54,
            match self.loc {
                InterpLoc::Default => 0_u8,
                InterpLoc::Centroid => 1_u8,
                InterpLoc::Offset => 2_u8,
            },
        );
        e.set_field(
            54..56,
            match self.freq {
                InterpFreq::Pass => 0_u8,
                InterpFreq::PassMulW => 1_u8,
                InterpFreq::Constant => 2_u8,
                InterpFreq::State => 3_u8,
            },
        );
    }
}

impl SM50Op for OpCCtl {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }
    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match self.mem_space {
            MemSpace::Global(addr_type) => {
                e.set_opcode(0xef60);

                assert!(self.addr_offset % 4 == 0);
                e.set_field(22..52, self.addr_offset / 4);
                e.set_field(
                    52..53,
                    match addr_type {
                        MemAddrType::A32 => 0_u8,
                        MemAddrType::A64 => 1_u8,
                    },
                );
            }
            MemSpace::Local => panic!("cctl does not support local"),
            MemSpace::Shared => {
                e.set_opcode(0xef80);

                assert!(self.addr_offset % 4 == 0);
                e.set_field(22..44, self.addr_offset / 4);
            }
        }

        e.set_field(
            0..4,
            match self.op {
                CCtlOp::Qry1 => 0_u8,
                CCtlOp::PF1 => 1_u8,
                CCtlOp::PF1_5 => 2_u8,
                CCtlOp::PF2 => 3_u8,
                CCtlOp::WB => 4_u8,
                CCtlOp::IV => 5_u8,
                CCtlOp::IVAll => 6_u8,
                CCtlOp::RS => 7_u8,
                CCtlOp::RSLB => 7_u8,
                op => panic!("Unsupported cache control {op:?}"),
            },
        );
        e.set_reg_src(8..16, &self.addr);
    }
}

impl SM50Op for OpMemBar {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0xef98);

        e.set_field(
            8..10,
            match self.scope {
                MemScope::CTA => 0_u8,
                MemScope::GPU => 1_u8,
                MemScope::System => 2_u8,
            },
        );
    }
}
