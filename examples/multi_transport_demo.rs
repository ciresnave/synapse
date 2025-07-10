//! Simplified multi-transport demonstration

use std::time::{Duration, Instant};
use tokio;
use synapse::{
    MessageUrgency, 
    transport::TransportRoute
};

/// Simple message structure
#[derive(Debug, Clone)]
pub struct SimpleMessage {
    pub to: String,
    pub from: String,
    pub content: String,
    pub urgency: MessageUrgency,
}

/// Multi-transport router demonstration
pub struct MultiTransportDemo {
    available_transports: Vec<TransportRoute>,
}

impl MultiTransportDemo {
    pub fn new() -> Self {
        Self {
            available_transports: vec![
                TransportRoute::DirectTcp { 
                    address: "127.0.0.1".to_string(), 
                    port: 8080, 
                    latency_ms: 25, 
                    established_at: std::time::Instant::now() 
                },
                TransportRoute::DirectUdp { 
                    address: "127.0.0.1".to_string(), 
                    port: 8081, 
                    latency_ms: 15, 
                    established_at: std::time::Instant::now() 
                },
                TransportRoute::LocalMdns { 
                    service_name: "example-service".to_string(),
                    address: "local.example.com".to_string(), 
                    port: 5353, 
                    latency_ms: 5, 
                    discovered_at: std::time::Instant::now() 
                },
                TransportRoute::FastEmailRelay { 
                    relay_server: "relay.example.com".to_string(),
                    estimated_latency_ms: 500 
                },
                TransportRoute::StandardEmail { 
                    estimated_latency_min: 1 
                },
            ],
        }
    }
    
    /// Select best transport for message urgency
    pub fn select_transport(&self, urgency: &MessageUrgency) -> TransportRoute {
        match urgency {
            MessageUrgency::RealTime => {
                // Prefer lowest latency options
                for transport in &self.available_transports {
                    match transport {
                        TransportRoute::LocalMdns { latency_ms, .. } if *latency_ms < 100 => {
                            return transport.clone();
                        }
                        TransportRoute::DirectUdp { latency_ms, .. } if *latency_ms < 100 => {
                            return transport.clone();
                        }
                        TransportRoute::DirectTcp { latency_ms, .. } if *latency_ms < 100 => {
                            return transport.clone();
                        }
                        _ => continue,
                    }
                }
                // Fallback to fastest available
                self.available_transports[0].clone()
            }
            MessageUrgency::Interactive => {
                // Accept up to 1 second latency
                for transport in &self.available_transports {
                    match transport {
                        TransportRoute::LocalMdns { latency_ms, .. } |
                        TransportRoute::DirectUdp { latency_ms, .. } |
                        TransportRoute::DirectTcp { latency_ms, .. } if *latency_ms < 1000 => {
                            return transport.clone();
                        }
                        TransportRoute::FastEmailRelay { estimated_latency_ms, .. } if *estimated_latency_ms < 1000 => {
                            return transport.clone();
                        }
                        _ => continue,
                    }
                }
                self.available_transports[0].clone()
            }
            MessageUrgency::Background => {
                // Prefer reliability (email)
                for transport in &self.available_transports {
                    match transport {
                        TransportRoute::StandardEmail { .. } => return transport.clone(),
                        TransportRoute::FastEmailRelay { .. } => return transport.clone(),
                        _ => continue,
                    }
                }
                self.available_transports.last().unwrap().clone()
            }
            MessageUrgency::Discovery => {
                // Always use standard email for discovery
                TransportRoute::StandardEmail { estimated_latency_min: 1 }
            }
        }
    }
    
    /// Send message with automatic transport selection
    pub async fn send_message(&self, message: SimpleMessage) -> Result<String, String> {
        let selected_transport = self.select_transport(&message.urgency);
        
        println!("📨 Sending message to '{}' via {:?}", message.to, selected_transport);
        println!("   Content: {}", message.content);
        
        // Simulate sending based on transport type
        let result = match &selected_transport {
            TransportRoute::DirectTcp { latency_ms, .. } => {
                tokio::time::sleep(Duration::from_millis(*latency_ms as u64)).await;
                format!("✅ Sent via TCP in {}ms", latency_ms)
            }
            TransportRoute::DirectUdp { latency_ms, .. } => {
                tokio::time::sleep(Duration::from_millis(*latency_ms as u64)).await;
                format!("✅ Sent via UDP in {}ms", latency_ms)
            }
            TransportRoute::LocalMdns { latency_ms, .. } => {
                tokio::time::sleep(Duration::from_millis(*latency_ms as u64)).await;
                format!("✅ Sent via mDNS in {}ms", latency_ms)
            }
            TransportRoute::FastEmailRelay { estimated_latency_ms, relay_server } => {
                tokio::time::sleep(Duration::from_millis(*estimated_latency_ms as u64)).await;
                format!("✅ Sent via Fast Email through {} in {}ms", relay_server, estimated_latency_ms)
            }
            TransportRoute::StandardEmail { estimated_latency_min } => {
                tokio::time::sleep(Duration::from_millis(*estimated_latency_min as u64 * 100)).await; // Simulate
                format!("✅ Sent via Standard Email in ~{}min", estimated_latency_min)
            }
            TransportRoute::Udp { latency, .. } => {
                tokio::time::sleep(*latency).await;
                format!("✅ Sent via UDP in {}ms", latency.as_millis())
            }
            TransportRoute::WebSocket { latency, .. } => {
                tokio::time::sleep(*latency).await;
                format!("✅ Sent via WebSocket in {}ms", latency.as_millis())
            }
            TransportRoute::Quic { latency, .. } => {
                tokio::time::sleep(*latency).await;
                format!("✅ Sent via QUIC in {}ms", latency.as_millis())
            }
            TransportRoute::NatTraversal { latency_ms, .. } => {
                tokio::time::sleep(Duration::from_millis(*latency_ms as u64)).await;
                format!("✅ Sent via NAT Traversal in {}ms", latency_ms)
            }
            TransportRoute::EmailDiscovery { target_transport } => {
                // Recursively handle the discovered transport
                match target_transport.as_ref() {
                    TransportRoute::DirectTcp { latency_ms, .. } => {
                        tokio::time::sleep(Duration::from_millis(*latency_ms as u64)).await;
                        format!("✅ Sent via Email Discovery -> TCP in {}ms", latency_ms)
                    }
                    _ => format!("✅ Sent via Email Discovery")
                }
            }
        };
        
        Ok(result)
    }
    
