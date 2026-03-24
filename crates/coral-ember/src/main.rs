// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

fn main() {
    if let Err(code) = coral_ember::run() {
        std::process::exit(code);
    }
}
