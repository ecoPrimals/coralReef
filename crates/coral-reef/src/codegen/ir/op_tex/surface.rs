// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! Surface load, store, and atomic operations.

use super::*;
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpSuLd {
    pub dst: Dst,
    pub fault: Dst,

    pub image_access: ImageAccess,
    pub image_dim: ImageDim,
    pub mem_order: MemOrder,
    pub mem_eviction_priority: MemEvictionPriority,

    #[src_type(SSA)]
    pub handle: Src,

    #[src_type(SSA)]
    pub coord: Src,
}

impl DisplayOp for OpSuLd {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "suld{}{}{}{} [{}] {}",
            self.image_access,
            self.image_dim,
            self.mem_order,
            self.mem_eviction_priority,
            self.coord,
            self.handle,
        )
    }
}
impl_display_for_op!(OpSuLd);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpSuSt {
    pub image_access: ImageAccess,
    pub image_dim: ImageDim,
    pub mem_order: MemOrder,
    pub mem_eviction_priority: MemEvictionPriority,

    #[src_type(SSA)]
    pub handle: Src,

    #[src_type(SSA)]
    pub coord: Src,

    #[src_type(SSA)]
    pub data: Src,
}

impl DisplayOp for OpSuSt {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "sust{}{}{}{} [{}] {} {}",
            self.image_access,
            self.image_dim,
            self.mem_order,
            self.mem_eviction_priority,
            self.coord,
            self.data,
            self.handle,
        )
    }
}
impl_display_for_op!(OpSuSt);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpSuAtom {
    pub dst: Dst,
    pub fault: Dst,

    pub image_dim: ImageDim,

    pub atom_op: AtomOp,
    pub atom_type: AtomType,

    pub mem_order: MemOrder,
    pub mem_eviction_priority: MemEvictionPriority,

    #[src_type(SSA)]
    pub handle: Src,

    #[src_type(SSA)]
    pub coord: Src,

    #[src_type(SSA)]
    pub data: Src,
}

impl DisplayOp for OpSuAtom {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "suatom.p{}{}{}{}{} [{}] {} {}",
            self.image_dim,
            self.atom_op,
            self.atom_type,
            self.mem_order,
            self.mem_eviction_priority,
            self.coord,
            self.data,
            self.handle,
        )
    }
}
impl_display_for_op!(OpSuAtom);

#[derive(Clone, Copy)]
pub enum SuClampMode {
    StoredInDescriptor,
    PitchLinear,
    BlockLinear,
}

impl fmt::Display for SuClampMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::StoredInDescriptor => ".sd",
            Self::PitchLinear => ".pl",
            Self::BlockLinear => ".bl",
        };
        write!(f, "{}", s)
    }
}

#[derive(Clone, Copy)]
pub enum SuClampRound {
    R1,
    R2,
    R4,
    R8,
    R16,
}

impl SuClampRound {
    pub const fn to_int(&self) -> u8 {
        match self {
            Self::R1 => 1,
            Self::R2 => 2,
            Self::R4 => 4,
            Self::R8 => 8,
            Self::R16 => 16,
        }
    }

    #[allow(dead_code)]
    pub fn to_mask(&self) -> u32 {
        !(self.to_int() as u32 - 1)
    }
}

impl fmt::Display for SuClampRound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, ".r{}", self.to_int())
    }
}

/// Kepler only
/// Surface Clamp
///
/// Can clamp coordinates of surface operations in a 0..=clamp inclusive
/// range. It also computes other information useful to compute the
/// real address of an element within an image for both block-lienar and
/// pitch-linear layouts. We can also reduce this operation to a "stupid"
/// inclusive clamp by setting modifier Mode=PitchLinear and is_2d=false
/// this will not compute any extra operations and is useful to clamp array
/// indexes.
///
/// Since the shader code does not know if an image layout is block-linear
/// or pitch-linear, this opcode must be able to do both, the operation
/// is then selected by the "clamp" bitfield, usually read from a descriptor.
/// In block-linear mode we divide the bits that will compute the higher
/// part and the lower part.
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice, Clone)]
pub struct OpSuClamp {
    #[dst_type(GPR)]
    pub dst: Dst,
    #[dst_type(Pred)]
    pub out_of_bounds: Dst,

