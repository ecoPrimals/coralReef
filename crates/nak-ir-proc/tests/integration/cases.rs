// SPDX-License-Identifier: AGPL-3.0-only
//! Integration tests for `nak_ir_proc` derives.

use std::fmt;
use std::fmt::Write as _;

use coral_reef_stubs::as_slice::{AsSlice, AttrList};
use nak_ir_proc::{DstsAsSlice, Encode, FromVariants, SrcsAsSlice};

use super::support::{DisplayOp, Dst, DstType, Src, SrcType};

// --- SrcsAsSlice: single field, uniform attr --------------------------------

#[derive(SrcsAsSlice)]
struct SingleSrc {
    #[src_type(A)]
    s: Src,
}

#[test]
fn srcs_single_field_from_ref() {
    let op = SingleSrc { s: Src(7) };
    assert_eq!(AsSlice::<Src>::as_slice(&op), &[Src(7)]);
    match AsSlice::<Src>::attrs(&op) {
        AttrList::Uniform(SrcType::A) => {}
        _ => panic!("expected Uniform(A)"),
    }
}

// --- SrcsAsSlice: default variant when #[src_type] omitted -------------------

#[derive(SrcsAsSlice)]
struct SingleSrcDefault {
    s: Src,
}

#[test]
fn srcs_default_attr_when_omitted() {
    let op = SingleSrcDefault { s: Src(1) };
    match AsSlice::<Src>::attrs(&op) {
        AttrList::Uniform(SrcType::DEFAULT) => {}
        _ => panic!("expected Uniform(DEFAULT)"),
    }
}

// --- SrcsAsSlice: array + #[src_types] (per-element list) ------------------

#[derive(SrcsAsSlice)]
struct ArraySrcPerElem {
    #[src_types(A, B, C)]
    srcs: [Src; 3],
}

#[test]
fn srcs_array_per_element_attrs() {
    let op = ArraySrcPerElem {
        srcs: [Src(1), Src(2), Src(3)],
    };
    assert_eq!(AsSlice::<Src>::as_slice(&op), &op.srcs[..]);
    match AsSlice::<Src>::attrs(&op) {
        AttrList::List(v) => {
            assert_eq!(v.len(), 3);
            assert_eq!(v[0], SrcType::A);
            assert_eq!(v[1], SrcType::B);
            assert_eq!(v[2], SrcType::C);
        }
        AttrList::Uniform(_) => panic!("expected List attrs"),
    }
}

// --- SrcsAsSlice: array with uniform #[src_type] replicated ----------------

#[derive(SrcsAsSlice)]
struct ArraySrcUniform {
    #[src_type(A)]
    srcs: [Src; 2],
}

#[test]
fn srcs_array_uniform_attr_single_field() {
    let op = ArraySrcUniform {
        srcs: [Src(9), Src(8)],
    };
    // One array field + `#[src_type]` without `#[src_types]` → `Uniform` (not per-element List).
    match AsSlice::<Src>::attrs(&op) {
        AttrList::Uniform(SrcType::A) => {}
        _ => panic!("expected Uniform(A) for single array field"),
    }
}

// --- SrcsAsSlice: #[src_names] accessors ----------------------------------

#[derive(SrcsAsSlice)]
struct ArraySrcNamed {
    #[src_type(A)]
    #[src_names(lhs, rhs)]
    pair: [Src; 2],
}

#[test]
fn srcs_named_accessors() {
    let mut op = ArraySrcNamed {
        pair: [Src(10), Src(20)],
    };
    assert_eq!(*op.lhs(), Src(10));
    assert_eq!(*op.rhs(), Src(20));
    *op.lhs_mut() = Src(30);
    assert_eq!(op.pair[0], Src(30));
}

// --- SrcsAsSlice: no matching fields (empty slice) ---------------------------

#[derive(SrcsAsSlice)]
struct NoSrcs {
    _label: u32,
}

#[test]
fn srcs_empty_when_no_operand_fields() {
    let op = NoSrcs { _label: 42 };
    assert!(AsSlice::<Src>::as_slice(&op).is_empty());
    assert!(AsSlice::<Src>::as_mut_slice(&mut NoSrcs { _label: 0 }).is_empty());
    match AsSlice::<Src>::attrs(&op) {
        AttrList::List(v) => assert!(v.is_empty()),
        AttrList::Uniform(_) => panic!("expected empty List"),
    }
}

// --- SrcsAsSlice: enum dispatch (direct + Box) -----------------------------

