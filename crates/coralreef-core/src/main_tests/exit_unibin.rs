// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

use std::process::ExitCode;

#[test]
fn install_panic_hook_sets_hook() {
    let prev = std::panic::take_hook();
    install_panic_hook();
    std::panic::set_hook(prev);
}

#[test]
fn unibin_exit_to_exit_code_success() {
    let ec: ExitCode = UniBinExit::Success.into();
    assert_eq!(ec, ExitCode::SUCCESS);
}

#[test]
fn unibin_exit_to_exit_code_general_error() {
    let ec: ExitCode = UniBinExit::GeneralError.into();
    assert_eq!(ec, ExitCode::from(1u8));
}

#[test]
fn unibin_exit_to_exit_code_config_error() {
    let ec: ExitCode = UniBinExit::ConfigError.into();
    assert_eq!(ec, ExitCode::from(2u8));
}

#[test]
fn unibin_exit_to_exit_code_internal_error() {
    let ec: ExitCode = UniBinExit::InternalError.into();
    assert_eq!(ec, ExitCode::from(3u8));
}

#[test]
fn unibin_exit_to_exit_code_signal() {
    let ec: ExitCode = UniBinExit::Signal.into();
    assert_eq!(ec, ExitCode::from(130u8));
}

#[test]
fn unibin_exit_code_values() {
    assert_eq!(UniBinExit::Success as i32, 0);
    assert_eq!(UniBinExit::GeneralError as i32, 1);
    assert_eq!(UniBinExit::ConfigError as i32, 2);
    assert_eq!(UniBinExit::InternalError as i32, 3);
    assert_eq!(UniBinExit::Signal as i32, 130);
}

#[test]
fn unibin_exit_to_exit_code() {
    let _: ExitCode = UniBinExit::Success.into();
    let _: ExitCode = UniBinExit::GeneralError.into();
    let _: ExitCode = UniBinExit::ConfigError.into();
    let _: ExitCode = UniBinExit::InternalError.into();
    let _: ExitCode = UniBinExit::Signal.into();
}

#[test]
fn unibin_exit_clone_and_copy() {
    let a = UniBinExit::Success;
    let b = a;
    assert_eq!(a as i32, b as i32);
}
