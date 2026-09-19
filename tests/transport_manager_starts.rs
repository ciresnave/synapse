// SPDX-License-Identifier: MIT OR Apache-2.0
//! Regression test: `TransportManager::start` must return when a factory is registered.
//!
//! `start_transport` used to hold a write guard on `transport_status` until the
//! end of the function, then ask for the same lock again. Tokio's `RwLock` is not
//! reentrant, so `start()` hung for every registered transport. Measured
//! 2026-09-17 at `8edce9c1`. Nothing had caught it because no test or probe had
//! started a transport through the manager.

use std::collections::HashMap;
use std::time::Duration;

use synapse::transport::{
    TransportManager, TransportManagerConfig, TransportStatus, TransportType, UdpTransportFactory,
};

mod common;
use common::free_udp_port;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn start_returns_when_a_udp_factory_is_registered() {
    let mut config = TransportManagerConfig {
        enabled_transports: vec![TransportType::Udp],
        ..Default::default()
    };
    let mut udp = HashMap::new();
    udp.insert("bind_port".to_string(), free_udp_port().to_string());
    config.transport_configs.insert(TransportType::Udp, udp);

    let manager = TransportManager::new(config);
    manager
        .register_factory(Box::new(UdpTransportFactory))
        .await
        .expect("register the UDP factory");

    let started = tokio::time::timeout(Duration::from_secs(5), manager.start()).await;
    assert!(
        started.is_ok(),
        "start() did not return within 5 s: start_transport holds the status lock and asks for it again"
    );
    started
        .unwrap()
        .expect("start() returned an error instead of starting UDP");

    let status = manager.get_transport_status().await;
    assert!(
        matches!(
            status.get(&TransportType::Udp),
            Some(TransportStatus::Running)
        ),
        "UDP should be Running after start(), got {status:?}"
    );

    manager.stop().await.expect("stop()");
}