    /// This modifier specifies if we use pitch-linear or block-linear
    /// calculations, another option is to support both and read the actual
    /// format from the clamp (shader code doesn't always know if an image
    /// layout).
    /// When mode=pitch_linear and is_2d=false the suclamp op enters a
    /// simpler "plain" mode where it only performs clamping and the output
    /// register doesn't contain any information bits about pitch-linear or
    /// block-linear calculations
    pub mode: SuClampMode,
    /// Strangely enough, "round" just rounds the clamp, not the source
    /// this does not help at all with clamping coordinates.
    /// It could be useful when clamping raw addresses of a multi-byte read.
    /// ex: if we read 4 bytes at once, and the buffer length is 16,
    ///     the bounds will be 15 (they are inclusive), but if we read
    ///     at address 15 we would read bytes 15..19, so we are out of range.
    ///     if we clamp tthe bounds to R4 the effective bound becomes 12
    ///     so the read will be performed from 12..16, remaining in bounds.
    pub round: SuClampRound,
    pub is_s32: bool,
    pub is_2d: bool,

    #[src_type(GPR)]
    pub coords: Src,

    /// Packed parameter containing both bounds (inclusive)
    /// and other information (explained in more details in Foldable):
    /// 0..20: bound (inclusive)
    /// 21: pitch_linear (used if mode == StoredInDescriptor)
    /// 22..26: coord shl
    /// 26..29: coord shr
    /// 29..32: n. of tiles
    #[src_type(ALU)]
    pub params: Src,
    /// Added to the coords, it's only an i6
    pub imm: i8,
}

impl Foldable for OpSuClamp {
    fn fold(&self, _sm: &dyn ShaderModel, f: &mut OpFoldData<'_>) {
        let src = f.get_u32_src(self, &self.coords);
        let params = f.get_u32_src(self, &self.params);
        let imm = self.imm; // i6

        let src = if self.is_s32 {
            (src as i32) as i64
        } else {
            src as i64
        };
        let src = src + (imm as i64);

        let params_bv = &params;
        let pitch_linear = match self.mode {
            SuClampMode::StoredInDescriptor => params_bv.get_bit(21),
            SuClampMode::PitchLinear => true,
            SuClampMode::BlockLinear => false,
        };

        let bounds = if pitch_linear && !self.is_2d {
            params
        } else {
            params_bv.get_bit_range_u64(0..20) as u32
        };

        let bounds = bounds & self.round.to_mask();
        let (is_oob, clamped) = if src < 0 {
            (true, 0)
        } else if src > (bounds as i64) {
            (true, bounds)
        } else {
            (false, src as u32)
        };

        let mut out = 0u32;
        let bv = &mut out;
        if pitch_linear {
            if !self.is_2d {
                // simple clamp mode, NO BITFIELD
                bv.set_field(0..32, clamped);
            } else {
                // Real, pitch_linear mode
                bv.set_field(0..20, clamped & 0xf_ffff);

                // Pass through el_size_log2
                bv.set_field(27..30, params_bv.get_bit_range_u64(26..29));
                bv.set_bit(30, true); // pitch_linear=true
                bv.set_bit(31, is_oob);
            }
        } else {
            // Block linear

            // Number of bits to discard for GoB coordinates
            let shr_a = params_bv.get_bit_range_u64(22..26) as u8;
            // Block coords
            bv.set_field(0..16, (clamped >> shr_a) & 0xffff);

            // Shift applied to coords, always zero except for x.
            // (for coord x=1 and format R32, we want to access byte 4)
            // e.g. R8 -> 0, R32 -> 2, 128 -> 4
            let el_size_log2 = params_bv.get_bit_range_u64(26..29) as u8;
            // Coord inside GoB (element space)
            bv.set_field(16..24, (clamped << el_size_log2) & 0xff);

            // Useful later to compute gob-space coords.
            let n_tiles = params_bv.get_bit_range_u64(29..32) as u8;
            bv.set_field(27..30, n_tiles);
            bv.set_bit(30, false); // pitch_linear=false
            bv.set_bit(31, is_oob);
        }
        f.set_u32_dst(self, &self.dst, out);
        f.set_pred_dst(self, &self.out_of_bounds, is_oob);
    }
}

