// SPDX-License-Identifier: AGPL-3.0-only
//! Exp 123-K: K80 Sovereign Compute — GR enable, falcon wake, PIO boot.
//!
//! Tesla K80 = dual GK210 (Kepler, SM 3.7). No firmware security.
//! Direct PIO IMEM/DMEM upload for FECS/GPCCS.
//!
//! Run: `sudo cargo test --test exp123k_k80_sovereign -p coral-driver -- --ignored --nocapture`

mod exp_k1_k2;
mod exp_k2b;
mod exp_k3_k4;
mod helpers;
mod nvidia470;
mod vbios;
