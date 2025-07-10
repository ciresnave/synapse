# 🔧 EMRP Troubleshooting Guide

This guide helps you diagnose and solve common issues when working with the Email-Based Message Routing Protocol (EMRP).

## 🚨 Common Issues and Solutions

### 1. Connection Issues

#### Problem: "Failed to establish TCP connection"

**Symptoms:**
```
Error: Transport error: Failed to establish TCP connection to 192.168.1.100:8080
```

**Possible Causes & Solutions:**

1. **Port is blocked by firewall**
   ```bash
   # Check if port is open (Windows)
   netstat -an | findstr :8080
   
   # Check if port is open (Linux/Mac)
   netstat -an | grep :8080
   ```
   **Solution:** Configure firewall to allow the port or use a different port.

2. **Target peer is not running**
   ```rust
   // Check peer status before sending
   let status = router.check_peer_status("Alice").await?;
   if !status.is_reachable {
       println!("Peer Alice is not reachable via direct connection");
   }
   ```

3. **Network connectivity issues**
   ```rust
   // Enable fallback to email transport
   let config = Config::builder()
       .enable_transport_fallback(true)
       .fallback_to_email(true)
       .build();
   ```

#### Problem: "UDP packets not reaching destination"

**Symptoms:**
```
Warning: UDP message to 192.168.1.100:8081 timed out, falling back to TCP
```

**Solutions:**

1. **Check UDP port availability**
   ```rust
   // Test UDP connectivity
   let result = router.test_udp_connectivity("192.168.1.100:8081").await;
   match result {
       Ok(_) => println!("UDP connectivity OK"),
       Err(e) => println!("UDP issue: {}", e),
   }
   ```

2. **Router/NAT configuration**
   - Ensure UDP ports are forwarded in router settings
   - Enable UPnP if supported:
   ```rust
   let config = Config::builder()
       .enable_upnp(true)
       .enable_nat_traversal(true)
       .build();
   ```

### 2. Email Transport Issues

#### Problem: "SMTP authentication failed"

**Symptoms:**
```
Error: Email transport error: SMTP authentication failed: Invalid credentials
```

**Solutions:**

1. **Verify credentials**
   ```rust
   // Test SMTP connection
   let result = router.test_smtp_connection().await;
   match result {
       Ok(_) => println!("SMTP connection successful"),
       Err(e) => println!("SMTP error: {}", e),
   }
   ```

2. **Check app-specific passwords**
   - Gmail: Use app-specific password, not regular password
   - Outlook: Enable SMTP in account settings
   
3. **Common email provider settings**
   ```rust
   // Gmail
   let config = Config::builder()
       .smtp_server("smtp.gmail.com".to_string())
       .smtp_port(587)
       .smtp_use_starttls(true)
       .build();
   
   // Outlook
   let config = Config::builder()
       .smtp_server("smtp-mail.outlook.com".to_string())
       .smtp_port(587)
       .smtp_use_starttls(true)
       .build();
   ```

#### Problem: "IMAP connection timeout"

**Symptoms:**
```
Error: IMAP connection timeout after 30 seconds
```

**Solutions:**

1. **Increase timeout**
   ```rust
   let config = Config::builder()
       .imap_connection_timeout(Duration::from_secs(60))
       .build();
   ```

2. **Check IMAP settings**
   ```rust
   // Test IMAP connectivity
   let result = router.test_imap_connection().await;
   if result.is_err() {
       // Fall back to POP3 or disable incoming email
       let config = Config::builder()
           .disable_imap(true)
           .enable_smtp_only_mode(true)
           .build();
   }
   ```

### 3. Identity and Discovery Issues

#### Problem: "Peer not found" or "Name resolution failed"

**Symptoms:**
```
Error: Identity resolution failed: No peer found with name 'Alice'
```

**Solutions:**

1. **Check identity registry**
   ```rust
   // List all registered peers
   let peers = router.list_registered_peers().await?;
   for peer in peers {
       println!("Registered: {} -> {}", peer.local_name, peer.global_id);
   }
   
   // Manually register missing peer
   router.register_peer("Alice", "alice@company.com").await?;
   ```

2. **Enable auto-discovery**
   ```rust
   let config = Config::builder()
       .enable_mdns(true)
       .enable_dns_discovery(true)
       .discovery_interval(Duration::from_secs(30))
       .build();
   
   // Start peer discovery
   router.start_peer_discovery().await?;
   ```

3. **Check DNS resolution**
   ```rust
   // Test DNS resolution
   let result = router.resolve_peer_dns("alice@company.com").await;
   match result {
       Ok(addresses) => println!("DNS resolved to: {:?}", addresses),
       Err(e) => println!("DNS resolution failed: {}", e),
   }
   ```