impl DisplayOp for OpSuClamp {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "suclamp{}", self.mode)?;
        if !matches!(self.round, SuClampRound::R1) {
            write!(f, "{}", self.round)?;
        }
        if !self.is_s32 {
            write!(f, ".u32")?;
        }
        if !self.is_2d {
            write!(f, ".1d")?;
        }

        write!(f, " {} {} {:x}", self.coords, self.params, self.imm)
    }
}
impl_display_for_op!(OpSuClamp);

/// Kepler only
/// BitField Merge
///
/// The resulting bit-field is composed of a high-part 8..32 that is merged
/// with the address by sueau, and a lower-part 0..8 that is provided
/// directly to suldga/sustga and defines the lower offset of the glonal array.
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice, Clone)]
pub struct OpSuBfm {
    #[dst_type(GPR)]
    pub dst: Dst,
    #[dst_type(Pred)]
    pub pdst: Dst,

    /// x, y, z
    #[src_type(ALU)]
    pub srcs: [Src; 3],
    /// When is_3d=false the third source is ignored, but still used in
    /// pitch-linear computation.
    pub is_3d: bool,
}

impl Foldable for OpSuBfm {
    fn fold(&self, _sm: &dyn ShaderModel, f: &mut OpFoldData<'_>) {
        let x_raw = f.get_u32_src(self, &self.srcs[0]);
        let y_raw = f.get_u32_src(self, &self.srcs[1]);
        let z_raw = f.get_u32_src(self, &self.srcs[2]);

        let x = &x_raw;
        let y = &y_raw;
        let z = &z_raw;

        let mut o_raw = 0u32;
        let o = &mut o_raw;

        let is_pitch_linear_2d = x.get_bit(30) || y.get_bit(30);

        if !is_pitch_linear_2d {
            // Copy coordinates inside of GoB space.
            // They are 6 bits from x and 3 from y (GoB is 64x8 bytes).
            // Bits from 0..8 are ignored by sueau and are used directly
            // by suldga/sustga.
            // Bit 9 will become the first bit of the higher part in
            // sueau.
            o.set_bit_range_u64(0..4, x.get_bit_range_u64(16..20));

            // Address calculation inside of GoB should virtually be
            // y * 64 + x * element_size (each row is linear).
            // So why are those bits swizzled like so?
            // I have no idea, but these are correct even for atomics
            // that accept real addresses.
            o.set_bit(4, y.get_bit(16));
            o.set_bit(5, y.get_bit(17));
            o.set_bit(6, x.get_bit(20));
            o.set_bit(7, y.get_bit(18));

            o.set_bit(8, x.get_bit(21));
            // 9..11: 0

            // -------------- Tiles --------------
            // Number of tiles log2
            let ntx = x.get_bit_range_u64(27..30) & 0x1;
            let nty = y.get_bit_range_u64(27..30);
            let ntz = z.get_bit_range_u64(27..30);
            let ntz = ntz * (self.is_3d as u64); // z is ignored if is_3d=false

            // Computes how many bits to dedicate to GoB coords inside
            // a block
            o.set_field(12..16, ntx + nty + ntz);

            // Coords in gob_space.
            // Remove 6 bits from x and 3 bits from y, those are used
            // as element coords in GoB space.
            let a = x.get_bit_range_u64(22..24); // 1100_0000
            let b = y.get_bit_range_u64(19..24); // 1111_1000
            let c = z.get_bit_range_u64(16..24); // 1111_1111

            // nt* indicates how many bits to consider (max 5)
            let a = a & ((1 << ntx) - 1);
            let b = b & ((1 << nty.min(5)) - 1);
            let c = c & ((1 << ntz.min(5)) - 1);

            // Compute gob offset
            // We can just or together at certain offsets because
            // Tiles are always powers of two in each direction.
            // z || y || x (LSB)
            let res = c;
            let res = (res << nty) | b;
            let res = (res << ntx) | a;
            let mask = match ntx {
                0 => 0x3ff,
                _ => 0x7ff,
            };

            // gob coords will be put before the block coords in
            // sueau.
            o.set_field(16..27, res & mask);
        } else {
            let d = z.get_bit_range_u64(0..8);
            let el_size_log2 = x.get_bit_range_u64(27..30);
            o.set_field(0..8, (d << el_size_log2) & 0xff);
            // 9..11: 0
            o.set_field(12..15, el_size_log2);
        }

        o.set_bit(11, is_pitch_linear_2d);

        let is_oob = x.get_bit(31) || y.get_bit(31) || (z.get_bit(31) && self.is_3d);
        f.set_u32_dst(self, &self.dst, o_raw);
        f.set_pred_dst(self, &self.pdst, is_oob);
    }
}

