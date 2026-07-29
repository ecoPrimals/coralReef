// SPDX-License-Identifier: AGPL-3.0-or-later
//! Infallible formatting macros for PTX emission into `String` buffers.

/// Infallible `write!` to a `String` buffer — PTX emission cannot fail.
#[allow(
    unused_macros,
    reason = "used by PTX emitter files not yet migrated from writeln!"
)]
macro_rules! write_ptx {
    ($dst:expr, $($arg:tt)*) => {
        {
            use ::std::fmt::Write as _;
            write!($dst, $($arg)*).expect("write to String is infallible")
        }
    };
}

/// Infallible `writeln!` to a `String` buffer — PTX emission cannot fail.
macro_rules! writeln_ptx {
    ($dst:expr, $($arg:tt)*) => {
        {
            use ::std::fmt::Write as _;
            writeln!($dst, $($arg)*).expect("write to String is infallible")
        }
    };
    ($dst:expr) => {
        {
            use ::std::fmt::Write as _;
            writeln!($dst).expect("write to String is infallible")
        }
    };
}
