# 🔧 Identity Resolution Troubleshooting Guide

> Debugging and solving common issues with unknown name resolution in EMRP

## 🚨 Common Problems and Solutions

### Problem 1: "Cannot find obvious contacts"

**Symptoms:**
- Simple names like "Alice" return `NotFound`
- Colleagues in the same organization aren't discoverable
- Local network peers aren't being found

**Diagnostic Steps:**

```rust
// Check if basic discovery is working
async fn diagnose_basic_discovery(router: &EnhancedSynapseRouter) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Test local name resolution first
    match router.resolve_local_name("Alice") {
        Some(global_id) => {
            println!("✅ Local name 'Alice' resolves to: {}", global_id);
            
            // Test if the global ID is reachable
            match router.test_connectivity(&global_id).await {
                Ok(true) => println!("✅ Connectivity to {} is working", global_id),
                Ok(false) => println!("❌ Cannot reach {} - network issue", global_id),
                Err(e) => println!("❌ Connectivity test failed: {}", e),
            }
        }
        None => {
            println!("❌ 'Alice' not found in local registry");
            println!("💡 Try registering Alice first:");
            println!("   router.register_peer(\"Alice\", \"alice@company.com\").await?;");
        }
    }
    
    // 2. Test discovery configuration
    let discovery_status = router.get_discovery_status().await?;
    println!("Discovery enabled: {}", discovery_status.enabled);
    println!("Available methods: {:?}", discovery_status.enabled_methods);
    
    if !discovery_status.enabled {
        println!("❌ Discovery is disabled");
        println!("💡 Enable discovery with:");
        println!("   router.configure_discovery(discovery_config).await?;");
    }
    
    // 3. Test mDNS for local network
    if discovery_status.enabled_methods.contains(&"mDNS".to_string()) {
        let local_peers = router.discover_local_peers().await?;
        println!("Found {} local peers via mDNS", local_peers.len());
        for peer in local_peers {
            println!("  - {} ({})", peer.name, peer.address);
        }
    }
    
    Ok(())
}
```

**Solutions:**

1. **Enable discovery methods:**
```rust
let discovery_config = DiscoveryConfig {
    allow_being_discovered: true,
    enabled_methods: vec![
        DiscoveryMethod::DnsDiscovery { 
            patterns: vec!["${name}@company.com".to_string()] 
        },
        DiscoveryMethod::PeerNetworkQuery { 
            ask_known_peers: true, 
            propagation_limit: 2 
        },
    ],
    // ... other config
};
router.configure_discovery(discovery_config).await?;
```

2. **Check network connectivity:**
```bash
# Test if mDNS is working
cargo run --example connectivity_demo

# Check if DNS resolution works
nslookup alice.company.com
```

3. **Verify firewall settings:**
- TCP port 8080 (default EMRP port)
- UDP port 5353 (mDNS)
- Organization-specific ports

### Problem 2: "Too many false positive matches"

**Symptoms:**
- Generic names return too many candidates
- Low confidence scores for obvious matches
- Wrong people are being contacted

**Diagnostic Steps:**

```rust
async fn diagnose_false_positives(router: &EnhancedSynapseRouter) -> Result<(), Box<dyn std::error::Error>> {
    let lookup = ContactLookupRequest {
        name: "John".to_string(),
        hints: vec![
            ContactHint::Organization("Engineering".to_string()),
        ],
        requester_context: RequesterContext {
            from_entity: "test@company.com".to_string(),
            purpose: "Testing".to_string(),
            urgency: MessageUrgency::Interactive,
        },
    };
    
    match router.resolve_contact_with_context(lookup).await? {
        ResolutionResult::ContactRequestRequired(candidates) => {
            println!("Found {} candidates for 'John':", candidates.len());
            
            for (i, candidate) in candidates.iter().enumerate() {
                println!("{}. {} (confidence: {:.2}, method: {})", 
                    i + 1,
                    candidate.global_id,
                    candidate.confidence,
                    candidate.discovery_method
                );
                
                // Check metadata for disambiguation
                if let Some(role) = candidate.metadata.get("role") {
                    println!("   Role: {}", role);
                }
                if let Some(org) = candidate.metadata.get("organization") {
                    println!("   Organization: {}", org);
                }
            }
            
            // Analyze confidence distribution
            let high_confidence = candidates.iter().filter(|c| c.confidence > 0.8).count();
            let medium_confidence = candidates.iter().filter(|c| c.confidence > 0.5 && c.confidence <= 0.8).count();
            let low_confidence = candidates.iter().filter(|c| c.confidence <= 0.5).count();
            
            println!("\nConfidence distribution:");
            println!("  High (>0.8): {}", high_confidence);
            println!("  Medium (0.5-0.8): {}", medium_confidence);
            println!("  Low (≤0.5): {}", low_confidence);
            
            if low_confidence > high_confidence {
                println!("⚠️  Too many low-confidence matches. Consider:");
                println!("   - Adding more specific hints");
                println!("   - Increasing confidence threshold");
                println!("   - Using more restrictive discovery methods");
            }
        }
        _ => {}
    }
    
    Ok(())
}
```

