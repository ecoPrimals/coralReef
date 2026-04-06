// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! Shader IO metadata: system values, vertex/fragment attribute info.

use std::cmp::{max, min};
use std::ops::Range;

use bitview::{BitMutViewable, BitViewable};

use super::PixelImap;
use crate::CompileError;

#[derive(Debug, Default)]
pub struct SysValInfo {
    pub ab: u32,
    pub c: u16,
}

#[derive(Debug)]
pub struct VtgIoInfo {
    pub sysvals_in: SysValInfo,
    pub sysvals_in_d: u8,
    pub sysvals_out: SysValInfo,
    pub sysvals_out_d: u8,
    pub attr_in: [u32; 4],
    pub attr_out: [u32; 4],
    pub store_req_start: u8,
    pub store_req_end: u8,
    pub clip_enable: u8,
    pub cull_enable: u8,
    pub xfb: Option<Box<super::TransformFeedbackInfo>>,
}

impl VtgIoInfo {
    fn mark_attrs(&mut self, addrs: Range<u16>, written: bool) -> Result<(), CompileError> {
        let sysvals = if written {
            &mut self.sysvals_out
        } else {
            &mut self.sysvals_in
        };

        let sysvals_d = if written {
            &mut self.sysvals_out_d
        } else {
            &mut self.sysvals_in_d
        };

        let attr = if written {
            &mut self.attr_out
        } else {
            &mut self.attr_in
        };

        let mut addrs = addrs;
        addrs.start &= !3;
        for addr in addrs.step_by(4) {
            if addr < 0x080 {
                sysvals.ab |= 1 << (addr / 4);
            } else if addr < 0x280 {
                let attr_idx = (addr - 0x080) as usize / 4;
                attr.set_bit(attr_idx, true);
            } else if addr < 0x2c0 {
                return Err(CompileError::NotImplemented(
                    "FF color I/O not supported".into(),
                ));
            } else if addr < 0x300 {
                sysvals.c |= 1 << ((addr - 0x2c0) / 4);
            } else if (0x3a0..0x3c0).contains(&addr) {
                *sysvals_d |= 1 << ((addr - 0x3a0) / 4);
            }
        }
        Ok(())
    }

    pub fn mark_attrs_read(&mut self, addrs: Range<u16>) -> Result<(), CompileError> {
        self.mark_attrs(addrs, false)
    }

    pub fn mark_attrs_written(&mut self, addrs: Range<u16>) -> Result<(), CompileError> {
        self.mark_attrs(addrs, true)
    }

    pub fn attr_written(&self, addr: u16) -> Result<bool, CompileError> {
        Ok(if addr < 0x080 {
            self.sysvals_out.ab & (1 << (addr / 4)) != 0
        } else if addr < 0x280 {
            let attr_idx = (addr - 0x080) as usize / 4;
            self.attr_out.get_bit(attr_idx)
        } else if addr < 0x2c0 {
            return Err(CompileError::NotImplemented(
                "FF color I/O not supported".into(),
            ));
        } else if addr < 0x300 {
            self.sysvals_out.c & (1 << ((addr - 0x2c0) / 4)) != 0
        } else if (0x3a0..0x3c0).contains(&addr) {
            self.sysvals_out_d & (1 << ((addr - 0x3a0) / 4)) != 0
        } else {
            return Err(CompileError::InvalidInput(
                format!("unknown I/O address 0x{addr:03x}").into(),
            ));
        })
    }

    pub fn mark_store_req(&mut self, addrs: Range<u16>) -> Result<(), CompileError> {
        let start: u8 = (addrs.start / 4)
            .try_into()
            .map_err(|_| CompileError::InvalidInput("store_req start index out of range".into()))?;
        let end: u8 = ((addrs.end - 1) / 4)
            .try_into()
            .map_err(|_| CompileError::InvalidInput("store_req end index out of range".into()))?;
        self.store_req_start = min(self.store_req_start, start);
        self.store_req_end = max(self.store_req_end, end);
        Ok(())
    }
}

#[derive(Debug)]
pub struct FragmentIoInfo {
    pub sysvals_in: SysValInfo,
    pub sysvals_in_d: [PixelImap; 8],
    pub attr_in: [PixelImap; 128],
    pub barycentric_attr_in: [u32; 4],

    pub reads_sample_mask: bool,
    pub writes_color: u32,
    pub writes_sample_mask: bool,
    pub writes_depth: bool,
}

