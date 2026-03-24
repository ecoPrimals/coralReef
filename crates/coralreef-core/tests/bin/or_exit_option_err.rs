// SPDX-License-Identifier: AGPL-3.0-only
//! Exercises `OrExit` for `Option::None` — exits with code 1.

use coralreef_core::or_exit::OrExit;

fn main() {
    let _: i32 = None::<i32>.or_exit("option was None");
}