**Solutions:**

1. **Add more specific hints:**
```rust
let lookup = ContactLookupRequest {
    name: "John".to_string(),
    hints: vec![
        ContactHint::Organization("Engineering Team".to_string()),
        ContactHint::Role("Senior Software Engineer".to_string()),
        ContactHint::Domain("company.com".to_string()),
        ContactHint::Expertise("Backend Development".to_string()),
    ],
    // ...
};
```

2. **Adjust confidence thresholds:**
```rust
// In your matching logic
let high_confidence_threshold = 0.85;
let candidates = discovery_results.into_iter()
    .filter(|c| c.confidence >= high_confidence_threshold)
    .collect();
```

3. **Use more restrictive discovery methods:**
```rust
// Focus on internal directory first
DiscoveryMethod::DirectoryLookup {
    servers: vec!["ldap://company.com".to_string()],
    search_patterns: vec!["(&(cn={name})(department=Engineering))".to_string()],
}
```

### Problem 3: "Contact requests are being ignored"

**Symptoms:**
- Contact requests sent but never approved/declined
- Auto-approval not working as expected
- Recipients not receiving requests

**Diagnostic Steps:**

```rust
async fn diagnose_contact_requests(
    router: &EnhancedSynapseRouter,
    request_id: &str
) -> Result<(), Box<dyn std::error::Error>> {
    // Check request status
    match router.get_contact_request_status(request_id).await? {
        RequestStatus::Pending => {
            println!("Request {} is still pending", request_id);
            
            // Check if it's been delivered
            let delivery_status = router.get_request_delivery_status(request_id).await?;
            match delivery_status {
                DeliveryStatus::Delivered => {
                    println!("✅ Request was delivered successfully");
                    println!("💡 Recipient may need time to respond");
                }
                DeliveryStatus::Failed(reason) => {
                    println!("❌ Request delivery failed: {}", reason);
                    println!("💡 Check network connectivity to recipient");
                }
                DeliveryStatus::InTransit => {
                    println!("🚀 Request is being delivered...");
                }
            }
        }
        RequestStatus::Approved(permissions) => {
            println!("✅ Request {} was approved with permissions: {:?}", request_id, permissions);
        }
        RequestStatus::Declined(reason) => {
            println!("❌ Request {} was declined: {:?}", request_id, reason);
        }
        RequestStatus::Expired => {
            println!("⏰ Request {} expired before being answered", request_id);
        }
    }
    
    // Check auto-approval rules
    let auto_rules = router.get_auto_approval_rules().await?;
    println!("Active auto-approval rules: {}", auto_rules.len());
    for rule in auto_rules {
        println!("  - {:?} -> {:?} (priority: {})", rule.condition, rule.action, rule.priority);
    }
    
    Ok(())
}
```

**Solutions:**

1. **Check auto-approval configuration:**
```rust
// Verify auto-approval rules are set up correctly
let rules = vec![
    AutoApprovalRule {
        condition: ApprovalCondition::FromDomain("company.com".to_string()),
        action: ApprovalAction::AutoApprove(vec![Permission::SingleMessage]),
        priority: 1,
    },
];

// Make sure they're applied
router.update_auto_approval_rules(rules).await?;
```

2. **Verify delivery method:**
```rust
// Check if recipient is reachable
let target = "recipient@company.com";
match router.test_connectivity(target).await? {
    true => println!("✅ Can reach {}", target),
    false => {
        println!("❌ Cannot reach {}", target);
        println!("💡 Try using email fallback:");
        router.send_contact_request_via_email(target, "...").await?;
    }
}
```

3. **Use fallback methods:**
```rust
// If direct contact fails, try email
if router.send_contact_request(&target, message, permissions).await.is_err() {
    println!("Direct contact failed, trying email fallback...");
    router.send_contact_request_via_email(&target.global_id, message).await?;
}
```

