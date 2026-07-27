// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals

use super::*;

fn zero_src() -> Src {
    Src::ZERO
}

#[test]
fn test_op_cs2r_display() {
    let op = OpCS2R {
        dst: Dst::None,
        idx: 0x10,
    };
    let s = format!("{op}");
    assert!(s.contains("cs2r"));
    assert!(s.contains("0x10"));
}

#[test]
fn test_op_isberd_display() {
    let op = OpIsberd {
        dst: Dst::None,
        idx: zero_src(),
    };
    let s = format!("{op}");
    assert!(s.contains("isberd"));
}

#[test]
fn test_op_vild_display() {
    let op = OpViLd {
        dst: Dst::None,
        idx: zero_src(),
        off: 0,
    };
    let s = format!("{op}");
    assert!(s.contains("vild"));
}

#[test]
fn test_op_kill_display() {
    let op = OpKill {};
    let s = format!("{op}");
    assert!(s.contains("kill"));
}

#[test]
fn test_op_nop_display() {
    let op = OpNop { label: None };
    let s = format!("{op}");
    assert!(s.contains("nop"));
}

#[test]
fn test_op_nop_with_label() {
    let mut alloc = LabelAllocator::new();
    let label = alloc.alloc();
    let op = OpNop { label: Some(label) };
    let s = format!("{op}");
    assert!(s.contains("nop"));
}

#[test]
fn test_pix_val_display() {
    assert_eq!(format!("{}", PixVal::MsCount), ".mscount");
    assert_eq!(format!("{}", PixVal::CovMask), ".covmask");
    assert_eq!(format!("{}", PixVal::InnerCoverage), ".inner_coverage");
}

#[test]
fn test_op_pixld_display() {
    let op = OpPixLd {
        dst: Dst::None,
        val: PixVal::MsCount,
    };
    let s = format!("{op}");
    assert!(s.contains("pixld"));
    assert!(s.contains("mscount"));
}

#[test]
fn test_op_s2r_display() {
    let op = OpS2R {
        dst: Dst::None,
        idx: 0x20,
    };
    let s = format!("{op}");
    assert!(s.contains("s2r"));
    assert!(s.contains("0x20"));
}

#[test]
fn test_vote_op_display() {
    assert_eq!(format!("{}", VoteOp::Any), "any");
    assert_eq!(format!("{}", VoteOp::All), "all");
    assert_eq!(format!("{}", VoteOp::Eq), "eq");
}

#[test]
fn test_op_vote_display() {
    let op = OpVote {
        op: VoteOp::Any,
        dsts: [Dst::None, Dst::None],
        pred: Src::new_imm_bool(true),
    };
    let s = format!("{op}");
    assert!(s.contains("vote"));
    assert!(s.contains("any"));
}

#[test]
fn test_match_op_display() {
    assert_eq!(format!("{}", MatchOp::All), ".all");
    assert_eq!(format!("{}", MatchOp::Any), ".any");
}

#[test]
fn test_op_match_display() {
    let op = OpMatch {
        dsts: [Dst::None, Dst::None],
        src: zero_src(),
        op: MatchOp::All,
        u64: false,
    };
    let s = format!("{op}");
    assert!(s.contains("match"));
    assert!(s.contains(".all"));
}

#[test]
fn test_op_undef_display() {
    let op = OpUndef { dst: Dst::None };
    let s = format!("{op}");
    assert!(s.contains("undef"));
}

#[test]
fn test_op_srcbar_display() {
    let op = OpSrcBar { src: zero_src() };
    let s = format!("{op}");
    assert!(s.contains("src_bar"));
}

#[test]
fn test_op_copy_display() {
    let op = OpCopy {
        dst: Dst::None,
        src: zero_src(),
    };
    let s = format!("{op}");
    assert!(s.contains("copy"));
}

#[test]
fn test_op_pin_display() {
    let op = OpPin {
        dst: Dst::None,
        src: zero_src(),
    };
    let s = format!("{op}");
    assert!(s.contains("pin"));
}

#[test]
fn test_op_unpin_display() {
    let op = OpUnpin {
        dst: Dst::None,
        src: zero_src(),
    };
    let s = format!("{op}");
    assert!(s.contains("unpin"));
}

#[test]
fn test_op_swap_display() {
    let op = OpSwap {
        dsts: [Dst::None, Dst::None],
        srcs: [zero_src(), Src::new_imm_u32(1)],
    };
    let s = format!("{op}");
    assert!(s.contains("swap"));
}

#[test]
fn test_op_parcopy_display() {
    let mut op = OpParCopy::new();
    op.push(Dst::None, zero_src());
    let s = format!("{op}");
    assert!(s.contains("par_copy"));
}

#[test]
fn test_op_regout_display() {
    let op = OpRegOut {
        srcs: vec![zero_src()],
    };
    let s = format!("{op}");
    assert!(s.contains("reg_out"));
}

#[test]
fn test_out_type_display() {
    assert_eq!(format!("{}", OutType::Emit), "emit");
    assert_eq!(format!("{}", OutType::Cut), "cut");
    assert_eq!(format!("{}", OutType::EmitThenCut), "emit_then_cut");
}

#[test]
fn test_op_out_display() {
    let op = OpOut {
        dst: Dst::None,
        srcs: [zero_src(), zero_src()],
        out_type: OutType::Emit,
    };
    let s = format!("{op}");
    assert!(s.contains("out"));
    assert!(s.contains("emit"));
}

#[test]
fn test_op_outfinal_display() {
    let op = OpOutFinal { handle: zero_src() };
    let s = format!("{op}");
    assert!(s.contains("out.final"));
}

#[test]
fn test_op_annotate_display() {
    let op = OpAnnotate {
        annotation: "test comment".into(),
    };
    let s = format!("{op}");
    assert!(s.contains("//"));
    assert!(s.contains("test comment"));
}