#[derive(SrcsAsSlice)]
struct InnerSrc {
    #[src_type(A)]
    s: Src,
}

#[derive(SrcsAsSlice)]
enum SrcEnum {
    Direct(InnerSrc),
    Boxed(Box<InnerSrc>),
}

#[test]
fn srcs_enum_delegates_boxed_and_unboxed() {
    let a = SrcEnum::Direct(InnerSrc { s: Src(1) });
    let b = SrcEnum::Boxed(Box::new(InnerSrc { s: Src(2) }));
    assert_eq!(AsSlice::<Src>::as_slice(&a), &[Src(1)]);
    assert_eq!(AsSlice::<Src>::as_slice(&b), &[Src(2)]);
    match AsSlice::<Src>::attrs(&a) {
        AttrList::Uniform(t) => assert_eq!(t, SrcType::A),
        AttrList::List(_) => panic!("expected uniform"),
    }
}

// --- DstsAsSlice: mirror of src patterns ----------------------------------

#[derive(DstsAsSlice)]
struct SingleDst {
    #[dst_type(Out)]
    d: Dst,
}

#[test]
fn dsts_single_and_attrs() {
    let op = SingleDst { d: Dst(5) };
    assert_eq!(AsSlice::<Dst>::as_slice(&op), &[Dst(5)]);
    match AsSlice::<Dst>::attrs(&op) {
        AttrList::Uniform(DstType::Out) => {}
        _ => panic!("expected Uniform(Out)"),
    }
}

// --- DisplayOp --------------------------------------------------------------

struct AtomDisplay;

impl DisplayOp for AtomDisplay {
    fn fmt_dsts(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "d0")
    }

    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ATOM")
    }
}

#[derive(nak_ir_proc::DisplayOp)]
enum ShowOp {
    Atom(AtomDisplay),
}

#[test]
fn display_op_delegates_to_variants() {
    let op = ShowOp::Atom(AtomDisplay);
    let ds = format!("{}", FmtDisplayOp(&op));
    assert!(ds.contains("d0"));
    assert!(ds.contains("ATOM"));
}

/// Like coral-reef's `Display` for `Op`, but minimal.
struct FmtDisplayOp<'a>(&'a ShowOp);

impl fmt::Display for FmtDisplayOp<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            ShowOp::Atom(x) => {
                let mut s = String::new();
                write!(&mut s, "{}", Fmt(|ff| DisplayOp::fmt_dsts(x, ff)))?;
                if !s.is_empty() {
                    write!(f, "{s} = ")?;
                }
                DisplayOp::fmt_op(x, f)
            }
        }
    }
}

struct Fmt<F: Fn(&mut fmt::Formatter) -> fmt::Result>(F);

impl<F: Fn(&mut fmt::Formatter) -> fmt::Result> fmt::Display for Fmt<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (self.0)(f)
    }
}

// --- FromVariants (including Box inner) -----------------------------------

#[derive(Debug, PartialEq, Eq, FromVariants)]
enum Packed {
    A(u8),
    B(Box<u16>),
}

#[test]
fn from_variants_tuple_and_box_inner() {
    let x: Packed = From::from(3u8);
    assert_eq!(x, Packed::A(3));
    let y: Packed = From::from(99u16);
    assert_eq!(y, Packed::B(Box::new(99)));
}

// --- Encode: 1-word vs 2-word encoding -------------------------------------

#[derive(Encode)]
#[encoding(VOP1)]
struct EncOneWord {
    #[enc(offset = 0, width = 8)]
    lo: u8,
}

#[derive(Encode)]
#[encoding(VOP3)]
struct EncTwoWords {
    #[enc(offset = 0, width = 8)]
    a: u8,
    #[enc(offset = 32, width = 8)]
    b: u8,
}

#[test]
fn encode_word_count_vop1_vs_other() {
    let one = EncOneWord { lo: 0xAB };
    assert_eq!(one.encode().len(), 1);
    let two = EncTwoWords { a: 1, b: 2 };
    assert_eq!(two.encode().len(), 2);
}

#[derive(Encode)]
#[encoding(VOP3)]
struct EncFieldWithoutEnc {
    /// Not encoded — no `#[enc]` attribute.
    _ignored: u32,
}

#[test]
fn encode_skips_fields_without_enc_attr() {
    let v = EncFieldWithoutEnc {
        _ignored: 0xFFFF_FFFF,
    };
    assert_eq!(v.encode(), [0, 0]);
}
