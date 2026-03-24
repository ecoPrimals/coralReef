// SPDX-License-Identifier: AGPL-3.0-only
//! Exercises `or_exit_code` on `Result::Err` with a non-default exit code.

use coralreef_core::or_exit::OrExit;

fn main() {
    let _: i32 = Result::<i32, &str>::Err("code path").or_exit_code("custom code exit", 42);
}
