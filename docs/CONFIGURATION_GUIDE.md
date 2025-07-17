# ⚙️ Synapse Configuration Guide

This guide covers all configuration options available in the Synapse neural communication network.

## 🏗️ Configuration Overview

Synapse uses a builder pattern for configuration, allowing you to customize every aspect of the system:

```rust
use message_routing_system::*;

let config = Config::builder()
    .tcp_port(8080)
    .enable_encryption(true)
    .smtp_server("smtp.gmail.com".to_string())
    .build();
```

## 🌐 Network Configuration

### Port Settings

```rust
let config = Config::builder()
    // TCP port for direct connections
    .tcp_port(8080)                     // Default: 8080
    
    // UDP port for fast messaging
    .udp_port(8081)                     // Default: 8081
    
    // Email server ports
    .email_server_port(2525)            // Default: 2525 (non-standard for testing)
    .smtp_port(587)                     // Default: 587 (submission)
    .imap_port(993)                     // Default: 993 (secure IMAP)
    
    .build();
```

### Transport Enablement

```rust
let config = Config::builder()
    // Enable/disable transport types
    .enable_tcp_transport(true)         // Default: true
    .enable_udp_transport(true)         // Default: true
    .enable_email_transport(true)       // Default: true
    .enable_mdns(true)                  // Default: true (local discovery)
    
    // Advanced networking
    .enable_nat_traversal(true)         // Default: false
    .enable_upnp(true)                  // Default: false
    .enable_ipv6(false)                 // Default: true
    
    .build();
```

### External Network Settings

```rust
let config = Config::builder()
    // External IP (for NAT/firewall scenarios)
    .external_ip("203.0.113.1".to_string())
    
    // STUN servers for NAT traversal
    .stun_servers(vec![
        "stun.l.google.com:19302".to_string(),
        "stun1.l.google.com:19302".to_string(),
        "stun.cloudflare.com:3478".to_string(),
    ])
    
    // Discovery servers
    .discovery_servers(vec![
        "discovery.emrp.org:8080".to_string(),
    ])
    
    .build();
```

## 📧 Email Configuration

### SMTP Settings (Outgoing Email)

```rust
let config = Config::builder()
    // SMTP server configuration
    .smtp_server("smtp.gmail.com".to_string())
    .smtp_port(587)
    .smtp_use_tls(true)                 // Default: true
    .smtp_use_starttls(true)            // Default: true
    
    // Authentication
    .email_username("mybot@gmail.com".to_string())
    .email_password("app_password".to_string())
    
    // Alternative: OAuth2 (when supported)
    .smtp_oauth2_token("oauth_token".to_string())
    
    .build();
```

### IMAP Settings (Incoming Email)

```rust
let config = Config::builder()
    // IMAP server configuration
    .imap_server("imap.gmail.com".to_string())
    .imap_port(993)
    .imap_use_tls(true)                 // Default: true
    
    // Mailbox settings
    .imap_inbox_folder("INBOX".to_string())
    .imap_sent_folder("Sent".to_string())
    .imap_trash_folder("Trash".to_string())
    
    // Polling configuration
    .email_poll_interval(Duration::from_secs(30))  // Default: 30s
    .email_batch_size(50)               // Default: 50 messages per fetch
    
    .build();
```

### Email Server Mode

```rust
let config = Config::builder()
    // Run local email server
    .email_server_mode(EmailServerMode::Full)       // Full SMTP+IMAP
    // .email_server_mode(EmailServerMode::SmtpOnly) // Outgoing only
    // .email_server_mode(EmailServerMode::Relay)    // Relay through external
    // .email_server_mode(EmailServerMode::External) // Use external provider only
    
    // Local server domains
    .server_domain("mycompany.com".to_string())
    .server_hostname("mail.mycompany.com".to_string())
    
    // TLS configuration for local server
    .tls_cert_path("/etc/ssl/certs/emrp.crt".to_string())
    .tls_key_path("/etc/ssl/private/emrp.key".to_string())
    
    .build();
```

## 🔒 Security Configuration

### Encryption Settings

```rust
let config = Config::builder()
    // Enable/disable encryption
    .enable_encryption(true)            // Default: true
    .require_encryption(true)           // Default: false
    
    // Encryption algorithms
    .symmetric_cipher(SymmetricCipher::AES256)      // Default: AES256
    .asymmetric_cipher(AsymmetricCipher::RSA4096)   // Default: RSA2048
    .hash_algorithm(HashAlgorithm::SHA256)          // Default: SHA256
    
    // Key management
    .key_size(4096)                     // Default: 2048
    .key_rotation_interval(Duration::from_secs(3600 * 24))  // Default: 24h
    
    .build();
```

### Authentication Settings

