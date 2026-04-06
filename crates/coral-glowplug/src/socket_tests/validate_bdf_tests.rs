// SPDX-License-Identifier: AGPL-3.0-or-later

use super::super::*;

#[test]
fn validate_bdf_accepts_standard_address() {
    let s = "0000:01:00.0";
    assert_eq!(validate_bdf(s).expect("valid BDF"), s);
}

#[test]
fn validate_bdf_accepts_short_form() {
    let s = "01:00.0";
    assert_eq!(validate_bdf(s).expect("valid BDF"), s);
}

#[test]
fn validate_bdf_rejects_empty() {
    let err = validate_bdf("").expect_err("empty BDF");
    assert_eq!(i32::from(err.code), -32602);
}

#[test]
fn validate_bdf_rejects_path_traversal_slash() {
    let err = validate_bdf("0000:01:00.0/extra").expect_err("slash");
    assert_eq!(i32::from(err.code), -32602);
}

#[test]
fn validate_bdf_rejects_dot_dot() {
    let err = validate_bdf("0000:01..00.0").expect_err("dotdot");
    assert_eq!(i32::from(err.code), -32602);
}

#[test]
fn validate_bdf_rejects_null_byte() {
    let err = validate_bdf("0000:01:\0.0").expect_err("nul");
    assert_eq!(i32::from(err.code), -32602);
}

#[test]
fn validate_bdf_rejects_invalid_character() {
    let err = validate_bdf("0000:01:00.x").expect_err("invalid char");
    assert_eq!(i32::from(err.code), -32602);
}

#[test]
fn validate_bdf_rejects_too_long() {
    let s = "0".repeat(17);
    let err = validate_bdf(&s).expect_err("length");
    assert_eq!(i32::from(err.code), -32602);
}
