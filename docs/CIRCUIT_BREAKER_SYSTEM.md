# ⚡ Synapse Circuit Breaker System

## Overview

The Synapse Circuit Breaker System provides comprehensive protection against cascading failures across all transport layers. This system implements the classic circuit breaker pattern with advanced features including external triggers, performance monitoring, and intelligent recovery mechanisms.

## 🎯 Key Features

### 1. Automatic Failure Protection

- **Threshold-based Protection**: Configurable failure thresholds trigger circuit opening
- **Request Rejection**: Failed requests are immediately rejected when circuit is open
- **Cascading Failure Prevention**: Isolates failures to prevent system-wide issues
- **Resource Protection**: Prevents overload of downstream services

### 2. Intelligent Recovery

- **Half-Open Testing**: Gradual recovery testing after timeout period
- **Success Rate Monitoring**: Evidence-based circuit closure decisions
- **Adaptive Timeouts**: Recovery timeout adjusts based on failure patterns
- **Progressive Recovery**: Gradually increases traffic during recovery

### 3. Advanced Monitoring

- **Real-time Statistics**: Comprehensive success/failure/rejection metrics
- **Event Broadcasting**: Real-time circuit state change notifications
- **Performance Tracking**: Latency and reliability monitoring
- **Health Diagnostics**: Circuit breaker health and performance analysis

### 4. External Trigger Support

- **Performance Triggers**: Latency-based circuit opening
- **Quality Triggers**: Reliability score thresholds
- **Network Triggers**: Packet loss and connectivity issues
- **Custom Triggers**: Extensible trigger framework

## 🏗️ Architecture

### Circuit Breaker States

```text
                   ┌─────────────┐
                   │   CLOSED    │◄──────────────────┐
                   │  (Normal)   │                   │
                   └─────────────┘                   │
                          │                          │
                  Failure threshold                  │
                      exceeded                       │
                          │                          │
                          ▼                          │
                   ┌─────────────┐                   │
                   │    OPEN     │                   │
                   │ (Rejecting) │                   │
                   └─────────────┘                   │
                          │                          │
                    Recovery timeout                 │
                          │                          │
                          ▼                          │
                   ┌─────────────┐                   │
                   │ HALF-OPEN   │                   │
                   │ (Testing)   │───────────────────┘
                   └─────────────┘
                          │
                    Failure detected
                          │
                          ▼
                   ┌─────────────┐
                   │    OPEN     │
                   │ (Rejecting) │
                   └─────────────┘
```

### Integration with Transports

```rust
// Circuit breaker integration across all transports
use synapse::{
    transport::{Transport, EnhancedMdnsTransport, EnhancedTcpTransport},
    circuit_breaker::{CircuitBreaker, CircuitBreakerConfig},
};
```

## 🚀 Quick Start

### Basic Usage

```rust
use synapse::transport::EnhancedMdnsTransport;
use synapse::circuit_breaker::CircuitBreakerConfig;

#[tokio::main]
async fn main() -> Result<()> {
    // Create transport with automatic circuit breaker
    let transport = EnhancedMdnsTransport::new(
        "my-entity",
        8080,
        None, // Uses default circuit breaker config
    ).await?;
    
    // Circuit breaker automatically protects all operations
    let message = SecureMessage::new(/*...*/);
    let result = transport.send_message("target", &message).await;
    
    match result {
        Ok(_) => println!("Message sent successfully"),
        Err(e) => println!("Message failed: {}", e),
    }
    
    Ok(())
}
```

### Custom Circuit Breaker Configuration

```rust
use synapse::circuit_breaker::CircuitBreakerConfig;
use std::time::Duration;

let circuit_config = CircuitBreakerConfig {
    failure_threshold: 5,              // Open after 5 failures
    minimum_requests: 3,               // Need 3 requests before evaluation
    failure_window: Duration::from_secs(60), // 1-minute failure window
    recovery_timeout: Duration::from_secs(30), // 30-second recovery timeout
    half_open_max_calls: 2,            // Max 2 calls in half-open state
    success_threshold: 0.7,            // 70% success rate to close
};

let transport = EnhancedMdnsTransport::new(
    "my-entity",
    8080,
    Some(circuit_config),
).await?;
```

## 📊 Monitoring and Statistics

### Getting Circuit Breaker Statistics

```rust
// Get current circuit breaker statistics
let stats = transport.get_circuit_breaker().get_stats();

println!("Circuit Breaker Statistics:");
println!("  State: {:?}", stats.state);
println!("  Total requests: {}", stats.total_requests);
println!("  Successful requests: {}", stats.success_count);
println!("  Failed requests: {}", stats.failure_count);
println!("  Rejected requests: {}", stats.rejection_count);
println!("  Success rate: {:.2}%", stats.success_rate() * 100.0);
println!("  Last state change: {:?}", stats.last_state_change);
```