impl FragmentIoInfo {
    pub fn mark_attr_read(&mut self, addr: u16, interp: PixelImap) -> Result<(), CompileError> {
        if addr < 0x080 {
            self.sysvals_in.ab |= 1 << (addr / 4);
        } else if addr < 0x280 {
            let attr_idx = (addr - 0x080) as usize / 4;
            self.attr_in[attr_idx] = interp;
        } else if addr < 0x2c0 {
            return Err(CompileError::NotImplemented(
                "FF color I/O not supported".into(),
            ));
        } else if addr < 0x300 {
            self.sysvals_in.c |= 1 << ((addr - 0x2c0) / 4);
        } else if (0x3a0..0x3c0).contains(&addr) {
            let attr_idx = (addr - 0x3a0) as usize / 4;
            self.sysvals_in_d[attr_idx] = interp;
        }
        Ok(())
    }

    pub fn mark_barycentric_attr_in(&mut self, addr: u16) -> Result<(), CompileError> {
        if !(0x80..0x280).contains(&addr) {
            return Err(CompileError::InvalidInput(
                format!("barycentric attr addr 0x{addr:03x} out of range 0x080..0x280").into(),
            ));
        }
        let attr_idx = (addr - 0x080) as usize / 4;
        self.barycentric_attr_in.set_bit(attr_idx, true);
        Ok(())
    }
}

#[derive(Debug)]
pub enum ShaderIoInfo {
    None,
    Vtg(VtgIoInfo),
    Fragment(FragmentIoInfo),
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitview::BitViewable;

    fn empty_vtg() -> VtgIoInfo {
        VtgIoInfo {
            sysvals_in: SysValInfo::default(),
            sysvals_in_d: 0,
            sysvals_out: SysValInfo::default(),
            sysvals_out_d: 0,
            attr_in: [0; 4],
            attr_out: [0; 4],
            store_req_start: 0,
            store_req_end: 0,
            clip_enable: 0,
            cull_enable: 0,
            xfb: None,
        }
    }

    fn empty_fragment() -> FragmentIoInfo {
        FragmentIoInfo {
            sysvals_in: SysValInfo::default(),
            sysvals_in_d: [PixelImap::Unused; 8],
            attr_in: [PixelImap::Unused; 128],
            barycentric_attr_in: [0; 4],
            reads_sample_mask: false,
            writes_color: 0,
            writes_sample_mask: false,
            writes_depth: false,
        }
    }

    #[test]
    fn sys_val_info_default_is_zeroed() {
        let s = SysValInfo::default();
        assert_eq!(s.ab, 0);
        assert_eq!(s.c, 0);
    }

    #[test]
    fn vtg_mark_attrs_read_sets_sysvals_ab_bits() {
        let mut v = empty_vtg();
        v.mark_attrs_read(0x0..0x10).unwrap();
        assert_eq!(v.sysvals_in.ab, 0b1111);
    }

    #[test]
    fn vtg_mark_attrs_read_sets_attr_in_bits() {
        let mut v = empty_vtg();
        v.mark_attrs_read(0x080..0x084).unwrap();
        assert!(v.attr_in.get_bit(0));
        let mut v = empty_vtg();
        v.mark_attrs_read(0x084..0x088).unwrap();
        assert!(v.attr_in.get_bit(1));
    }

    #[test]
    fn vtg_mark_attrs_read_sets_sysvals_c_and_d() {
        let mut v = empty_vtg();
        v.mark_attrs_read(0x2c0..0x2c4).unwrap();
        assert_eq!(v.sysvals_in.c, 1);

        let mut v = empty_vtg();
        v.mark_attrs_read(0x3a0..0x3a4).unwrap();
        assert_eq!(v.sysvals_in_d, 1);
    }

    #[test]
    fn vtg_mark_attrs_read_rejects_fixed_function_color_range() {
        let mut v = empty_vtg();
        let err = v.mark_attrs_read(0x280..0x284).unwrap_err();
        assert!(matches!(err, crate::CompileError::NotImplemented(_)));
    }

    #[test]
    fn vtg_mark_attrs_written_sets_parallel_output_tracking() {
        let mut v = empty_vtg();
        v.mark_attrs_written(0x0..0x04).unwrap();
        assert_eq!(v.sysvals_out.ab, 1);
        v.mark_attrs_written(0x080..0x084).unwrap();
        assert!(v.attr_out.get_bit(0));
    }