### Problem 4: "Discovery is too slow"

**Symptoms:**
- Long delays before returning results
- Timeouts during discovery
- High resource usage

**Diagnostic Steps:**

```rust
async fn diagnose_performance(router: &EnhancedSynapseRouter) -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Instant;
    
    let start = Instant::now();
    
    let lookup = ContactLookupRequest {
        name: "TestUser".to_string(),
        hints: vec![ContactHint::Domain("company.com".to_string())],
        requester_context: RequesterContext {
            from_entity: "test@company.com".to_string(),
            purpose: "Performance test".to_string(),
            urgency: MessageUrgency::Interactive,
        },
    };
    
    // Test each discovery method individually
    let methods = vec![
        ("DNS Discovery", DiscoveryMethod::DnsDiscovery { 
            patterns: vec!["testuser@company.com".to_string()] 
        }),
        ("Peer Network", DiscoveryMethod::PeerNetworkQuery { 
            ask_known_peers: true, 
            propagation_limit: 2 
        }),
    ];
    
    for (name, method) in methods {
        let method_start = Instant::now();
        
        let results = router.test_discovery_method(&method, &lookup).await?;
        
        let duration = method_start.elapsed();
        println!("{}: {} results in {:?}", name, results.len(), duration);
        
        if duration > std::time::Duration::from_secs(5) {
            println!("⚠️  {} is slow (>{:?})", name, duration);
        }
    }
    
    let total_duration = start.elapsed();
    println!("Total discovery time: {:?}", total_duration);
    
    Ok(())
}
```

**Solutions:**

1. **Configure timeouts:**
```rust
let discovery_config = DiscoveryConfig {
    // Set reasonable timeouts
    method_timeout: Duration::from_secs(3),
    total_timeout: Duration::from_secs(10),
    
    // Limit concurrent operations
    max_concurrent_discoveries: 5,
    
    // Cache results
    cache_ttl: Duration::from_minutes(15),
    // ...
};
```

2. **Optimize discovery methods:**
```rust
// Use faster methods first
let optimized_methods = vec![
    // Fast: Local registry lookup
    DiscoveryMethod::LocalRegistry,
    
    // Medium: DNS queries
    DiscoveryMethod::DnsDiscovery { 
        patterns: vec!["${name}@${domain}".to_string()] 
    },
    
    // Slow: Peer network queries (use sparingly)
    DiscoveryMethod::PeerNetworkQuery { 
        ask_known_peers: true, 
        propagation_limit: 1  // Reduced from 2
    },
];
```

3. **Enable caching:**
```rust
// Configure result caching
router.configure_discovery_cache(DiscoveryCacheConfig {
    max_entries: 1000,
    ttl: Duration::from_minutes(30),
    negative_cache_ttl: Duration::from_minutes(5), // Cache "not found" results
}).await?;
```

## 🔍 Debug Tools and Utilities

### Debug Mode Discovery

```rust
async fn debug_discovery_process(router: &EnhancedSynapseRouter, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Enable debug logging
    router.set_debug_mode(true).await?;
    
    let lookup = ContactLookupRequest {
        name: name.to_string(),
        hints: vec![ContactHint::Domain("company.com".to_string())],
        requester_context: RequesterContext {
            from_entity: "debug@company.com".to_string(),
            purpose: "Debug test".to_string(),
            urgency: MessageUrgency::Interactive,
        },
    };
    
    // Get detailed discovery trace
    let trace = router.resolve_contact_with_trace(lookup).await?;
    
    println!("Discovery trace for '{}':", name);
    for step in trace.steps {
        println!("  [{}] {}: {} ({}ms)", 
            step.timestamp.format("%H:%M:%S%.3f"),
            step.method,
            step.result,
            step.duration_ms
        );
        
        if let Some(error) = step.error {
            println!("    Error: {}", error);
        }
        
        if !step.candidates.is_empty() {
            println!("    Candidates found: {}", step.candidates.len());
            for candidate in step.candidates {
                println!("      - {} (confidence: {:.2})", candidate.global_id, candidate.confidence);
            }
        }
    }
    
    println!("Final result: {:?}", trace.final_result);
    println!("Total time: {}ms", trace.total_duration_ms);
    
    Ok(())
}
```

### Network Connectivity Tester