impl DisplayOp for OpSuBfm {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "subfm")?;

        if self.is_3d {
            write!(f, ".3d")?;
        }

        write!(f, " {} {} {}", self.srcs[0], self.srcs[1], self.srcs[2])
    }
}
impl_display_for_op!(OpSuBfm);

/// Kepler only
/// Used to compute the higher 32 bits of image address using
/// the merged bitfield and the block coordinates (offset).
/// It can switch to a pitch_linear mode (bit 11 of bit-field).
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice, Clone)]
pub struct OpSuEau {
    #[dst_type(GPR)]
    pub dst: Dst,

    /// offset is computed from the block coordinates.
    /// it's ok to add it directly to the address since they are both
    /// "aligned" to 64 (the first 8 bits are removed from both)
    #[src_type(GPR)]
    pub off: Src,

    ///  8.. 9: offset, last bit
    /// 11..12: pitch_linear: when enabled the bf-offset is ignored and
    ///         the off_shl is subtracted by 8
    /// 12..16: off_shl, shifts left the offset by off_shl + 1
    /// 16..27: 11-bit offset, when joined with the 1-bit offset completes the
    ///         12-bit offset ORed to the src offset after shifting
    ///         (unless pitch_linear)
    #[src_type(ALU)]
    pub bit_field: Src,

    #[src_type(GPR)]
    pub addr: Src,
}

impl Foldable for OpSuEau {
    fn fold(&self, _sm: &dyn ShaderModel, f: &mut OpFoldData<'_>) {
        let off_raw = f.get_u32_src(self, &self.off);
        let bf_raw = f.get_u32_src(self, &self.bit_field);
        let addr = f.get_u32_src(self, &self.addr);

        let bf = &bf_raw;

        let off1 = bf.get_bit_range_u64(8..9) as u32;
        let is_pitch_linear = bf.get_bit(11);
        let off_shift = bf.get_bit_range_u64(12..16) as u32;
        let offs = bf.get_bit_range_u64(16..27) as u32;

        let res = if !is_pitch_linear {
            // Block linear
            // off_raw are the block coordinates
            // to those we add gob coordinates from the merged bitfield
            // and the MSB of in-gob coordinates.
            let omul = off_shift + 1;
            let real_off = (off_raw << omul) | (offs << 1) | off1;
            addr.wrapping_add(real_off & 0x7ff_ffff)
        } else {
            // Add the high part of the coordinates to addr
            // off << (omul - 8)
            // but for negative values do a shr instead.
            // In fact, off_shift will always be < 8 because pitch_linear
            // subfm only assigns bits 12..15, so this is always a shr
            let shl_amount = off_shift as i32 - 8;
            let off = if shl_amount < 0 {
                off_raw >> (-shl_amount as u32)
            } else {
                off_raw << (shl_amount as u32)
            };
            addr.wrapping_add(off & 0xff_ffff)
        };
        f.set_u32_dst(self, &self.dst, res);
    }
}

impl DisplayOp for OpSuEau {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sueau {} {} {}", self.off, self.bit_field, self.addr)
    }
}
impl_display_for_op!(OpSuEau);

#[cfg(test)]
mod tests {
    use super::*;

    fn zero_src() -> Src {
        Src::ZERO
    }

    #[test]
    fn test_op_suld_display() {
        let op = OpSuLd {
            dst: Dst::None,
            fault: Dst::None,
            image_access: ImageAccess::Binary(MemType::B32),
            image_dim: ImageDim::_2D,
            mem_order: MemOrder::Constant,
            mem_eviction_priority: MemEvictionPriority::Normal,
            handle: zero_src(),
            coord: Src::new_imm_u32(0),
        };
        let s = format!("{}", op);
        assert!(s.contains("suld"));
        assert!(s.contains(".b32"));
        assert!(s.contains(".2d"));
        assert!(s.contains(".constant"));
    }