```rust
let config = Config::builder()
    // Authentication requirements
    .require_authentication(true)       // Default: false
    .allow_anonymous_discovery(false)   // Default: true
    
    // Authentication methods
    .auth_methods(vec![
        AuthMethod::PublicKey,
        AuthMethod::Certificate,
        AuthMethod::SharedSecret,
    ])
    
    // PKI configuration
    .ca_cert_path("/etc/ssl/ca.crt".to_string())
    .client_cert_path("/etc/ssl/client.crt".to_string())
    .client_key_path("/etc/ssl/client.key".to_string())
    
    .build();
```

### Trust and Verification

```rust
let config = Config::builder()
    // Trust levels
    .default_trust_level(TrustLevel::Verified)
    .allow_self_signed(false)           // Default: true (development)
    .verify_peer_certificates(true)     // Default: false
    
    // Known hosts/peers
    .trusted_peers(vec![
        "alice@company.com".to_string(),
        "bot@trusted-domain.com".to_string(),
    ])
    
    // Blocked entities
    .blocked_peers(vec![
        "spam@bad-domain.com".to_string(),
    ])
    
    .build();
```

## ⚡ Performance Configuration

### Connection Settings

```rust
let config = Config::builder()
    // Timeouts
    .connection_timeout(Duration::from_secs(10))    // Default: 10s
    .read_timeout(Duration::from_secs(30))          // Default: 30s
    .write_timeout(Duration::from_secs(30))         // Default: 30s
    
    // Retry configuration
    .max_retries(3)                     // Default: 3
    .retry_delay(Duration::from_millis(500))        // Default: 500ms
    .backoff_multiplier(2.0)            // Default: 2.0
    
    // Keep-alive
    .tcp_keepalive(true)                // Default: true
    .keepalive_interval(Duration::from_secs(60))    // Default: 60s
    
    .build();
```

### Threading and Concurrency

```rust
let config = Config::builder()
    // Thread pool sizes
    .worker_threads(4)                  // Default: CPU count
    .io_threads(2)                      // Default: CPU count / 2
    .max_concurrent_connections(100)    // Default: 100
    
    // Buffer sizes
    .send_buffer_size(8192)             // Default: 8KB
    .receive_buffer_size(8192)          // Default: 8KB
    .message_queue_size(1000)           // Default: 1000
    
    .build();
```

### Caching and Storage

```rust
let config = Config::builder()
    // Message caching
    .enable_message_cache(true)         // Default: true
    .message_cache_size(1000)           // Default: 1000 messages
    .message_cache_ttl(Duration::from_secs(3600))   // Default: 1 hour
    
    // Identity caching
    .identity_cache_size(500)           // Default: 500 identities
    .identity_cache_ttl(Duration::from_secs(1800))  // Default: 30 minutes
    
    // Persistent storage
    .data_directory("/var/lib/emrp".to_string())
    .enable_persistent_storage(true)    // Default: false
    
    .build();
```

## 🔍 Discovery Configuration

### Local Network Discovery

```rust
let config = Config::builder()
    // mDNS configuration
    .mdns_service_name("_synapse._tcp.local.".to_string())
    .mdns_domain("local.".to_string())
    .mdns_ttl(120)                      // Default: 120 seconds
    
    // Discovery intervals
    .discovery_interval(Duration::from_secs(30))    // Default: 30s
    .discovery_timeout(Duration::from_secs(5))      // Default: 5s
    
    // Network scanning
    .enable_lan_scanning(true)          // Default: false
    .lan_scan_ports(vec![8080, 8081, 8082])
    
    .build();
```

### Global Discovery

```rust
let config = Config::builder()
    // DNS-based discovery
    .enable_dns_discovery(true)         // Default: true
    .dns_discovery_domain("_synapse._tcp.example.com".to_string())
    
    // Registry servers
    .registry_servers(vec![
        "registry.emrp.org:443".to_string(),
    ])
    
    // Peer exchange
    .enable_peer_exchange(true)         // Default: true
    .max_peer_exchange_size(10)         // Default: 10
    
    .build();
```

## 📊 Monitoring and Logging

### Logging Configuration

```rust
let config = Config::builder()
    // Log levels
    .log_level(LogLevel::Info)          // Default: Info
    .enable_debug_logging(false)        // Default: false
    
    // Log destinations
    .log_to_file(true)                  // Default: false
    .log_file_path("/var/log/emrp.log".to_string())
    .log_rotation_size(10 * 1024 * 1024)   // Default: 10MB
    
    // Structured logging
    .enable_json_logging(false)         // Default: false
    .enable_tracing(true)               // Default: true
    
    .build();
```

### Metrics and Monitoring

```rust
let config = Config::builder()
    // Metrics collection
    .enable_metrics(true)               // Default: false
    .metrics_port(9090)                 // Default: 9090 (Prometheus)
    .metrics_path("/metrics".to_string())
    
    // Health checks
    .enable_health_checks(true)         // Default: true
    .health_check_interval(Duration::from_secs(60))    // Default: 60s
    
    // Performance monitoring
    .enable_latency_tracking(true)      // Default: false
    .enable_bandwidth_monitoring(true)  // Default: false
    
    .build();
```

