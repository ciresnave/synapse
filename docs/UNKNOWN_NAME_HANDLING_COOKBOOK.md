# 👥 Unknown Name Handling Cookbook

> Practical recipes for handling unknown contacts in EMRP

## 🎯 Quick Start: The Most Common Scenarios

### Scenario 1: "I want to contact Alice from the AI team"

**The Problem**: You know someone exists but don't have their exact contact details.

**The Solution**: Use contextual hints to guide discovery.

```rust
use message_routing_system::discovery::*;

async fn contact_alice_from_ai_team(router: &EnhancedEmrpRouter) -> Result<(), Box<dyn std::error::Error>> {
    let lookup = ContactLookupRequest {
        name: "Alice".to_string(),
        hints: vec![
            ContactHint::Organization("AI Team".to_string()),
            ContactHint::Role("AI Engineer".to_string()),
            // If you know the company domain
            ContactHint::Domain("company.com".to_string()),
        ],
        requester_context: RequesterContext {
            from_entity: "your-bot@company.com".to_string(),
            purpose: "Project coordination discussion".to_string(),
            urgency: MessageUrgency::Interactive,
        },
    };
    
    match router.resolve_contact_with_context(lookup).await? {
        ResolutionResult::Direct(global_id) => {
            // Found her! Send message directly
            router.send_message_smart(
                &global_id,
                "Hi Alice! I'm working on the ML project and wanted to coordinate with the AI team.",
                MessageType::Direct,
                SecurityLevel::Authenticated,
                MessageUrgency::Interactive
            ).await?;
            println!("✅ Message sent to Alice directly");
        }
        
        ResolutionResult::ContactRequestRequired(candidates) => {
            // Found potential matches, need permission
            println!("Found {} potential matches for Alice", candidates.len());
            
            for (i, candidate) in candidates.iter().enumerate() {
                println!("  {}. {} (confidence: {:.1}%)", 
                    i + 1, 
                    candidate.global_id, 
                    candidate.confidence * 100.0
                );
                
                // Send contact request to most likely candidate
                if i == 0 && candidate.confidence > 0.7 {
                    let request_id = router.send_contact_request(
                        candidate,
                        "Hi! I'm looking for Alice from the AI team to discuss project coordination.",
                        vec![Permission::Conversation(Duration::hours(24))]
                    ).await?;
                    
                    println!("📬 Contact request sent: {}", request_id);
                }
            }
        }
        
        ResolutionResult::Suggestions(similar) => {
            println!("Did you mean one of these existing contacts?");
            for suggestion in similar {
                println!("  - {} ({})", suggestion.local_name, suggestion.global_id);
            }
        }
        
        ResolutionResult::NotFound => {
            println!("❌ Could not find Alice from the AI team");
            println!("💡 Try asking someone from the team to introduce you, or use a more specific search");
        }
    }
    
    Ok(())
}
```

### Scenario 2: "Find the deployment bot for the web team"

**The Problem**: You need to contact an automated system but don't know its exact name.

**The Solution**: Use entity type and purpose hints.

```rust
async fn find_deployment_bot(router: &EnhancedEmrpRouter) -> Result<(), Box<dyn std::error::Error>> {
    let lookup = ContactLookupRequest {
        name: "deployment bot".to_string(),
        hints: vec![
            ContactHint::EntityType(EntityType::AutomatedSystem),
            ContactHint::Purpose("deployment".to_string()),
            ContactHint::Organization("web team".to_string()),
            ContactHint::Capability("deployment".to_string()),
        ],
        requester_context: RequesterContext {
            from_entity: "developer@company.com".to_string(),
            purpose: "Trigger production deployment".to_string(),
            urgency: MessageUrgency::High,
        },
    };
    
    match router.resolve_contact_with_context(lookup).await? {
        ResolutionResult::Direct(global_id) => {
            // Send deployment command
            let deployment_request = json!({
                "action": "deploy",
                "environment": "production",
                "branch": "main",
                "requestor": "developer@company.com"
            });
            
            router.send_message_smart(
                &global_id,
                &deployment_request.to_string(),
                MessageType::Direct,
                SecurityLevel::Authenticated,
                MessageUrgency::High
            ).await?;
            
            println!("✅ Deployment request sent to {}", global_id);
        }
        
        ResolutionResult::ContactRequestRequired(candidates) => {
            // Automated systems typically have auto-approval for work requests
            for candidate in candidates {
                if candidate.global_id.contains("deploy") || candidate.global_id.contains("bot") {
                    let request_id = router.send_contact_request(
                        &candidate,
                        "Need to trigger a production deployment",
                        vec![Permission::SingleMessage]
                    ).await?;
                    
                    println!("📬 Auto-request sent to: {}", candidate.global_id);
                }
            }
        }
        
        _ => {
            println!("❌ Could not find deployment bot");
            println!("💡 Try contacting the web team directly or check internal documentation");
        }
    }
    
    Ok(())
}
```