#### Problem: "mDNS discovery not working"

**Symptoms:**
```
Warning: mDNS discovery found 0 peers on local network
```

**Solutions:**

1. **Check mDNS support**
   ```rust
   // Verify mDNS is working
   let result = router.test_mdns_functionality().await;
   if result.is_err() {
       println!("mDNS not supported on this network");
       // Use manual peer registration instead
   }
   ```

2. **Network configuration**
   - Ensure multicast is enabled on network interface
   - Check if corporate firewall blocks mDNS (port 5353)
   
3. **Alternative discovery methods**
   ```rust
   let config = Config::builder()
       .enable_lan_scanning(true)  // Scan common ports
       .lan_scan_ports(vec![8080, 8081, 8082])
       .build();
   ```

### 4. Security and Encryption Issues

#### Problem: "Encryption key not found"

**Symptoms:**
```
Error: Crypto error: No private key found for decryption
```

**Solutions:**

1. **Generate or import keys**
   ```rust
   // Generate new key pair
   router.generate_key_pair().await?;
   
   // Or import existing keys
   router.import_private_key(&private_key_data).await?;
   router.import_public_key("alice@company.com", &alice_public_key).await?;
   ```

2. **Check key storage**
   ```rust
   // Verify keys are properly stored
   let key_status = router.get_crypto_status().await?;
   println!("Private key available: {}", key_status.has_private_key);
   println!("Public keys count: {}", key_status.public_key_count);
   ```

#### Problem: "Message signature verification failed"

**Symptoms:**
```
Warning: Message signature verification failed for sender 'alice@company.com'
```

**Solutions:**

1. **Update public keys**
   ```rust
   // Refresh public key for sender
   router.refresh_public_key("alice@company.com").await?;
   
   // Or disable signature verification temporarily
   let config = Config::builder()
       .require_signature_verification(false)
       .build();
   ```

2. **Check time synchronization**
   - Ensure system clocks are synchronized (signatures include timestamps)

### 5. Performance Issues

#### Problem: "Messages are slow to send/receive"

**Symptoms:**
```
Warning: Message delivery took 5.2 seconds (expected <1s)
```

**Diagnostic Steps:**

1. **Check transport selection**
   ```rust
   // Monitor which transport is being used
   let metrics = router.get_transport_metrics().await?;
   for (transport, stats) in metrics {
       println!("{}: avg_latency={}ms, usage={}%", 
                transport, stats.avg_latency_ms, stats.usage_percentage);
   }
   ```

2. **Optimize transport preferences**
   ```rust
   // Prefer faster transports
   let config = Config::builder()
       .transport_preference_order(vec![
           TransportType::UDP,
           TransportType::TCP,
           TransportType::Email,
       ])
       .build();
   ```

3. **Tune performance settings**
   ```rust
   let config = Config::builder()
       .connection_timeout(Duration::from_secs(5))  // Faster timeout
       .max_retries(2)                              // Fewer retries
       .worker_threads(8)                           // More threads
       .send_buffer_size(16384)                     // Larger buffers
       .build();
   ```

#### Problem: "High memory usage"

**Solutions:**

1. **Tune cache sizes**
   ```rust
   let config = Config::builder()
       .message_cache_size(500)        // Reduce from default 1000
       .identity_cache_size(250)       // Reduce from default 500
       .build();
   ```

2. **Enable garbage collection**
   ```rust
   // Periodically clean up old data
   router.cleanup_old_messages(Duration::from_secs(3600)).await?;
   router.cleanup_old_identities(Duration::from_secs(7200)).await?;
   ```

### 6. Platform-Specific Issues

#### Windows Issues

1. **Firewall blocking connections**
   ```powershell
   # Allow EMRP through Windows Firewall
   netsh advfirewall firewall add rule name="EMRP TCP" dir=in action=allow protocol=TCP localport=8080
   netsh advfirewall firewall add rule name="EMRP UDP" dir=in action=allow protocol=UDP localport=8081
   ```

2. **Permission issues with email clients**
   - Run as Administrator for initial setup
   - Configure Windows Defender to allow EMRP

#### Linux Issues

1. **Port permission issues**
   ```bash
   # Use capabilities instead of running as root
   sudo setcap 'cap_net_bind_service=+ep' /path/to/emrp-binary
   ```

2. **systemd service configuration**
   ```ini
   [Unit]
   Description=EMRP Router
   After=network.target
   
   [Service]
   Type=simple
   User=emrp
   WorkingDirectory=/opt/emrp
   ExecStart=/opt/emrp/emrp-router --config /etc/emrp/config.toml
   Restart=always
   
   [Install]
   WantedBy=multi-user.target
   ```