    /// Test all transport selection scenarios
    pub async fn demo_transport_selection(&self) {
        println!("🚀 Multi-Transport EMRP Demonstration\n");
        
        let test_messages = vec![
            SimpleMessage {
                to: "Alice".to_string(),
                from: "Claude".to_string(),
                content: "Real-time collaboration request".to_string(),
                urgency: MessageUrgency::RealTime,
            },
            SimpleMessage {
                to: "Bob".to_string(),
                from: "Claude".to_string(),
                content: "Interactive chat message".to_string(),
                urgency: MessageUrgency::Interactive,
            },
            SimpleMessage {
                to: "Charlie".to_string(),
                from: "Claude".to_string(),
                content: "Background task notification".to_string(),
                urgency: MessageUrgency::Background,
            },
            SimpleMessage {
                to: "unknown@example.com".to_string(),
                from: "Claude".to_string(),
                content: "Connection discovery request".to_string(),
                urgency: MessageUrgency::Discovery,
            },
        ];
        
        for message in test_messages {
            let start = Instant::now();
            match self.send_message(message).await {
                Ok(result) => {
                    let elapsed = start.elapsed();
                    println!("   Result: {}", result);
                    println!("   Actual time: {:?}\n", elapsed);
                }
                Err(e) => println!("   Error: {}\n", e),
            }
        }
    }
    
    /// Show transport capabilities
    pub fn show_capabilities(&self) {
        println!("📊 Available Transport Capabilities:");
        for (i, transport) in self.available_transports.iter().enumerate() {
            match transport {
                TransportRoute::DirectTcp { latency_ms, .. } => {
                    println!("  {}. TCP Direct: {}ms latency, high reliability", i+1, latency_ms);
                }
                TransportRoute::DirectUdp { latency_ms, .. } => {
                    println!("  {}. UDP Direct: {}ms latency, fast but lossy", i+1, latency_ms);
                }
                TransportRoute::LocalMdns { latency_ms, .. } => {
                    println!("  {}. mDNS Local: {}ms latency, LAN only", i+1, latency_ms);
                }
                TransportRoute::FastEmailRelay { estimated_latency_ms, .. } => {
                    println!("  {}. Fast Email: {}ms latency, global reach", i+1, estimated_latency_ms);
                }
                TransportRoute::StandardEmail { estimated_latency_min } => {
                    println!("  {}. Standard Email: ~{}min latency, universal compatibility", i+1, estimated_latency_min);
                }
                TransportRoute::Udp { latency, .. } => {
                    println!("  {}. UDP: {}ms latency, fast", i+1, latency.as_millis());
                }
                TransportRoute::WebSocket { latency, .. } => {
                    println!("  {}. WebSocket: {}ms latency, real-time", i+1, latency.as_millis());
                }
                TransportRoute::Quic { latency, .. } => {
                    println!("  {}. QUIC: {}ms latency, modern", i+1, latency.as_millis());
                }
                TransportRoute::NatTraversal { latency_ms, .. } => {
                    println!("  {}. NAT Traversal: {}ms latency, NAT-aware", i+1, latency_ms);
                }
                TransportRoute::EmailDiscovery { .. } => {
                    println!("  {}. Email Discovery: dynamic discovery", i+1);
                }
            }
        }
        println!();
    }
}

#[tokio::main]
async fn main() {
    println!("🌟 Email-Based Message Routing Protocol (EMRP)");
    println!("    Multi-Transport Architecture Demo\n");
    
    let router = MultiTransportDemo::new();
    
    // Show available capabilities
    router.show_capabilities();
    
    // Demonstrate intelligent transport selection
    router.demo_transport_selection().await;
    
    println!("✨ Demo completed successfully!");
    println!("\n📋 Key Features Demonstrated:");
    println!("   • Intelligent transport selection based on message urgency");
    println!("   • Multiple transport options (TCP, UDP, mDNS, Email)");
    println!("   • Automatic fallback mechanisms");
    println!("   • Real-time to background communication support");
    println!("   • Universal compatibility via email backbone");
}