### Scenario 3: "Contact Dr. Smith who works on machine learning"

**The Problem**: Common name with professional context - need to disambiguate.

**The Solution**: Use professional hints and fuzzy matching.

```rust
async fn contact_dr_smith_ml(router: &EnhancedEmrpRouter) -> Result<(), Box<dyn std::error::Error>> {
    let lookup = ContactLookupRequest {
        name: "Dr. Smith".to_string(),
        hints: vec![
            ContactHint::Role("Professor".to_string()),
            ContactHint::Expertise("Machine Learning".to_string()),
            ContactHint::EntityType(EntityType::Human),
            ContactHint::Title("Dr.".to_string()),
            // Try common academic domains
            ContactHint::Domain("university.edu".to_string()),
        ],
        requester_context: RequesterContext {
            from_entity: "student@university.edu".to_string(),
            purpose: "Research collaboration on ML project".to_string(),
            urgency: MessageUrgency::Background,
        },
    };
    
    match router.resolve_contact_with_context(lookup).await? {
        ResolutionResult::Direct(global_id) => {
            router.send_message_smart(
                &global_id,
                "Dear Dr. Smith, I'm a graduate student working on machine learning and would like to discuss potential research collaboration.",
                MessageType::Direct,
                SecurityLevel::Authenticated,
                MessageUrgency::Background
            ).await?;
        }
        
        ResolutionResult::ContactRequestRequired(candidates) => {
            // Multiple Dr. Smiths found - be more specific
            println!("Found {} researchers named Dr. Smith:", candidates.len());
            
            for candidate in candidates {
                println!("  - {} ({})", candidate.global_id, candidate.discovery_method);
                
                // Look for ML-related metadata
                if candidate.metadata.get("expertise").map_or(false, |exp| 
                    exp.to_lowercase().contains("machine learning") ||
                    exp.to_lowercase().contains("artificial intelligence")
                ) {
                    let request_id = router.send_contact_request(
                        &candidate,
                        "Dear Dr. Smith, I'm interested in your machine learning research. Could we discuss potential collaboration?",
                        vec![Permission::SingleMessage]
                    ).await?;
                    
                    println!("📬 Research inquiry sent to: {}", candidate.global_id);
                }
            }
        }
        
        ResolutionResult::Suggestions(similar) => {
            println!("Found similar existing contacts. Did you mean:");
            for suggestion in similar {
                println!("  - {} ({})", suggestion.local_name, suggestion.global_id);
            }
            
            // Offer to search with different hints
            println!("\n💡 Try being more specific:");
            println!("  - Include the university name");
            println!("  - Specify the research area (computer vision, NLP, etc.)");
            println!("  - Use first name if known");
        }
        
        ResolutionResult::NotFound => {
            println!("❌ Could not find Dr. Smith with ML expertise");
            println!("💡 Suggestions:");
            println!("  - Try searching academic directories");
            println!("  - Contact the CS department directly");
            println!("  - Use a different search strategy");
        }
    }
    
    Ok(())
}
```

## 🔧 Advanced Patterns

### Pattern 1: Multi-Stage Discovery with Fallbacks

When your first attempt doesn't work, try increasingly broad searches:

```rust
async fn progressive_search(
    router: &EnhancedEmrpRouter,
    name: &str,
    initial_hints: Vec<ContactHint>
) -> Result<ResolutionResult, Box<dyn std::error::Error>> {
    
    // Stage 1: Exact search with all hints
    let mut lookup = ContactLookupRequest {
        name: name.to_string(),
        hints: initial_hints.clone(),
        requester_context: RequesterContext {
            from_entity: "searcher@company.com".to_string(),
            purpose: "Contact attempt".to_string(),
            urgency: MessageUrgency::Interactive,
        },
    };
    
    if let result @ (ResolutionResult::Direct(_) | ResolutionResult::ContactRequestRequired(_)) = 
        router.resolve_contact_with_context(lookup.clone()).await? {
        return Ok(result);
    }
    
    // Stage 2: Broaden search - remove specific hints
    lookup.hints = initial_hints.into_iter()
        .filter(|hint| matches!(hint, 
            ContactHint::Organization(_) | 
            ContactHint::Domain(_) |
            ContactHint::EntityType(_)
        ))
        .collect();
    
    if let result @ (ResolutionResult::Direct(_) | ResolutionResult::ContactRequestRequired(_)) = 
        router.resolve_contact_with_context(lookup.clone()).await? {
        return Ok(result);
    }
    
    // Stage 3: Fuzzy name search only
    lookup.hints = vec![];
    let result = router.resolve_contact_with_context(lookup).await?;
    
    Ok(result)
}
```

