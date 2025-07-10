//! Simple Message Sending Example
//! 
//! This example demonstrates basic message sending patterns using the Enhanced EMRP Router.
//! It shows how to initialize the router, send messages, and handle basic communication scenarios.

use synapse::{
    EnhancedEmrpRouter, Config, 
    types::{SecurityLevel, MessageType},
    transport::MessageUrgency,
    error::Result as EmrpResult,
};

#[tokio::main]
async fn main() -> EmrpResult<()> {
    println!("� Simple Message Sending Example");
    println!("=================================");
    
    // Create a basic router configuration
    let config = Config::for_testing();
    
    // Initialize the enhanced router
    let router = EnhancedEmrpRouter::new(
        config,
        "demo-bot@company.com".to_string()
    ).await?;
    
    // Start the router services
    router.start().await?;
    println!("🚀 Enhanced EMRP Router started successfully");
    
    // Example 1: Send a simple direct message
    println!("\n📝 Example 1: Sending a direct message");
    match router.send_message_smart(
        "alice@company.com",
        "Hello Alice! This is a test message from the demo bot.",
        MessageType::Direct,
        SecurityLevel::Secure,
        MessageUrgency::Interactive
    ).await {
        Ok(message_id) => println!("✅ Message sent successfully. ID: {}", message_id),
        Err(e) => println!("❌ Failed to send message: {}", e),
    }
    
    // Example 2: Send a system message
    println!("\n📝 Example 2: Sending a system message");
    match router.send_message_smart(
        "bob@company.com",
        "System notification: Demo bot is now online and ready.",
        MessageType::System,
        SecurityLevel::Authenticated,
        MessageUrgency::Background
    ).await {
        Ok(message_id) => println!("✅ System message sent successfully. ID: {}", message_id),
        Err(e) => println!("❌ Failed to send system message: {}", e),
    }
    
    // Example 3: Send a real-time message
    println!("\n📝 Example 3: Sending a real-time message");
    match router.send_message_smart(
        "charlie@company.com",
        "Real-time alert: This message requires immediate attention!",
        MessageType::Direct,
        SecurityLevel::Secure,
        MessageUrgency::RealTime
    ).await {
        Ok(message_id) => println!("✅ Real-time message sent successfully. ID: {}", message_id),
        Err(e) => println!("❌ Failed to send real-time message: {}", e),
    }
    
    // Example 4: Send a tool call message
    println!("\n📝 Example 4: Sending a tool call message");
    match router.send_message_smart(
        "ai-assistant@company.com",
        "Please analyze the latest project status report.",
        MessageType::ToolCall,
        SecurityLevel::Secure,
        MessageUrgency::Interactive
    ).await {
        Ok(message_id) => println!("✅ Tool call message sent successfully. ID: {}", message_id),
        Err(e) => println!("❌ Failed to send tool call message: {}", e),
    }
    
    // Example 5: Send a broadcast message
    println!("\n📝 Example 5: Sending a broadcast message");
    match router.send_message_smart(
        "team@company.com",
        "Broadcast: Demo bot testing is now complete.",
        MessageType::Broadcast,
        SecurityLevel::Authenticated,
        MessageUrgency::Background
    ).await {
        Ok(message_id) => println!("✅ Broadcast message sent successfully. ID: {}", message_id),
        Err(e) => println!("❌ Failed to send broadcast message: {}", e),
    }
    
    // Example 6: Test connection capabilities
    println!("\n📝 Example 6: Testing connection capabilities");
    let capabilities = router.test_connection("test-peer@company.com").await;
    println!("🔗 Connection capabilities:");
    println!("   Email: {}", capabilities.email);
    println!("   Direct TCP: {}", capabilities.direct_tcp);
    println!("   Direct UDP: {}", capabilities.direct_udp);
    println!("   mDNS Local: {}", capabilities.mdns_local);
    println!("   NAT Traversal: {}", capabilities.nat_traversal);
    println!("   Estimated Latency: {}ms", capabilities.estimated_latency_ms);
    
    // Example 7: Get router status
    println!("\n📝 Example 7: Getting router status");
    let status = router.status().await;
    println!("📊 Router Status:");
    println!("   Multi-transport enabled: {}", status.multi_transport_enabled);
    println!("   Email server enabled: {}", status.email_server_enabled);
    println!("   Available transports: {}", status.available_transports.len());
    println!("   Email configured: {}", status.emrp_status.email_configured);
    println!("   Known entities: {}", status.emrp_status.known_entities);
    
    println!("\n🎉 Example completed successfully!");
    println!("This demonstrates basic message sending patterns with the Enhanced EMRP Router:");
    println!("  1. Direct messages with different security levels");
    println!("  2. System notifications");
    println!("  3. Real-time messages");
    println!("  4. Tool call messages");
    println!("  5. Broadcast messages");
    println!("  6. Connection testing");
    println!("  7. Router status monitoring");
    
    Ok(())
}
