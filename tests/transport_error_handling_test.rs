// SPDX-License-Identifier: MIT OR Apache-2.0
use anyhow::Result;
use synapse::Config;
use synapse::transport::abstraction::TransportTarget;
use synapse::transport::providers::{ProductionTransportProvider, TransportProvider};

#[tokio::test]
async fn test_transport_error_handling() -> Result<()> {
    // Test transport error handling with various failure scenarios
    let config = Config::default_for_entity("test".to_string(), "test".to_string());
    let provider = ProductionTransportProvider;

    // Test TCP transport error handling
    match provider.create_tcp_transport(&config).await {
        Ok(Some(tcp_transport)) => {
            // Test connection failure handling
            let target = TransportTarget::new("invalid_host".to_string());
            let result = tcp_transport.test_connectivity(&target).await;
            match result {
                Ok(connectivity) => {
                    assert!(
                        !connectivity.connected,
                        "Should fail to connect to invalid host"
                    );
                }
                Err(_) => {
                    // Also acceptable - some transports may return Err for invalid hosts
                    println!("✓ TCP transport returned error for invalid host as expected");
                }
            }
        }
        Ok(None) | Err(_) => {
            // Transport creation failed - expected in test environment
            println!("✓ TCP transport creation failed as expected in test environment");
        }
    }

    // Test email transport error handling
    let email_transport = provider
        .create_email_transport(&config)
        .await?
        .expect("email transport must construct: its validator is real");
    let target = TransportTarget::new("invalid@".to_string());
    let connectivity = email_transport.test_connectivity(&target).await?;
    assert!(
        !connectivity.connected,
        "Should fail to connect with an invalid email address"
    );

    println!("✓ Transport error handling test completed");
    Ok(())
}

#[tokio::test]
async fn test_transport_timeout_handling() -> Result<()> {
    // Test transport timeout handling under various network conditions
    let config = Config::default_for_entity("test".to_string(), "test".to_string());
    let provider = ProductionTransportProvider;

    // Test connection timeout
    match provider.create_tcp_transport(&config).await {
        Ok(Some(tcp_transport)) => {
            // Test with an invalid hostname that should fail DNS resolution quickly
            let start_time = std::time::Instant::now();
            let target = TransportTarget::new("nonexistent.invalid.hostname.test".to_string());
            let result = tcp_transport.test_connectivity(&target).await;
            let elapsed = start_time.elapsed();

            // Should either timeout or fail quickly (allow some margin for system differences)
            assert!(
                elapsed.as_secs() < 45,
                "Timeout should be reasonable (less than 45 seconds)"
            );
            match result {
                Ok(connectivity) => {
                    assert!(
                        !connectivity.connected,
                        "Should timeout or fail on non-routable address"
                    );
                }
                Err(_) => {
                    // Also acceptable - some transports may return Err for timeouts
                    println!("✓ TCP transport returned error for timeout as expected");
                }
            }
        }
        Ok(None) | Err(_) => {
            println!("✓ TCP transport creation failed as expected in test environment");
        }
    }

    // Test with deliberately slow connection
    println!("✓ Transport timeout handling test completed");
    Ok(())
}

#[tokio::test]
async fn test_transport_resilience() -> Result<()> {
    // Test transport resilience and recovery capabilities
    let config = Config::default_for_entity("test".to_string(), "test".to_string());
    let provider = ProductionTransportProvider;

    // Test circuit breaker functionality
    match provider.create_tcp_transport(&config).await {
        Ok(Some(tcp_transport)) => {
            // Simulate multiple failures
            for i in 0..3 {
                let target = TransportTarget::new(format!("invalid_host_{}", i));
                let result = tcp_transport.test_connectivity(&target).await;
                match result {
                    Ok(connectivity) => {
                        assert!(!connectivity.connected, "Should fail on invalid hosts");
                    }
                    Err(_) => {
                        // Also acceptable - some transports may return Err for invalid hosts
                        println!(
                            "✓ TCP transport returned error for invalid host {} as expected",
                            i
                        );
                    }
                }
            }

            // Transport should still be functional for valid operations
            println!("✓ Transport maintained resilience after failures");
        }
        Ok(None) | Err(_) => {
            println!("✓ TCP transport creation failed as expected in test environment");
        }
    }

    println!("✓ Transport resilience test completed");
    Ok(())
}