### Event Monitoring

```rust
// Subscribe to circuit breaker events
let circuit_breaker = transport.get_circuit_breaker();
let mut events = circuit_breaker.subscribe_events();

tokio::spawn(async move {
    while let Ok(event) = events.recv().await {
        match event {
            CircuitEvent::Opened { reason, failure_count, timestamp } => {
                println!("🔴 Circuit OPENED: {} (failures: {})", reason, failure_count);
            }
            CircuitEvent::HalfOpened { timestamp } => {
                println!("🟡 Circuit HALF-OPENED - testing recovery");
            }
            CircuitEvent::Closed { success_count, timestamp } => {
                println!("🟢 Circuit CLOSED - service recovered (successes: {})", success_count);
            }
            CircuitEvent::RequestRejected { timestamp } => {
                println!("❌ Request rejected - circuit is open");
            }
            CircuitEvent::ExternalTriggerActivated { trigger_type, details } => {
                println!("⚡ External trigger activated: {} - {}", trigger_type, details);
            }
        }
    }
});
```

## 🔧 Advanced Features

### External Triggers

```rust
use synapse::circuit_breaker::{ExternalTrigger, TriggerType};

// Set up latency-based trigger
let circuit_breaker = transport.get_circuit_breaker();
let latency_trigger = ExternalTrigger {
    trigger_type: TriggerType::Latency,
    threshold: 1000.0, // 1 second
    condition: "average_latency > threshold".to_string(),
};

circuit_breaker.add_external_trigger(latency_trigger).await?;

// Manual trigger activation
circuit_breaker.activate_external_trigger(
    TriggerType::Custom,
    "High error rate detected".to_string()
).await?;
```

### Shared Circuit Breakers

```rust
use std::sync::Arc;

// Create shared circuit breaker for multiple transports
let shared_breaker = Arc::new(CircuitBreaker::new(circuit_config));

// Use shared breaker across multiple transports
let result1 = transport1.send_message_with_breaker(
    "target1",
    &message,
    Some(shared_breaker.clone())
).await;

let result2 = transport2.send_message_with_breaker(
    "target2",
    &message,
    Some(shared_breaker.clone())
).await;
```

### Performance Monitoring Integration

```rust
// Get performance metrics
let performance_metrics = transport.get_performance_metrics().await?;

// Circuit breaker automatically responds to performance degradation
if performance_metrics.average_latency > Duration::from_millis(500) {
    // Automatic external trigger activation
    println!("High latency detected - circuit breaker may activate");
}
```

## 🎯 Multi-Transport Integration

### Coordinated Circuit Breakers

```rust
use synapse::transport::{EnhancedMdnsTransport, EnhancedTcpTransport};

// Create multiple transports with circuit breakers
let mdns_transport = EnhancedMdnsTransport::new("entity", 8080, None).await?;
let tcp_transport = EnhancedTcpTransport::new(8081).await?;

// Each transport has independent circuit breaker protection
let mdns_result = mdns_transport.send_message("target", &message).await;
let tcp_result = tcp_transport.send_message("192.168.1.100:8081", &message).await;

// Monitor all circuit breakers
let mdns_stats = mdns_transport.get_circuit_breaker().get_stats();
let tcp_stats = tcp_transport.get_circuit_breaker().get_stats();

println!("mDNS Circuit: {:?}", mdns_stats.state);
println!("TCP Circuit: {:?}", tcp_stats.state);
```

### Transport Selection Based on Circuit State

```rust
// Smart transport selection based on circuit breaker states
async fn send_with_fallback(
    message: &SecureMessage,
    target: &str,
    transports: &[&dyn Transport],
) -> Result<String> {
    for transport in transports {
        let circuit_stats = transport.get_circuit_breaker().get_stats();
        
        // Skip transports with open circuit breakers
        if circuit_stats.state == CircuitState::Open {
            continue;
        }
        
        // Try sending with this transport
        match transport.send_message(target, message).await {
            Ok(result) => return Ok(result),
            Err(_) => continue, // Try next transport
        }
    }
    
    Err("All transports failed or have open circuit breakers".into())
}
```

## 📈 Configuration Options

### Circuit Breaker Configuration