    #[test]
    fn vtg_attr_written_queries_output_sysvals_attrs_and_special_ranges() {
        let mut v = empty_vtg();
        v.mark_attrs_written(0x04..0x08).unwrap();
        assert!(v.attr_written(0x04).unwrap());
        v.mark_attrs_written(0x084..0x088).unwrap();
        assert!(v.attr_written(0x084).unwrap());
        v.mark_attrs_written(0x2c4..0x2c8).unwrap();
        assert!(v.attr_written(0x2c4).unwrap());
        v.mark_attrs_written(0x3a4..0x3a8).unwrap();
        assert!(v.attr_written(0x3a4).unwrap());
    }

    #[test]
    fn vtg_attr_written_rejects_ff_color_and_unknown_address() {
        let v = empty_vtg();
        let err = v.attr_written(0x284).unwrap_err();
        assert!(matches!(err, crate::CompileError::NotImplemented(_)));
        let err = v.attr_written(0x350).unwrap_err();
        assert!(matches!(err, crate::CompileError::InvalidInput(_)));
    }

    #[test]
    fn vtg_mark_store_req_tracks_min_start_and_max_end() {
        let mut v = empty_vtg();
        v.mark_store_req(0..8).unwrap();
        assert_eq!(v.store_req_start, 0);
        assert_eq!(v.store_req_end, 1);
        v.mark_store_req(8..32).unwrap();
        assert_eq!(v.store_req_start, 0);
        assert_eq!(v.store_req_end, 7);
    }

    #[test]
    fn vtg_mark_store_req_rejects_index_overflow() {
        let mut v = empty_vtg();
        let err = v.mark_store_req(1024..1028).unwrap_err();
        assert!(matches!(err, crate::CompileError::InvalidInput(_)));
    }

    #[test]
    fn fragment_mark_attr_read_sets_sysvals_and_interpolators() {
        let mut f = empty_fragment();
        f.mark_attr_read(0x04, PixelImap::Perspective).unwrap();
        assert_eq!(f.sysvals_in.ab, 1 << 1);
        f.mark_attr_read(0x080, PixelImap::Constant).unwrap();
        assert_eq!(f.attr_in[0], PixelImap::Constant);
        f.mark_attr_read(0x2c0, PixelImap::ScreenLinear).unwrap();
        assert_eq!(f.sysvals_in.c, 1);
        f.mark_attr_read(0x3a0, PixelImap::Perspective).unwrap();
        assert_eq!(f.sysvals_in_d[0], PixelImap::Perspective);
    }

    #[test]
    fn fragment_mark_attr_read_rejects_fixed_function_color() {
        let mut f = empty_fragment();
        let err = f.mark_attr_read(0x280, PixelImap::Constant).unwrap_err();
        assert!(matches!(err, crate::CompileError::NotImplemented(_)));
    }

    #[test]
    fn fragment_mark_barycentric_attr_in_sets_bit_for_valid_range() {
        let mut f = empty_fragment();
        f.mark_barycentric_attr_in(0x080).unwrap();
        assert!(f.barycentric_attr_in.get_bit(0));
        let mut f = empty_fragment();
        f.mark_barycentric_attr_in(0x27c).unwrap();
        assert!(f.barycentric_attr_in.get_bit(127));
    }

    #[test]
    fn fragment_mark_barycentric_attr_in_rejects_out_of_range() {
        let mut f = empty_fragment();
        let err = f.mark_barycentric_attr_in(0x079).unwrap_err();
        assert!(matches!(err, crate::CompileError::InvalidInput(_)));
        let err = f.mark_barycentric_attr_in(0x280).unwrap_err();
        assert!(matches!(err, crate::CompileError::InvalidInput(_)));
    }

    #[test]
    fn shader_io_info_variants_round_trip_debug() {
        let dbg = format!("{:?}", ShaderIoInfo::None);
        assert_eq!(dbg, "None");
        let dbg = format!(
            "{:?}",
            ShaderIoInfo::Vtg(VtgIoInfo {
                sysvals_in: SysValInfo { ab: 1, c: 0 },
                sysvals_in_d: 0,
                sysvals_out: SysValInfo::default(),
                sysvals_out_d: 0,
                attr_in: [0; 4],
                attr_out: [0; 4],
                store_req_start: 0,
                store_req_end: 0,
                clip_enable: 0,
                cull_enable: 0,
                xfb: None,
            })
        );
        assert!(dbg.starts_with("Vtg("));
    }
}