### Pattern 2: Batch Contact Discovery

When you need to find multiple people at once:

```rust
async fn find_team_members(
    router: &EnhancedEmrpRouter,
    team_name: &str,
    member_names: Vec<&str>
) -> Result<HashMap<String, ResolutionResult>, Box<dyn std::error::Error>> {
    
    let mut results = HashMap::new();
    
    // Create lookup requests for each member
    let lookups: Vec<_> = member_names.into_iter().map(|name| {
        ContactLookupRequest {
            name: name.to_string(),
            hints: vec![
                ContactHint::Organization(team_name.to_string()),
                ContactHint::Domain("company.com".to_string()),
            ],
            requester_context: RequesterContext {
                from_entity: "coordinator@company.com".to_string(),
                purpose: format!("Team coordination for {}", team_name),
                urgency: MessageUrgency::Interactive,
            },
        }
    }).collect();
    
    // Process lookups concurrently
    let futures = lookups.into_iter().map(|lookup| {
        let name = lookup.name.clone();
        let router = router.clone();
        async move {
            let result = router.resolve_contact_with_context(lookup).await?;
            Ok::<(String, ResolutionResult), Box<dyn std::error::Error>>((name, result))
        }
    });
    
    let lookup_results = futures::future::join_all(futures).await;
    
    for result in lookup_results {
        match result {
            Ok((name, resolution)) => {
                results.insert(name, resolution);
            }
            Err(e) => {
                eprintln!("Failed to lookup {}: {}", name, e);
            }
        }
    }
    
    Ok(results)
}
```

### Pattern 3: Smart Contact Request Approval

Configure intelligent auto-approval based on context:

```rust
async fn setup_smart_approval_rules(router: &EnhancedEmrpRouter) -> Result<(), Box<dyn std::error::Error>> {
    let discovery_config = DiscoveryConfig {
        allow_being_discovered: true,
        discovery_permissions: DiscoveryPermissions {
            // Allow discovery by company domain and partners
            discoverable_by_domain: vec![
                "company.com".to_string(),
                "partner1.com".to_string(),
                "partner2.com".to_string(),
            ],
            discoverable_by_entity_type: vec![
                EntityType::Human,
                EntityType::AiModel,
                EntityType::AutomatedSystem,
            ],
            require_introduction: false,
            public_profile_info: ProfileInfo {
                name: "Project Coordinator".to_string(),
                role: Some("AI Development".to_string()),
                organization: Some("Tech Company AI Division".to_string()),
                bio: Some("Coordinating AI projects and research collaborations".to_string()),
            },
        },
        auto_approval_rules: vec![
            // Auto-approve colleagues
            AutoApprovalRule {
                condition: ApprovalCondition::FromDomain("company.com".to_string()),
                action: ApprovalAction::AutoApprove(vec![
                    Permission::Conversation(Duration::days(30))
                ]),
                priority: 1,
            },
            
            // Auto-approve research requests from universities
            AutoApprovalRule {
                condition: ApprovalCondition::And(vec![
                    ApprovalCondition::FromDomain("*.edu".to_string()),
                    ApprovalCondition::WithPurpose("research".to_string()),
                ]),
                action: ApprovalAction::AutoApprove(vec![
                    Permission::SingleMessage
                ]),
                priority: 2,
            },
            
            // Auto-approve emergency contacts
            AutoApprovalRule {
                condition: ApprovalCondition::WithUrgency(MessageUrgency::Emergency),
                action: ApprovalAction::AutoApprove(vec![
                    Permission::EmergencyContact
                ]),
                priority: 0, // Highest priority
            },
            
            // Require manual approval for marketing/sales
            AutoApprovalRule {
                condition: ApprovalCondition::WithPurpose("marketing".to_string()),
                action: ApprovalAction::RequireManualApproval,
                priority: 3,
            },
        ],
        max_discovery_requests_per_hour: 50,
        max_pending_requests: 20,
    };
    
    router.configure_discovery(discovery_config).await?;
    Ok(())
}
```

