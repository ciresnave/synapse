// SPDX-License-Identifier: MIT OR Apache-2.0
//! QUIC has no real implementation yet: `quic_unified.rs` used to fabricate connections
//! (a 50 ms sleep, a hardcoded RTT, no networking) while claiming `Delivered`. Until the
//! QUIC slice lands, `QuicTransportFactory::create_transport` must refuse to construct
//! a transport instead of pretending to be one.
//!
//! Control: `UdpTransportFactory`, called the same way, must still succeed -- this proves
//! QUIC specifically refuses, not that construction is broken in general.

use synapse::transport::{QuicTransportFactory, TransportFactory, UdpTransportFactory};

#[tokio::test]
async fn quic_factory_refuses_to_construct() {
    let factory = QuicTransportFactory;
    let config = factory.default_config();

    let result = factory.create_transport(&config).await;

    let err = match result {
        Ok(_) => panic!("QUIC construction must fail until the QUIC slice lands"),
        Err(e) => e,
    };
    let message = err.to_string();
    assert!(
        message.contains("QUIC slice"),
        "error message must name the QUIC slice, got: {message}"
    );
}

#[tokio::test]
async fn udp_factory_still_constructs_control() {
    let factory = UdpTransportFactory;
    let mut config = factory.default_config();
    // Constructing the UDP transport binds nothing: the factory calls `start_server` (the only
    // bind) just when `bind_port` parses to a port above 0. Port 0 therefore skips it, so this
    // control opens no socket -- no Windows Firewall prompt, no clash with the default port 8081.
    config.insert("bind_port".to_string(), "0".to_string());

    let result = factory.create_transport(&config).await;

    assert!(
        result.is_ok(),
        "UDP construction should succeed (control): {:?}",
        result.err()
    );
}
