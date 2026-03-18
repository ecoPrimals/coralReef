// SPDX-License-Identifier: AGPL-3.0-only
//! Test binary that exercises OrExit error path — exits with code 1.
//! Run via: cargo run --bin or_exit_err (exits immediately)

use coralreef_core::or_exit::OrExit;

fn main() {
    let _: i32 = Result::<i32, String>::Err("test error".into()).or_exit("test context");
}
