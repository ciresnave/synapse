// SPDX-License-Identifier: MIT OR Apache-2.0
//! Transport contract, Task 1: the dead transports are deleted, not merely unused.

const DELETED: &[&str] = &[
    "src/transport/tcp.rs",
    "src/transport/tcp_enhanced.rs",
    "src/transport/udp.rs",
    "src/transport/quic.rs",
    "src/transport/nat_traversal_clean.rs",
    "src/transport/email_enhanced.rs",
    // `email_unified.rs` was dead when this list was written; the 2026-09-25 email-transport plan's
    // Task 1 recreated it as the live `EmailTransportImpl`/`EmailTransportFactory` (real SMTP
    // send/receive, Direct mode) -- it belongs on the "still alive" side now, alongside
    // `tcp_unified.rs`/`quic_unified.rs`/`websocket_unified.rs` below.
    "src/transport/websocket.rs",
    "src/transport/production_http.rs",
    "src/wasm/browser.rs",
    "src/wasm/crypto.rs",
    "src/wasm/storage.rs",
    "src/wasm/webrtc.rs",
    "src/wasm/websocket.rs",
    "src/wasm/worker.rs",
];

#[test]
fn the_dead_transport_files_are_deleted() {
    let root = env!("CARGO_MANIFEST_DIR");
    for path in DELETED {
        assert!(
            !std::path::Path::new(root).join(path).exists(),
            "{path} should be deleted"
        );
    }
    // Control: a live transport file still exists, so the check is looking in the right place.
    assert!(
        std::path::Path::new(root)
            .join("src/transport/udp_unified.rs")
            .exists()
    );
}

#[test]
fn there_is_one_transport_trait() {
    let module =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/transport/mod.rs"))
            .unwrap();
    assert!(
        !module.contains("pub trait Transport"),
        "the old Transport trait must be deleted"
    );
    let abstraction = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/transport/abstraction.rs"
    ))
    .unwrap();
    assert!(
        abstraction.contains("pub trait Transport"),
        "control: the kept trait is still defined"
    );
}