## 🚨 Error Handling and Edge Cases

### Handling Ambiguous Results

```rust
async fn handle_ambiguous_results(
    candidates: Vec<DiscoveredIdentity>
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    
    if candidates.is_empty() {
        return Ok(None);
    }
    
    // Sort by confidence
    let mut sorted_candidates = candidates;
    sorted_candidates.sort_by(|a, b| 
        b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal)
    );
    
    let top_candidate = &sorted_candidates[0];
    
    // If confidence is very high, use it
    if top_candidate.confidence > 0.9 {
        return Ok(Some(top_candidate.global_id.clone()));
    }
    
    // If there's a clear winner (much higher confidence than others)
    if sorted_candidates.len() > 1 {
        let second_best = &sorted_candidates[1];
        if top_candidate.confidence - second_best.confidence > 0.3 {
            return Ok(Some(top_candidate.global_id.clone()));
        }
    }
    
    // Too ambiguous - ask for clarification
    println!("Multiple matches found. Please clarify:");
    for (i, candidate) in sorted_candidates.iter().take(5).enumerate() {
        println!("  {}. {} (confidence: {:.1}%, from: {})", 
            i + 1,
            candidate.global_id,
            candidate.confidence * 100.0,
            candidate.discovery_method
        );
    }
    
    Ok(None)
}
```

### Rate Limiting and Backoff

```rust
use tokio::time::{sleep, Duration};
use std::sync::atomic::{AtomicU32, Ordering};

pub struct DiscoveryRateLimiter {
    requests_this_hour: AtomicU32,
    last_reset: std::sync::Mutex<std::time::Instant>,
    max_requests_per_hour: u32,
}

impl DiscoveryRateLimiter {
    pub fn new(max_requests_per_hour: u32) -> Self {
        Self {
            requests_this_hour: AtomicU32::new(0),
            last_reset: std::sync::Mutex::new(std::time::Instant::now()),
            max_requests_per_hour,
        }
    }
    
    pub async fn check_rate_limit(&self) -> Result<(), &'static str> {
        // Reset counter if hour has passed
        {
            let mut last_reset = self.last_reset.lock().unwrap();
            if last_reset.elapsed() >= std::time::Duration::from_secs(3600) {
                self.requests_this_hour.store(0, Ordering::SeqCst);
                *last_reset = std::time::Instant::now();
            }
        }
        
        let current_count = self.requests_this_hour.load(Ordering::SeqCst);
        if current_count >= self.max_requests_per_hour {
            return Err("Rate limit exceeded");
        }
        
        self.requests_this_hour.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    
    pub async fn with_backoff<F, T>(
        &self,
        mut operation: F
    ) -> Result<T, Box<dyn std::error::Error>>
    where
        F: FnMut() -> futures::future::BoxFuture<'static, Result<T, Box<dyn std::error::Error>>>,
    {
        let mut delay = Duration::from_millis(100);
        let max_delay = Duration::from_secs(30);
        let max_retries = 5;
        
        for attempt in 0..max_retries {
            self.check_rate_limit().await.map_err(|e| e.to_string())?;
            
            match operation().await {
                Ok(result) => return Ok(result),
                Err(e) if attempt == max_retries - 1 => return Err(e),
                Err(_) => {
                    sleep(delay).await;
                    delay = std::cmp::min(delay * 2, max_delay);
                }
            }
        }
        
        unreachable!()
    }
}
```

## 💡 Best Practices

### 1. **Start Specific, Then Broaden**

- Begin with as many contextual hints as possible
- Progressively remove hints if no results found
- Use fuzzy matching as a last resort

### 2. **Respect Privacy and Consent**

- Always explain why you want to contact someone
- Use appropriate permission levels for your use case
- Don't spam contact requests

### 3. **Cache Discovery Results**

- Save successful lookups to avoid repeated queries
- Set reasonable TTLs for cached data
- Respect "not found" results to avoid retry loops

### 4. **Handle Edge Cases Gracefully**

- Plan for ambiguous results
- Implement proper error handling
- Provide helpful suggestions when searches fail

### 5. **Use Appropriate Urgency Levels**

- `Emergency`: True emergencies only
- `High`: Urgent business needs
- `Interactive`: Real-time conversations
- `Background`: Non-urgent coordination

This cookbook provides practical, ready-to-use patterns for the most common unknown name resolution scenarios in EMRP.
