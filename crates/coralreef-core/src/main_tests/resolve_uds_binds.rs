// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

#[test]
fn tcp_bind_passes_through() {
    let (actual, override_path) = resolve_uds_binds("127.0.0.1:0");
    assert_eq!(actual, "127.0.0.1:0");
    assert!(override_path.is_none());
}

#[test]
fn unix_composition_socket_produces_c2_tarpc_extension() {
    let (tarpc, jsonrpc) = resolve_uds_binds("unix:///run/user/1000/biomeos/coralreef-alpha.sock");
    assert_eq!(
        tarpc,
        "unix:///run/user/1000/biomeos/coralreef-alpha.tarpc.sock"
    );
    assert_eq!(
        jsonrpc.as_ref().unwrap().to_str().unwrap(),
        "/run/user/1000/biomeos/coralreef-alpha.sock"
    );
}

#[test]
fn unix_tarpc_extension_skips_redirect() {
    let (tarpc, jsonrpc) =
        resolve_uds_binds("unix:///run/user/1000/biomeos/coralreef-alpha.tarpc.sock");
    assert_eq!(
        tarpc,
        "unix:///run/user/1000/biomeos/coralreef-alpha.tarpc.sock"
    );
    assert!(jsonrpc.is_none());
}

#[test]
fn unix_no_extension_gets_tarpc_sock() {
    let (tarpc, jsonrpc) = resolve_uds_binds("unix:///tmp/coralreef");
    assert_eq!(tarpc, "unix:///tmp/coralreef.tarpc.sock");
    assert_eq!(
        jsonrpc.as_ref().unwrap().to_str().unwrap(),
        "/tmp/coralreef"
    );
}