```rust
pub struct CircuitBreakerConfig {
    pub failure_threshold: usize,          // Number of failures to open circuit
    pub minimum_requests: usize,           // Minimum requests before evaluation
    pub failure_window: Duration,          // Time window for failure counting
    pub recovery_timeout: Duration,        // Time before attempting recovery
    pub half_open_max_calls: usize,       // Max calls in half-open state
    pub success_threshold: f64,           // Success rate to close circuit (0.0-1.0)
}

// Default configuration
impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            minimum_requests: 2,
            failure_window: Duration::from_secs(60),
            recovery_timeout: Duration::from_secs(60),
            half_open_max_calls: 1,
            success_threshold: 0.7,
        }
    }
}
```

### Environment-Specific Configurations

```rust
// Development configuration (more lenient)
let dev_config = CircuitBreakerConfig {
    failure_threshold: 10,
    minimum_requests: 5,
    failure_window: Duration::from_secs(120),
    recovery_timeout: Duration::from_secs(30),
    half_open_max_calls: 3,
    success_threshold: 0.5,
};

// Production configuration (strict)
let prod_config = CircuitBreakerConfig {
    failure_threshold: 3,
    minimum_requests: 2,
    failure_window: Duration::from_secs(30),
    recovery_timeout: Duration::from_secs(120),
    half_open_max_calls: 1,
    success_threshold: 0.9,
};
```

## 🛡️ Use Cases

### 1. Network Resilience

```rust
// Protect against network instability
let network_config = CircuitBreakerConfig {
    failure_threshold: 3,
    minimum_requests: 2,
    failure_window: Duration::from_secs(30),
    recovery_timeout: Duration::from_secs(60),
    half_open_max_calls: 1,
    success_threshold: 0.8,
};

let transport = EnhancedMdnsTransport::new("entity", 8080, Some(network_config)).await?;
```

### 2. Service Protection

```rust
// Protect downstream services from overload
let service_config = CircuitBreakerConfig {
    failure_threshold: 5,
    minimum_requests: 3,
    failure_window: Duration::from_secs(60),
    recovery_timeout: Duration::from_secs(300), // 5-minute recovery
    half_open_max_calls: 2,
    success_threshold: 0.9,
};
```

### 3. AI Agent Communication

```rust
// Protect AI agents from communication failures
async fn ai_communication_with_protection() -> Result<()> {
    let ai_transport = EnhancedMdnsTransport::new("ai-agent", 8080, None).await?;
    
    // Circuit breaker automatically protects AI communication
    let result = ai_transport.send_message("other-ai", &message).await;
    
    match result {
        Ok(_) => println!("AI communication successful"),
        Err(e) => {
            println!("AI communication failed: {}", e);
            // Circuit breaker prevents cascading failures
        }
    }
    
    Ok(())
}
```

## 📊 Metrics and Analytics

### Circuit Breaker Metrics

```rust
// Get comprehensive circuit breaker metrics
let metrics = transport.get_circuit_breaker_metrics().await?;

println!("Circuit Breaker Analytics:");
println!("  State duration: {:?}", metrics.current_state_duration);
println!("  Total state changes: {}", metrics.total_state_changes);
println!("  Time in closed state: {:.2}%", metrics.closed_state_percentage);
println!("  Time in open state: {:.2}%", metrics.open_state_percentage);
println!("  Time in half-open state: {:.2}%", metrics.half_open_state_percentage);
println!("  Average recovery time: {:?}", metrics.average_recovery_time);
```

### Performance Impact Analysis

```rust
// Analyze circuit breaker performance impact
let impact_analysis = transport.get_performance_impact_analysis().await?;

println!("Performance Impact:");
println!("  Requests saved from failures: {}", impact_analysis.requests_saved);
println!("  Response time improvement: {:?}", impact_analysis.response_time_improvement);
println!("  Resource usage reduction: {:.2}%", impact_analysis.resource_usage_reduction);
```

## 🔗 Integration with Router

### Automatic Circuit Breaker Integration

```rust
// Router automatically uses circuit breaker protection
let router = EnhancedSynapseRouter::new(config, entity_id).await?;

// All router operations are protected by circuit breakers
router.send_message_smart(
    "target",
    "Hello!",
    MessageType::Direct,
    SecurityLevel::Authenticated,
    MessageUrgency::Interactive,
).await?;
// Circuit breaker protection is automatic
```

### Router-Level Circuit Breaker Configuration

```rust
// Configure circuit breakers at router level
let router_config = EnhancedRouterConfig {
    circuit_breaker_config: Some(CircuitBreakerConfig {
        failure_threshold: 3,
        minimum_requests: 2,
        failure_window: Duration::from_secs(60),
        recovery_timeout: Duration::from_secs(120),
        half_open_max_calls: 1,
        success_threshold: 0.8,
    }),
    // ... other config options
};

let router = EnhancedSynapseRouter::new_with_config(router_config, entity_id).await?;
```