#### macOS Issues

1. **Network permissions**
   - Grant network access permissions in System Preferences
   - Allow EMRP in macOS Firewall settings

## 🔍 Debugging Tools and Commands

### Built-in Diagnostic Commands

```rust
// Get comprehensive system status
let status = router.get_diagnostic_info().await?;
println!("{:#?}", status);

// Test all transports
let transport_tests = router.test_all_transports().await?;
for (transport, result) in transport_tests {
    println!("{}: {}", transport, if result.is_ok() { "✅" } else { "❌" });
}

// Network connectivity test
let connectivity = router.test_network_connectivity().await?;
println!("Internet: {}, LAN: {}, Email: {}", 
         connectivity.internet, connectivity.lan, connectivity.email);
```

### Logging Configuration for Debugging

```rust
// Enable maximum logging
let config = Config::builder()
    .log_level(LogLevel::Trace)
    .enable_debug_logging(true)
    .log_to_file(true)
    .log_file_path("emrp_debug.log".to_string())
    .build();

// Or use environment variable
std::env::set_var("RUST_LOG", "message_routing_system=trace");
```

### Performance Monitoring

```rust
// Enable detailed metrics
let config = Config::builder()
    .enable_metrics(true)
    .enable_latency_tracking(true)
    .enable_bandwidth_monitoring(true)
    .build();

// Monitor in real-time
tokio::spawn(async move {
    loop {
        let metrics = router.get_real_time_metrics().await.unwrap();
        println!("CPU: {}%, Memory: {}MB, Messages/s: {}", 
                 metrics.cpu_usage, metrics.memory_mb, metrics.message_rate);
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
});
```

## 🧪 Testing and Validation

### Connection Testing

```rust
#[tokio::test]
async fn test_basic_connectivity() {
    let config = Config::test_config().build();
    let router = EnhancedEmrpRouter::new(config, "test@example.com".to_string()).await.unwrap();
    
    // Test each transport individually
    assert!(router.test_tcp_transport().await.is_ok());
    assert!(router.test_udp_transport().await.is_ok());
    assert!(router.test_email_transport().await.is_ok());
}
```

### End-to-End Testing

```rust
#[tokio::test]
async fn test_message_roundtrip() {
    // Set up two routers
    let config1 = Config::test_config().tcp_port(8080).build();
    let config2 = Config::test_config().tcp_port(8081).build();
    
    let router1 = EnhancedEmrpRouter::new(config1, "alice@test.com".to_string()).await.unwrap();
    let router2 = EnhancedEmrpRouter::new(config2, "bob@test.com".to_string()).await.unwrap();
    
    // Cross-register
    router1.register_peer("Bob", "bob@test.com").await.unwrap();
    router2.register_peer("Alice", "alice@test.com").await.unwrap();
    
    // Test message sending
    router1.send_message_smart("Bob", "Hello!", MessageType::Direct, 
                              SecurityLevel::Basic, MessageUrgency::Interactive).await.unwrap();
    
    // Verify receipt
    let mut receiver = router2.get_message_receiver().await.unwrap();
    let message = tokio::time::timeout(Duration::from_secs(5), receiver.recv()).await.unwrap().unwrap();
    assert_eq!(message.content, "Hello!");
}
```

## 📞 Getting Help

### Check Project Resources

1. **API Documentation**: Run `cargo doc --open` for full API reference
2. **Examples**: Check the `examples/` directory for working code
3. **Integration Tests**: Look at `tests/` for real-world usage patterns

### Enable Verbose Logging

```bash
# Set environment variable for detailed logs
export RUST_LOG=message_routing_system=debug

# Or for maximum verbosity
export RUST_LOG=trace
```

### Create Minimal Reproduction

When reporting issues, create a minimal example:

```rust
use message_routing_system::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Minimal configuration that reproduces the issue
    let config = Config::builder()
        .tcp_port(8080)
        .log_level(LogLevel::Debug)
        .build();
    
    let router = EnhancedEmrpRouter::new(config, "test@example.com".to_string()).await?;
    
    // Steps that trigger the problem
    router.start().await?;
    
    // ... problematic operation here ...
    
    Ok(())
}
```

### System Information for Bug Reports

Include this information when reporting issues:

```rust
// Get system info for bug reports
let system_info = router.get_system_info().await?;
println!("OS: {}", system_info.os);
println!("Rust Version: {}", system_info.rust_version);
println!("EMRP Version: {}", system_info.emrp_version);
println!("Network Interfaces: {:#?}", system_info.network_interfaces);
```

Remember: Most EMRP issues are configuration-related. Double-check your network settings, firewall rules, and authentication credentials before diving into complex debugging!