    #[test]
    fn test_op_suld_field_access() {
        let op = OpSuLd {
            dst: Dst::None,
            fault: Dst::None,
            image_access: ImageAccess::Formatted(ChannelMask::for_comps(4)),
            image_dim: ImageDim::_3D,
            mem_order: MemOrder::Weak,
            mem_eviction_priority: MemEvictionPriority::First,
            handle: zero_src(),
            coord: zero_src(),
        };
        assert!(matches!(op.image_dim, ImageDim::_3D));
        assert_eq!(op.image_dim.coord_comps(), 3);
        let s = format!("{}", op);
        assert!(s.contains(".p.rgba"));
        assert!(s.contains(".3d"));
        assert!(s.contains(".weak"));
        assert!(s.contains(".ef"));
    }

    #[test]
    fn test_op_sust_display() {
        let op = OpSuSt {
            image_access: ImageAccess::Binary(MemType::U16),
            image_dim: ImageDim::_1DBuffer,
            mem_order: MemOrder::Strong(MemScope::CTA),
            mem_eviction_priority: MemEvictionPriority::Last,
            handle: zero_src(),
            coord: zero_src(),
            data: Src::new_imm_u32(42),
        };
        let s = format!("{}", op);
        assert!(s.contains("sust"));
        assert!(s.contains(".u16"));
        assert!(s.contains(".buf"));
        assert!(s.contains(".strong.cta"));
        assert!(s.contains(".el"));
    }

    #[test]
    fn test_op_suatom_display() {
        let op = OpSuAtom {
            dst: Dst::None,
            fault: Dst::None,
            image_dim: ImageDim::_2DArray,
            atom_op: AtomOp::Add,
            atom_type: AtomType::U32,
            mem_order: MemOrder::Weak,
            mem_eviction_priority: MemEvictionPriority::Normal,
            handle: zero_src(),
            coord: zero_src(),
            data: Src::new_imm_u32(1),
        };
        let s = format!("{}", op);
        assert!(s.contains("suatom"));
        assert!(s.contains(".a2d"));
        assert!(s.contains(".add"));
        assert!(s.contains(".u32"));
        assert!(s.contains(".weak"));
    }

    #[test]
    fn test_su_clamp_mode_display() {
        assert_eq!(format!("{}", SuClampMode::StoredInDescriptor), ".sd");
        assert_eq!(format!("{}", SuClampMode::PitchLinear), ".pl");
        assert_eq!(format!("{}", SuClampMode::BlockLinear), ".bl");
    }

    #[test]
    fn test_su_clamp_round_display_and_to_int() {
        assert_eq!(SuClampRound::R1.to_int(), 1);
        assert_eq!(SuClampRound::R4.to_int(), 4);
        assert_eq!(SuClampRound::R16.to_int(), 16);
        assert_eq!(format!("{}", SuClampRound::R1), ".r1");
        assert_eq!(format!("{}", SuClampRound::R8), ".r8");
    }

    #[test]
    fn test_op_suclamp_display() {
        let op = OpSuClamp {
            dst: Dst::None,
            out_of_bounds: Dst::None,
            mode: SuClampMode::PitchLinear,
            round: SuClampRound::R4,
            is_s32: true,
            is_2d: true,
            coords: zero_src(),
            params: Src::new_imm_u32(0),
            imm: 0,
        };
        let s = format!("{}", op);
        assert!(s.contains("suclamp"));
        assert!(s.contains(".pl"));
        assert!(s.contains(".r4"));
    }

    #[test]
    fn test_op_suclamp_display_1d_u32() {
        let op = OpSuClamp {
            dst: Dst::None,
            out_of_bounds: Dst::None,
            mode: SuClampMode::BlockLinear,
            round: SuClampRound::R1,
            is_s32: false,
            is_2d: false,
            coords: zero_src(),
            params: zero_src(),
            imm: 4,
        };
        let s = format!("{}", op);
        assert!(s.contains(".bl"));
        assert!(s.contains(".u32"));
        assert!(s.contains(".1d"));
    }

    #[test]
    fn test_op_subfm_display() {
        let op = OpSuBfm {
            dst: Dst::None,
            pdst: Dst::None,
            srcs: [zero_src(), zero_src(), zero_src()],
            is_3d: true,
        };
        let s = format!("{}", op);
        assert!(s.contains("subfm"));
        assert!(s.contains(".3d"));
    }

    #[test]
    fn test_op_sueau_display() {
        let op = OpSuEau {
            dst: Dst::None,
            off: zero_src(),
            bit_field: Src::new_imm_u32(0),
            addr: zero_src(),
        };
        let s = format!("{}", op);
        assert!(s.contains("sueau"));
    }
}