## 🎯 Best Practices

### 1. Configuration Guidelines

```rust
// Choose appropriate thresholds based on your use case
let config = CircuitBreakerConfig {
    // Critical services: lower threshold
    failure_threshold: if is_critical_service { 2 } else { 5 },
    
    // Network-dependent services: longer recovery time
    recovery_timeout: if is_network_dependent { 
        Duration::from_secs(300) 
    } else { 
        Duration::from_secs(60) 
    },
    
    // High-availability services: higher success threshold
    success_threshold: if is_high_availability { 0.9 } else { 0.7 },
    
    // Other settings...
    ..Default::default()
};
```

### 2. Monitoring Strategy

```rust
// Set up comprehensive monitoring
let circuit_breaker = transport.get_circuit_breaker();

// Monitor key metrics
let mut metrics_interval = tokio::time::interval(Duration::from_secs(60));
tokio::spawn(async move {
    loop {
        metrics_interval.tick().await;
        let stats = circuit_breaker.get_stats();
        
        // Log metrics to monitoring system
        log_metrics("circuit_breaker.success_rate", stats.success_rate());
        log_metrics("circuit_breaker.failure_count", stats.failure_count as f64);
        log_metrics("circuit_breaker.rejection_count", stats.rejection_count as f64);
    }
});
```

### 3. Error Handling

```rust
// Proper error handling with circuit breakers
async fn send_with_circuit_breaker(
    transport: &dyn Transport,
    target: &str,
    message: &SecureMessage,
) -> Result<String> {
    match transport.send_message(target, message).await {
        Ok(result) => Ok(result),
        Err(e) => {
            // Check if error is due to circuit breaker
            if e.to_string().contains("circuit breaker") {
                // Handle circuit breaker rejection
                println!("Circuit breaker is open - service temporarily unavailable");
                // Implement fallback strategy
                return fallback_send_strategy(target, message).await;
            }
            Err(e)
        }
    }
}
```

## 📚 API Reference

### Core Types

```rust
// Circuit breaker state
#[derive(Debug, Clone, PartialEq)]
pub enum CircuitState {
    Closed,    // Normal operation
    Open,      // Rejecting requests
    HalfOpen,  // Testing recovery
}

// Circuit breaker statistics
pub struct CircuitStats {
    pub state: CircuitState,
    pub total_requests: u64,
    pub success_count: u64,
    pub failure_count: u64,
    pub rejection_count: u64,
    pub last_state_change: Option<Instant>,
    pub current_state_duration: Duration,
}

// Circuit breaker events
pub enum CircuitEvent {
    Opened { reason: String, failure_count: u64, timestamp: Instant },
    HalfOpened { timestamp: Instant },
    Closed { success_count: u64, timestamp: Instant },
    RequestRejected { timestamp: Instant },
    ExternalTriggerActivated { trigger_type: TriggerType, details: String },
}
```

### Main API

```rust
impl CircuitBreaker {
    // Creation and configuration
    pub fn new(config: CircuitBreakerConfig) -> Self;
    
    // Request handling
    pub async fn call<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce() -> Result<R>;
    
    // Monitoring
    pub fn get_stats(&self) -> CircuitStats;
    pub fn subscribe_events(&self) -> broadcast::Receiver<CircuitEvent>;
    
    // External triggers
    pub async fn add_external_trigger(&self, trigger: ExternalTrigger) -> Result<()>;
    pub async fn activate_external_trigger(&self, trigger_type: TriggerType, details: String) -> Result<()>;
    
    // Manual control
    pub async fn force_open(&self, reason: String) -> Result<()>;
    pub async fn force_close(&self) -> Result<()>;
    pub async fn reset(&self) -> Result<()>;
}
```

## 🎉 Conclusion

The Synapse Circuit Breaker System provides comprehensive protection against failures across all transport layers. Key benefits include:

- **Automatic Protection**: No manual configuration required for basic protection
- **Intelligent Recovery**: Evidence-based recovery decisions
- **Comprehensive Monitoring**: Real-time statistics and event notifications
- **Transport Integration**: Seamless integration with all Synapse transports
- **Extensible Design**: Support for custom triggers and configurations

The circuit breaker system is essential for building resilient, fault-tolerant communication networks that can handle real-world network conditions and service failures.

For more information, see the [Circuit Breaker API Documentation](../api/circuit_breaker.md) and [Multi-Transport Circuit Breaker Examples](../examples/multi_transport_circuit_breaker_demo.rs).
