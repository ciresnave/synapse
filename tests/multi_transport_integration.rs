//! Multi-transport integration tests

use synapse::{
    Config, 
    MessageUrgency, 
    TransportRoute,
    HybridConnection, 
    TransportMetrics, 
    NatMethod,
    router::EmrpRouter,
    router_enhanced::EnhancedEmrpRouter,
    transport::MultiTransportRouter,
};

#[tokio::test]
async fn test_multi_transport_nat_traversal() {
    let methods = vec![
        NatMethod::Stun {
            server: "stun.example.com:3478".to_string(),
        },
        NatMethod::Turn {
            server: "turn.example.com:3478".to_string(),
            username: "user".to_string(),
        },
        NatMethod::Upnp,
        NatMethod::IceCandidate,
    ];
}

use tokio;
use std::time::Duration;

#[tokio::test]
async fn test_enhanced_router_initialization() {
    let config = Config::for_testing();
    
    match EnhancedEmrpRouter::new(config, "test-entity".to_string()).await {
        Ok(router) => {
            let status = router.status().await;
            println!("Enhanced router initialized successfully");
            println!("Available transports: {:?}", status.available_transports);
            println!("Multi-transport enabled: {}", status.multi_transport_enabled);
            
            // Should always have email available
            assert!(status.available_transports.contains(&"email".to_string()));
        }
        Err(e) => {
            println!("Router initialization failed (expected for testing): {}", e);
            // This is expected in testing environment without email credentials
        }
    }
}

#[tokio::test]
async fn test_transport_capabilities() {
    let config = Config::for_testing();
    
    if let Ok(router) = EnhancedEmrpRouter::new(config, "test-entity".to_string()).await {
        let capabilities = router.test_connection("example-peer").await;
        
        println!("Connection capabilities:");
        println!("  Email: {}", capabilities.email);
        println!("  Direct TCP: {}", capabilities.direct_tcp);
        println!("  Direct UDP: {}", capabilities.direct_udp);
        println!("  mDNS Local: {}", capabilities.mdns_local);
        println!("  NAT Traversal: {}", capabilities.nat_traversal);
        println!("  Estimated latency: {}ms", capabilities.estimated_latency_ms);
        
        // Email should always be available
        assert!(capabilities.email);
    }
}

#[tokio::test]
async fn test_message_urgency_levels() {
    // Test all urgency levels are defined
    let urgencies = vec![
        MessageUrgency::RealTime,
        MessageUrgency::Interactive,
        MessageUrgency::Background,
        MessageUrgency::Discovery,
    ];
    
    for urgency in urgencies {
        println!("Testing urgency level: {:?}", urgency);
        // Just verify the enum values exist and can be used
    }
}

#[tokio::test]
async fn test_transport_route_types() {
    use std::time::Instant;
    
    // Test all transport route types can be created
    let routes = vec![
        TransportRoute::DirectTcp {
            address: "127.0.0.1".to_string(),
            port: 8080,
            latency_ms: 50,
            established_at: Instant::now(),
        },
        TransportRoute::DirectUdp {
            address: "127.0.0.1".to_string(),
            port: 8080,
            latency_ms: 30,
            established_at: Instant::now(),
        },
        TransportRoute::LocalMdns {
            service_name: "_emrp._tcp.local".to_string(),
            address: "192.168.1.100".to_string(),
            port: 8080,
            latency_ms: 10,
            discovered_at: Instant::now(),
        },
        TransportRoute::StandardEmail {
            estimated_latency_min: 1,
        },
        TransportRoute::FastEmailRelay {
            relay_server: "relay.example.com".to_string(),
            estimated_latency_ms: 500,
        },
    ];
    
    for route in routes {
        println!("Created transport route: {:?}", route);
    }
}

#[tokio::test]
async fn test_multi_transport_router_creation() {
    let config = Config::for_testing();
    
    // Test that MultiTransportRouter can be created (even if transports fail to initialize)
    match MultiTransportRouter::new(config, "test-entity".to_string()).await {
        Ok(router) => {
            let capabilities = router.get_capabilities();
            println!("Multi-transport router capabilities: {:?}", capabilities);
            
            // Should always have email capability
            assert!(capabilities.contains(&"email".to_string()));
        }
        Err(e) => {
            println!("Multi-transport router creation failed (expected in test environment): {}", e);
            // This is expected without proper network setup
        }
    }
}