## 🔧 Development and Testing

### Development Mode

```rust
let config = Config::builder()
    // Development settings
    .development_mode(true)             // Default: false
    .allow_insecure_connections(true)   // Default: false in production
    .disable_certificate_verification(true)    // Default: false
    
    // Mock services
    .use_mock_email(true)               // Default: false
    .use_mock_dns(true)                 // Default: false
    
    // Testing features
    .enable_test_endpoints(true)        // Default: false
    .test_message_latency(Duration::from_millis(100))   // Simulate latency
    
    .build();
```

### Testing Configuration

```rust
impl Config {
    pub fn test_config() -> ConfigBuilder {
        Config::builder()
            .tcp_port(0)                    // Random port
            .udp_port(0)                    // Random port
            .email_server_port(0)           // Random port
            .use_mock_email(true)
            .use_mock_dns(true)
            .development_mode(true)
            .log_level(LogLevel::Debug)
    }
    
    pub fn integration_test_config() -> ConfigBuilder {
        Config::builder()
            .tcp_port(8080)
            .udp_port(8081)
            .enable_encryption(false)       // Faster for testing
            .connection_timeout(Duration::from_secs(1))
            .max_retries(1)
    }
}
```

## 🌍 Environment-Specific Configurations

### Local Development

```rust
let config = Config::builder()
    .tcp_port(8080)
    .udp_port(8081)
    .enable_mdns(true)
    .email_server_mode(EmailServerMode::External)
    .smtp_server("localhost".to_string())
    .smtp_port(1025)  // MailHog or similar
    .development_mode(true)
    .log_level(LogLevel::Debug)
    .build();
```

### Docker Container

```rust
let config = Config::builder()
    .tcp_port(std::env::var("TCP_PORT")?.parse()?)
    .external_ip(std::env::var("EXTERNAL_IP").ok())
    .data_directory("/data".to_string())
    .log_to_file(false)  // Use container logging
    .enable_health_checks(true)
    .health_check_port(8090)
    .build();
```

### Kubernetes Deployment

```rust
let config = Config::builder()
    .tcp_port(8080)
    .enable_nat_traversal(false)  // Service mesh handles routing
    .registry_servers(vec![
        "emrp-registry.default.svc.cluster.local:80".to_string()
    ])
    .enable_metrics(true)
    .metrics_port(9090)
    .build();
```

### Production Server

```rust
let config = Config::builder()
    .tcp_port(443)
    .enable_tls(true)
    .tls_cert_path("/etc/ssl/certs/emrp.crt".to_string())
    .tls_key_path("/etc/ssl/private/emrp.key".to_string())
    .require_encryption(true)
    .require_authentication(true)
    .log_level(LogLevel::Info)
    .enable_metrics(true)
    .data_directory("/var/lib/emrp".to_string())
    .max_concurrent_connections(1000)
    .build();
```

## 📝 Configuration File Format

EMRP supports configuration files in TOML format:

```toml
# emrp.toml
[network]
tcp_port = 8080
udp_port = 8081
enable_mdns = true
external_ip = "203.0.113.1"

[email]
smtp_server = "smtp.gmail.com"
smtp_port = 587
username = "mybot@gmail.com"
password = "app_password"

[security]
enable_encryption = true
require_authentication = true
key_size = 4096

[performance]
max_retries = 3
connection_timeout = "10s"
worker_threads = 4

[logging]
level = "info"
enable_file_logging = true
log_path = "/var/log/emrp.log"
```

Load configuration from file:

```rust
let config = Config::from_file("emrp.toml")?;
```

## 🔄 Environment Variables

Common environment variables supported:

```bash
# Network
EMRP_TCP_PORT=8080
EMRP_UDP_PORT=8081
EMRP_EXTERNAL_IP=203.0.113.1

# Email
EMRP_SMTP_SERVER=smtp.gmail.com
EMRP_EMAIL_USERNAME=mybot@gmail.com
EMRP_EMAIL_PASSWORD=app_password

# Security
EMRP_ENABLE_ENCRYPTION=true
EMRP_REQUIRE_AUTH=true

# Logging
EMRP_LOG_LEVEL=info
RUST_LOG=message_routing_system=debug
```

Load from environment:

```rust
let config = Config::from_env()?;
```

## 🚀 Best Practices

1. **Use different ports for different environments** to avoid conflicts
2. **Enable encryption in production** but consider disabling for development speed
3. **Configure appropriate timeouts** based on your network conditions
4. **Use configuration files** for complex deployments
5. **Monitor resource usage** and adjust thread/buffer sizes accordingly
6. **Test your configuration** in staging environments before production
7. **Use secrets management** for passwords and keys in production
8. **Enable monitoring and health checks** for production deployments

This configuration system provides maximum flexibility while maintaining sensible defaults for easy setup!
