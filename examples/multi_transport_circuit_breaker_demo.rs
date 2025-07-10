//! Multi-Transport Circuit Breaker Integration Demo
//!
//! This example demonstrates the circuit breaker functionality
//! integrated across multiple transport types: mDNS, TCP, and Email.

use synapse::{
    transport::{
        Transport, 
        mdns_enhanced::{EnhancedMdnsTransport, MdnsConfig},
        tcp_enhanced::EnhancedTcpTransport,
        email_enhanced::EmailEnhancedTransport,
    },
    types::{SecureMessage, SecurityLevel, EmailConfig, SmtpConfig, ImapConfig},
    error::Result,
};
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();
    
    info!("🔄 Multi-Transport Circuit Breaker Demo Starting");
    
    // Create email config for email transport
    let email_config = EmailConfig {
        smtp: SmtpConfig {
            host: "localhost".to_string(),
            port: 587,
            username: "demo@example.com".to_string(),
            password: "demo".to_string(),
            use_ssl: false,
            use_tls: false,
        },
        imap: ImapConfig {
            host: "localhost".to_string(),
            port: 993,
            username: "demo@example.com".to_string(),
            password: "demo".to_string(),
            use_ssl: true,
        },
    };
    
    // Create transports with circuit breakers
    info!("🏗️ Creating transports with circuit breakers...");
    
    let mut mdns_transport = EnhancedMdnsTransport::new(
        "multi-demo-entity".to_string(),
        8080,
        Some(MdnsConfig::default()),
    ).await?;
    
    let tcp_transport = EnhancedTcpTransport::new(8081).await?;
    
    let email_transport = EmailEnhancedTransport::new(email_config).await?;
    
    // Start mDNS service
    mdns_transport.start().await?;
    
    info!("✅ All transports created with circuit breaker protection");
    
    // Get circuit breaker references for monitoring
    let mdns_cb = mdns_transport.get_circuit_breaker();
    let tcp_cb = tcp_transport.get_circuit_breaker();
    let email_cb = email_transport.get_circuit_breaker();
    
    // Subscribe to circuit breaker events for all transports
    let mut mdns_events = mdns_cb.subscribe_events();
    let mut tcp_events = tcp_cb.subscribe_events();
    let mut email_events = email_cb.subscribe_events();
    
    // Spawn event monitors for each transport
    let mdns_monitor = tokio::spawn(async move {
        info!("📡 Starting mDNS circuit breaker event monitor");
        while let Ok(event) = mdns_events.recv().await {
            match event {
                synapse::circuit_breaker::CircuitEvent::Opened { reason, failure_count, .. } => {
                    warn!("🔴 mDNS Circuit breaker OPENED: {} (failures: {})", reason, failure_count);
                }
                synapse::circuit_breaker::CircuitEvent::HalfOpened { .. } => {
                    info!("🟡 mDNS Circuit breaker HALF-OPENED - testing recovery");
                }
                synapse::circuit_breaker::CircuitEvent::Closed { .. } => {
                    info!("🟢 mDNS Circuit breaker CLOSED - service recovered");
                }
                synapse::circuit_breaker::CircuitEvent::RequestRejected { .. } => {
                    warn!("❌ mDNS Circuit breaker rejected request");
                }
                synapse::circuit_breaker::CircuitEvent::ExternalTriggerActivated { .. } => {
                    info!("⚡ mDNS Circuit breaker external trigger activated");
                }
            }
        }
    });
    
    let tcp_monitor = tokio::spawn(async move {
        info!("📡 Starting TCP circuit breaker event monitor");
        while let Ok(event) = tcp_events.recv().await {
            match event {
                synapse::circuit_breaker::CircuitEvent::Opened { reason, failure_count, .. } => {
                    warn!("🔴 TCP Circuit breaker OPENED: {} (failures: {})", reason, failure_count);
                }
                synapse::circuit_breaker::CircuitEvent::HalfOpened { .. } => {
                    info!("🟡 TCP Circuit breaker HALF-OPENED - testing recovery");
                }
                synapse::circuit_breaker::CircuitEvent::Closed { .. } => {
                    info!("🟢 TCP Circuit breaker CLOSED - service recovered");
                }
                synapse::circuit_breaker::CircuitEvent::RequestRejected { .. } => {
                    warn!("❌ TCP Circuit breaker rejected request");
                }
                synapse::circuit_breaker::CircuitEvent::ExternalTriggerActivated { .. } => {
                    info!("⚡ TCP Circuit breaker external trigger activated");
                }
            }
        }
    });
    
    let email_monitor = tokio::spawn(async move {
        info!("📡 Starting Email circuit breaker event monitor");
        while let Ok(event) = email_events.recv().await {
            match event {
                synapse::circuit_breaker::CircuitEvent::Opened { reason, failure_count, .. } => {
                    warn!("🔴 Email Circuit breaker OPENED: {} (failures: {})", reason, failure_count);
                }
                synapse::circuit_breaker::CircuitEvent::HalfOpened { .. } => {
                    info!("🟡 Email Circuit breaker HALF-OPENED - testing recovery");
                }
                synapse::circuit_breaker::CircuitEvent::Closed { .. } => {
                    info!("🟢 Email Circuit breaker CLOSED - service recovered");
                }
                synapse::circuit_breaker::CircuitEvent::RequestRejected { .. } => {
                    warn!("❌ Email Circuit breaker rejected request");
                }
                synapse::circuit_breaker::CircuitEvent::ExternalTriggerActivated { .. } => {
                    info!("⚡ Email Circuit breaker external trigger activated");
                }
            }
        }
    });
    
    // Test each transport with failing requests to trigger circuit breakers
    info!("🧪 Testing circuit breakers across all transports");
    
    let test_message = SecureMessage::new(
        "non-existent-target".to_string(),
        "multi-demo-entity".to_string(),
        b"Hello from multi-transport demo!".to_vec(),
        vec![], // Empty signature for demo
        SecurityLevel::Public,
    );
    
    // Test mDNS transport
    info!("🔍 Testing mDNS transport circuit breaker...");
    for i in 1..=5 {
        info!("📡 mDNS Attempt {} - testing connectivity", i);
        match mdns_transport.test_connectivity("non-existent-peer").await {
            Ok(_) => info!("✅ mDNS connectivity test succeeded"),
            Err(e) => warn!("❌ mDNS connectivity test failed: {}", e),
        }
        sleep(Duration::from_millis(500)).await;
    }
    
    // Test TCP transport
    info!("🔍 Testing TCP transport circuit breaker...");
    for i in 1..=5 {
        info!("📡 TCP Attempt {} - testing connectivity", i);
        match tcp_transport.test_connectivity("192.168.999.999:8080").await {
            Ok(_) => info!("✅ TCP connectivity test succeeded"),
            Err(e) => warn!("❌ TCP connectivity test failed: {}", e),
        }
        sleep(Duration::from_millis(500)).await;
    }
    
    // Test Email transport
    info!("🔍 Testing Email transport circuit breaker...");
    for i in 1..=5 {
        info!("📡 Email Attempt {} - testing connectivity", i);
        match email_transport.test_connectivity("nonexistent@example.com").await {
            Ok(_) => info!("✅ Email connectivity test succeeded"),
            Err(e) => warn!("❌ Email connectivity test failed: {}", e),
        }
        sleep(Duration::from_millis(500)).await;
    }
    
    // Test message sending through circuit breakers
    info!("📤 Testing message sending with circuit breaker protection");
    
    // Try mDNS
    match mdns_transport.send_message_with_breaker("non-existent-peer", &test_message, None).await {
        Ok(_) => info!("✅ mDNS message sent successfully"),
        Err(e) => warn!("❌ mDNS message sending failed: {}", e),
    }
    
    // Try TCP
    match tcp_transport.send_message_with_breaker("192.168.999.999:8080", &test_message, None).await {
        Ok(_) => info!("✅ TCP message sent successfully"),
        Err(e) => warn!("❌ TCP message sending failed: {}", e),
    }
    
    // Try Email
    match email_transport.send_message_with_breaker("nonexistent@example.com", &test_message, None).await {
        Ok(_) => info!("✅ Email message sent successfully"),
        Err(e) => warn!("❌ Email message sending failed: {}", e),
    }
    
    // Display final statistics
    sleep(Duration::from_millis(1000)).await;
    
    info!("📊 Final Circuit Breaker Statistics:");
    
    let mdns_stats = mdns_transport.get_circuit_breaker().get_stats();
    info!("  mDNS: {} requests, {} failures, {} successes, {} rejections", 
          mdns_stats.total_requests, mdns_stats.failure_count, 
          mdns_stats.success_count, mdns_stats.rejection_count);
    
    let tcp_stats = tcp_transport.get_circuit_breaker().get_stats();
    info!("  TCP: {} requests, {} failures, {} successes, {} rejections", 
          tcp_stats.total_requests, tcp_stats.failure_count, 
          tcp_stats.success_count, tcp_stats.rejection_count);
    
    let email_stats = email_transport.get_circuit_breaker_stats();
    info!("  Email: {} requests, {} failures, {} successes, {} rejections", 
          email_stats.total_requests, email_stats.failure_count, 
          email_stats.success_count, email_stats.rejection_count);
    
    info!("🏁 Multi-Transport Circuit Breaker Demo completed successfully!");
    info!("💡 Key benefits demonstrated:");
    info!("   - Circuit breaker protection across all transport types");
    info!("   - Independent failure isolation per transport");
    info!("   - Unified monitoring and statistics");
    info!("   - Automatic recovery testing");
    info!("   - Prevention of cascading failures");
    
    // Clean up monitors
    mdns_monitor.abort();
    tcp_monitor.abort();
    email_monitor.abort();
    
    Ok(())
}