```rust
async fn test_network_connectivity(router: &EnhancedSynapseRouter) -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing network connectivity...");
    
    // Test basic network access
    match router.test_internet_connectivity().await {
        Ok(true) => println!("✅ Internet connectivity: OK"),
        Ok(false) => println!("❌ Internet connectivity: FAILED"),
        Err(e) => println!("❌ Internet connectivity test error: {}", e),
    }
    
    // Test DNS resolution
    let test_domains = vec!["company.com", "google.com", "github.com"];
    for domain in test_domains {
        match router.test_dns_resolution(domain).await {
            Ok(ips) => println!("✅ DNS {}: {:?}", domain, ips),
            Err(e) => println!("❌ DNS {}: {}", domain, e),
        }
    }
    
    // Test EMRP ports
    let test_ports = vec![8080, 8081, 5353];
    for port in test_ports {
        match router.test_port_accessibility(port).await {
            Ok(true) => println!("✅ Port {}: Open", port),
            Ok(false) => println!("❌ Port {}: Blocked", port),
            Err(e) => println!("❌ Port {} test error: {}", port, e),
        }
    }
    
    // Test known peers
    let known_peers = router.get_known_peers().await?;
    println!("Testing connectivity to {} known peers...", known_peers.len());
    
    for peer in known_peers.iter().take(5) {  // Test first 5
        match router.test_peer_connectivity(&peer.global_id).await {
            Ok(true) => println!("✅ Peer {}: Reachable", peer.global_id),
            Ok(false) => println!("❌ Peer {}: Unreachable", peer.global_id),
            Err(e) => println!("❌ Peer {} error: {}", peer.global_id, e),
        }
    }
    
    Ok(())
}
```

### Discovery Method Benchmark

```rust
async fn benchmark_discovery_methods(router: &EnhancedSynapseRouter) -> Result<(), Box<dyn std::error::Error>> {
    let test_cases = vec![
        ("Known contact", "Alice", vec![ContactHint::Domain("company.com".to_string())]),
        ("Unknown contact", "RandomPerson", vec![ContactHint::Domain("company.com".to_string())]),
        ("Common name", "John", vec![ContactHint::Organization("Engineering".to_string())]),
    ];
    
    let methods = vec![
        ("DNS Discovery", DiscoveryMethod::DnsDiscovery { 
            patterns: vec!["${name}@company.com".to_string()] 
        }),
        ("Peer Network", DiscoveryMethod::PeerNetworkQuery { 
            ask_known_peers: true, 
            propagation_limit: 1 
        }),
    ];
    
    println!("Discovery Method Benchmark");
    println!("==========================");
    
    for (case_name, name, hints) in test_cases {
        println!("\nTest case: {} ('{}')", case_name, name);
        
        for (method_name, method) in &methods {
            let lookup = ContactLookupRequest {
                name: name.to_string(),
                hints: hints.clone(),
                requester_context: RequesterContext {
                    from_entity: "benchmark@company.com".to_string(),
                    purpose: "Benchmark test".to_string(),
                    urgency: MessageUrgency::Interactive,
                },
            };
            
            let start = std::time::Instant::now();
            let results = router.test_discovery_method(method, &lookup).await?;
            let duration = start.elapsed();
            
            println!("  {}: {} results in {:?}", method_name, results.len(), duration);
            
            // Show top result if any
            if let Some(top_result) = results.first() {
                println!("    Best: {} (confidence: {:.2})", top_result.global_id, top_result.confidence);
            }
        }
    }
    
    Ok(())
}
```

## 📊 Monitoring and Metrics

### Discovery Metrics Collection

```rust
pub struct DiscoveryMetrics {
    pub total_requests: u64,
    pub successful_resolutions: u64,
    pub failed_resolutions: u64,
    pub average_resolution_time: Duration,
    pub method_performance: HashMap<String, MethodMetrics>,
}

pub struct MethodMetrics {
    pub requests: u64,
    pub successes: u64,
    pub average_time: Duration,
    pub error_rate: f64,
}

impl EnhancedSynapseRouter {
    pub async fn get_discovery_metrics(&self) -> Result<DiscoveryMetrics, Box<dyn std::error::Error>> {
        // Implementation would gather metrics from internal counters
        todo!()
    }
    
    pub async fn reset_discovery_metrics(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Reset all counters
        todo!()
    }
}
```

This troubleshooting guide provides comprehensive tools and techniques for diagnosing and fixing identity resolution issues in EMRP systems.