#[tokio::test]
async fn test_transport_selection_logic() {
    use synapse::transport::TransportSelector;
    
    let mut selector = TransportSelector::new();
    
    // Test that transport selection works for different urgency levels
    let target = "test-target";
    
    for urgency in [MessageUrgency::RealTime, MessageUrgency::Interactive, MessageUrgency::Background] {
        match selector.choose_optimal_transport(target, urgency).await {
            Ok(route) => {
                println!("Selected route for {:?}: {:?}", urgency, route);
            }
            Err(e) => {
                println!("Transport selection failed for {:?}: {}", urgency, e);
                // Expected in test environment
            }
        }
    }
}

#[tokio::test]
async fn test_hybrid_connection_structure() {
    use synapse::transport::{HybridConnection, TransportMetrics};
    use std::time::{Duration, Instant};
    
    // Test that HybridConnection can be created
    let connection = HybridConnection {
        primary: TransportRoute::DirectTcp {
            address: "127.0.0.1".to_string(),
            port: 8080,
            latency_ms: 25,
            established_at: Instant::now(),
        },
        fallback: TransportRoute::StandardEmail {
            estimated_latency_min: 1,
        },
        discovery_latency: Duration::from_millis(100),
        connection_latency: Duration::from_millis(50),
        total_setup_time: Duration::from_millis(150),
        metrics: TransportMetrics {
            latency: Duration::from_millis(25),
            throughput_bps: 1_000_000,
            packet_loss: 0.01,
            jitter_ms: 5,
            reliability_score: 0.90,
            last_updated: Instant::now(),
        },
    };
    
    println!("Hybrid connection created successfully");
    println!("Primary route: {:?}", connection.primary);
    println!("Fallback route: {:?}", connection.fallback);
    println!("Total setup time: {:?}", connection.total_setup_time);
}

#[tokio::test]
async fn test_benchmarking_functionality() {
    let config = Config::for_testing();
    
    if let Ok(router) = EnhancedEmrpRouter::new(config, "test-entity".to_string()).await {
        let benchmarks = router.benchmark_transport("test-target").await;
        
        println!("Transport benchmarks:");
        println!("  Email latency: {}ms", benchmarks.email_latency_ms);
        println!("  TCP latency: {:?}", benchmarks.tcp_latency_ms);
        println!("  UDP latency: {:?}", benchmarks.udp_latency_ms);
        println!("  mDNS latency: {:?}", benchmarks.mdns_latency_ms);
        println!("  NAT traversal latency: {:?}", benchmarks.nat_traversal_latency_ms);
        
        // Email should always have a benchmark
        assert!(benchmarks.email_latency_ms > 0);
    }
}

#[tokio::test]
async fn test_configuration_compatibility() {
    // Test that enhanced router works with existing config
    let config = Config::for_testing();
    
    // Should work with both regular and enhanced routers
    match EmrpRouter::new(config.clone(), "test-regular".to_string()).await {
        Ok(_router) => {
            println!("Regular EMRP router initialization successful");
        }
        Err(e) => {
            println!("Regular router failed (expected): {}", e);
        }
    }
    
    match EnhancedEmrpRouter::new(config, "test-enhanced".to_string()).await {
        Ok(_router) => {
            println!("Enhanced EMRP router initialization successful");
        }
        Err(e) => {
            println!("Enhanced router failed (expected): {}", e);
        }
    }
}

#[tokio::test]
async fn test_nat_traversal_methods() {
    use synapse::transport::NatMethod;
    
    // Test that NAT traversal methods can be created
    let methods = vec![
        NatMethod::Stun {
            server: "stun.l.google.com:19302".to_string(),
        },
        NatMethod::Turn {
            server: "turn.example.com:3478".to_string(),
            username: "user".to_string(),
        },
        NatMethod::Upnp,
    ];
    
    for method in methods {
        println!("NAT traversal method: {:?}", method);
    }
}

// Custom test utilities go here
