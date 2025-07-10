# 🧠 Synapse: Enhanced Identity Resolution and Neural Communication Network

## Current State Analysis

Synapse currently handles identity resolution through:
1. **Local Name Registry**: Simple name → Global ID mappings
2. **Manual Registration**: Explicit peer registration
3. **Basic Discovery**: mDNS for local network peers

## Problem: Unknown Name Resolution Gap

When someone wants to contact an unknown Synapse participant, the current system requires:
- Manual registration of every contact
- Prior knowledge of exact global IDs
- No discovery mechanism for other Synapse participants

## Comprehensive Solution: Synapse Participant Registry

### 🎯 Vision: Contextual, Privacy-Aware Neural Discovery

Allow natural contact patterns while respecting privacy:
- "Send a message to Alice from the AI Lab" (contextual lookup)
- "Contact the deployment bot for the web team" (role-based addressing)
- "Find the research assistant at Stanford" (organizational discovery)
- "Reach out to anyone working on climate modeling" (topic/interest-based)
- Priority routing for important relationships (boss, family, close collaborators)

### 🏗️ Multi-Layer Lookup Architecture

#### Layer 1: Contextual Hints Resolution
```rust
// Instead of just "Alice", accept rich context
let lookup_request = ContactLookupRequest {
    name: "Alice".to_string(),
    hints: vec![
        ContactHint::Organization("AI Lab".to_string()),
        ContactHint::Domain("example.com".to_string()),
        ContactHint::Role("Researcher".to_string()),
        ContactHint::EntityType(EntityType::AiModel),
    ],
    requester_context: RequesterContext {
        from_entity: "MyBot@company.com".to_string(),
        purpose: "Collaboration on ML project".to_string(),
        urgency: MessageUrgency::Interactive,
    },
};
```

#### Layer 2: Smart Discovery Methods
```rust
pub enum DiscoveryMethod {
    // Synapse Participant Registry (primary method)
    SynapseRegistry {
        registry_nodes: Vec<String>, // ["registry.synapse.org", "backup.synapse.org"]
        cache_ttl: Duration,
    },
    
    // Local network discovery
    LocalNetwork {
        mdns_enabled: bool,
        broadcast_discovery: bool,
    },
    
    // Peer referrals (ask known Synapse contacts)
    PeerReferral {
        ask_known_peers: bool,
        max_hops: u32,
    },
    
    // Simple domain inference for Synapse entities
    DomainInference {
        common_patterns: Vec<String>, // ["{name}@{domain}", "{name}.bot@{domain}"]
        known_synapse_domains: Vec<String>,
    },
}

// Enhanced registry data structure
pub struct ParticipantProfile {
    // Core identity
    pub global_id: String,              // "alice.work@ai-lab.com"
    pub display_name: String,           // "Alice Smith (Work)"
    pub entity_type: EntityType,        // Human, AiModel, Service
    pub identity_context: IdentityContext,
    
    // Organization and role
    pub organization: Option<String>,   // "AI Lab"
    pub role: Option<String>,          // "Research Assistant"
    pub capabilities: Vec<String>,      // ["machine_learning", "real_time"]
    
    // Discovery and privacy
    pub discovery_permissions: DiscoveryPermissions,
    pub forwarding_rules: Vec<ForwardingRule>,
    
    // Availability and contact preferences
    pub availability: AvailabilityRules,
    pub preferred_contact_methods: Vec<ContactMethod>,
    
    // Technical details
    pub public_key: Option<PublicKey>,
    pub supported_protocols: Vec<String>,
    pub last_seen: DateTime<Utc>,
    pub registry_metadata: RegistryMetadata,
}

pub enum IdentityContext {
    Professional { 
        organization: String, 
        role: String,
        business_hours: BusinessHours,
    },
    Personal {
        timezone: String,
        casual_contact_ok: bool,
    },
    Research { 
        institution: String,
        research_areas: Vec<String>,
    },
    Service { 
        service_type: String,
        availability: ServiceAvailability,
        auto_response: bool,
    },
}

// Trust and reputation system
pub struct TrustRatings {
    // Entity-to-entity trust (subjective, personal experience)
    pub entity_trust: EntityTrustRatings,
    
    // Entity-to-network trust (objective, publicly verifiable)
    pub network_trust: NetworkTrustRating,
    
    // Network proximity (degrees of separation)
    pub network_proximity: NetworkProximity,
    
    // Verification status
    pub identity_verification: IdentityVerification,
}

pub struct EntityTrustRatings {
    /// Direct trust ratings from other entities (subjective)
    pub received_ratings: HashMap<String, DirectTrustScore>,
    pub given_ratings: HashMap<String, DirectTrustScore>,
    pub average_received: f64,
    pub total_ratings: u32,
}

pub struct DirectTrustScore {
    pub score: u8, // 0-100
    pub category: TrustCategory,
    pub given_by: String,
    pub given_at: DateTime<Utc>,
    pub comment: Option<String>,
    pub relationship_context: RelationshipType,
}

pub struct NetworkTrustRating {
    /// Objective, publicly verifiable network reputation
    pub network_score: f64, // 0-100, calculated from verifiable actions
    pub blockchain_verified: bool,
    pub verifiable_actions: Vec<VerifiableAction>,
    pub participation_metrics: ParticipationMetrics,
    pub reputation_history: Vec<ReputationEvent>,
}

pub struct VerifiableAction {
    pub action_type: ActionType,
    pub timestamp: DateTime<Utc>,
    pub blockchain_hash: Option<String>, // Immutable proof
    pub witnesses: Vec<String>,          // Other participants who can verify
    pub impact_score: i32,               // Positive or negative impact
    pub description: String,
}

pub enum ActionType {
    // Positive actions (increase network trust)
    HelpfulResponse,      // Provided useful assistance
    KnowledgeSharing,     // Shared valuable information
    BugReport,           // Reported and helped fix issues
    Mentoring,           // Helped others learn
    Collaboration,       // Successful project collaboration
    
    // Negative actions (decrease network trust)
    Spam,                // Sent unsolicited/irrelevant messages
    Harassment,          // Inappropriate behavior
    Misinformation,      // Spread false information
    BadFaith,            // Failed to honor commitments
    Abuse,               // Misused the system
}

pub struct ParticipationMetrics {
    pub messages_sent: u64,
    pub messages_received: u64,
    pub successful_collaborations: u64,
    pub knowledge_contributions: u64,
    pub reports_against: u64,        // Times reported for bad behavior
    pub reports_validated: u64,      // Times reports were confirmed
    pub account_age: chrono::Duration,
    pub last_active: DateTime<Utc>,
}

pub struct NetworkProximity {
    /// Degrees of separation (objective network metric)
    pub degrees_of_separation: HashMap<String, u32>,
    pub connection_paths: HashMap<String, Vec<String>>,
    pub last_calculated: DateTime<Utc>,
}

pub struct DiscoveryPermissions {
    pub discoverable: DiscoverabilityLevel,
    pub discovery_allowlist: Vec<DiscoveryPermission>,
    pub discovery_blocklist: Vec<String>,
    pub require_introduction: bool,
    pub min_trust_score: Option<f64>,
    pub min_network_score: Option<f64>,
    pub public_profile_fields: PublicProfileFields,
}

pub enum DiscoverabilityLevel {
    Public,      // Anyone can discover, appears in searches
    Unlisted,    // Discoverable through referrals/hints but not in general searches
    Private,     // Not discoverable, direct contact only (requires exact contact info)
    Stealth,     // Completely invisible, even direct attempts fail unless pre-authorized
}
```

#### Layer 3: Permission and Consent System
```rust
pub struct ContactRequest {
    pub target_identity: DiscoveredIdentity,
    pub requester: String,
    pub purpose: String,
    pub requested_permissions: Vec<Permission>,
    pub expiry: DateTime<Utc>,
}

pub enum Permission {
    SingleMessage,
    Conversation(Duration),
    OngoingCollaboration,
    EmergencyContact,
}

pub enum ContactResponse {
    Approved {
        granted_permissions: Vec<Permission>,
        preferred_contact_method: ContactMethod,
        introduction_message: Option<String>,
    },
    ConditionalApproval {
        conditions: Vec<String>,
        requires_introduction: bool,
    },
    Declined {
        reason: Option<String>,
        suggest_alternative: Option<String>,
    },
    Deferred {
        ask_again_after: Duration,
        preferred_time: Option<String>,
    },
}
```

### 🤖 Implementation Strategy

#### Phase 1: Smart Name Resolution
```rust
impl IdentityRegistry {
    /// Enhanced resolution that tries multiple strategies
    pub async fn resolve_with_context(
        &mut self,
        name: &str,
        hints: Vec<ContactHint>,
        requester: &str,
    ) -> Result<ResolutionResult> {
        // Try exact local name first
        if let Some(global_id) = self.resolve_local_name(name) {
            return Ok(ResolutionResult::Direct(global_id.clone()));
        }
        
        // Try fuzzy matching of known names
        if let Some(matches) = self.fuzzy_match_local_names(name, 0.8) {
            return Ok(ResolutionResult::Suggestions(matches));
        }
        
        // Apply hint-based discovery
        let discovery_results = self.discover_with_hints(name, hints).await?;
        
        // If we found candidates, initiate contact request process
        if !discovery_results.is_empty() {
            return Ok(ResolutionResult::ContactRequestRequired(discovery_results));
        }
        
        Ok(ResolutionResult::NotFound)
    }
    
    /// Discover identities using contextual hints
    async fn discover_with_hints(
        &self,
        name: &str,
        hints: Vec<ContactHint>,
    ) -> Result<Vec<DiscoveredIdentity>> {
        let mut candidates = Vec::new();
        
        // DNS-based discovery
        candidates.extend(self.dns_discovery(name, &hints).await?);
        
        // Domain inference
        candidates.extend(self.domain_inference(name, &hints).await?);
        
        // Peer network queries
        candidates.extend(self.peer_network_discovery(name, &hints).await?);
        
        // Directory service lookups
        candidates.extend(self.directory_lookup(name, &hints).await?);
        
        // Deduplicate and rank by confidence
        candidates.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
        candidates.dedup_by(|a, b| a.global_id == b.global_id);
        
        Ok(candidates)
    }
    
    /// DNS-based discovery using hints
    async fn dns_discovery(
        &self,
        name: &str,
        hints: &[ContactHint],
    ) -> Result<Vec<DiscoveredIdentity>> {
        let mut candidates = Vec::new();
        let mut domains = Vec::new();
        
        // Extract domain hints
        for hint in hints {
            match hint {
                ContactHint::Domain(domain) => domains.push(domain.clone()),
                ContactHint::Organization(org) => {
                    // Try to infer domain from organization
                    domains.push(format!("{}.com", org.to_lowercase().replace(" ", "")));
                    domains.push(format!("{}.org", org.to_lowercase().replace(" ", "")));
                }
                _ => {}
            }
        }
        
        // If no domain hints, try common patterns
        if domains.is_empty() {
            domains = vec![
                "gmail.com".to_string(),
                "company.com".to_string(),
                "university.edu".to_string(),
            ];
        }
        
        // Try various email patterns
        for domain in domains {
            let patterns = vec![
                format!("{}@{}", name.to_lowercase(), domain),
                format!("{}.ai@{}", name.to_lowercase(), domain),
                format!("{}.bot@{}", name.to_lowercase(), domain),
            ];
            
            for pattern in patterns {
                if let Ok(discovered) = self.verify_email_exists(&pattern).await {
                    candidates.push(DiscoveredIdentity {
                        global_id: pattern,
                        local_name_suggestion: name.to_string(),
                        confidence: discovered.confidence,
                        discovery_method: "DNS".to_string(),
                        metadata: discovered.metadata,
                    });
                }
            }
        }
        
        Ok(candidates)
    }
    
    /// Query known peers about unknown contacts
    async fn peer_network_discovery(
        &self,
        name: &str,
        hints: &[ContactHint],
    ) -> Result<Vec<DiscoveredIdentity>> {
        let mut candidates = Vec::new();
        
        // Create discovery query
        let query = PeerDiscoveryQuery {
            target_name: name.to_string(),
            hints: hints.to_vec(),
            max_hops: 2,
            trust_threshold: 70,
        };
        
        // Ask trusted peers
        let trusted_peers = self.get_trusted_peers(80);
        for peer in trusted_peers {
            if let Ok(response) = self.send_discovery_query(&peer.global_id, &query).await {
                candidates.extend(response.candidates);
            }
        }
        
        Ok(candidates)
    }
}
```

#### Phase 2: Contact Request System
```rust
pub struct ContactRequestManager {
    pending_requests: HashMap<String, ContactRequest>,
    auto_approval_rules: Vec<AutoApprovalRule>,
    notification_handlers: Vec<Box<dyn NotificationHandler>>,
}

impl ContactRequestManager {
    /// Send a contact request to a discovered identity
    pub async fn send_contact_request(
        &mut self,
        target: &DiscoveredIdentity,
        requester: &str,
        purpose: &str,
        permissions: Vec<Permission>,
    ) -> Result<ContactRequestId> {
        let request = ContactRequest {
            id: Uuid::new_v4().to_string(),
            target_identity: target.clone(),
            requester: requester.to_string(),
            purpose: purpose.to_string(),
            requested_permissions: permissions,
            created_at: Utc::now(),
            expiry: Utc::now() + Duration::hours(24),
        };
        
        // Check auto-approval rules first
        if let Some(auto_response) = self.check_auto_approval(&request) {
            return Ok(self.handle_auto_approval(request, auto_response).await?);
        }
        
        // Send request to target
        let request_message = self.format_contact_request(&request);
        self.send_request_message(&target.global_id, request_message).await?;
        
        // Store pending request
        self.pending_requests.insert(request.id.clone(), request);
        
        Ok(request.id)
    }
    
    /// Handle incoming contact request
    pub async fn handle_incoming_request(
        &mut self,
        request: ContactRequest,
    ) -> Result<()> {
        // Notify user about incoming request
        for handler in &self.notification_handlers {
            handler.notify_contact_request(&request).await?;
        }
        
        // Store for user decision
        self.pending_requests.insert(request.id.clone(), request);
        
        Ok(())
    }
    
    /// User responds to a contact request
    pub async fn respond_to_request(
        &mut self,
        request_id: &str,
        response: ContactResponse,
    ) -> Result<()> {
        let request = self.pending_requests.remove(request_id)
            .ok_or_else(|| EmrpError::NotFound("Contact request not found".to_string()))?;
        
        // Send response back to requester
        let response_message = self.format_contact_response(&request, &response);
        self.send_response_message(&request.requester, response_message).await?;
        
        // If approved, register the contact
        if let ContactResponse::Approved { .. } = response {
            self.register_approved_contact(&request).await?;
        }
        
        Ok(())
    }
}
```

### 🧠 Synapse Custom Blockchain for Trust Verification

#### Trust Point Staking System
```rust
use sha2::{Sha256, Digest};
use chrono::{DateTime, Utc, Duration};

pub struct SynapseBlockchain {
    pub blocks: Vec<TrustBlock>,
    pub pending_reports: HashMap<String, PendingTrustReport>,
    pub trust_balances: HashMap<String, TrustBalance>,
    pub staking_pool: HashMap<String, Vec<TrustStake>>,
}

pub struct TrustBlock {
    pub index: u64,
    pub timestamp: DateTime<Utc>,
    pub previous_hash: String,
    pub merkle_root: String,
    pub trust_reports: Vec<VerifiedTrustReport>,
    pub validators: Vec<String>, // Participant IDs who validated this block
    pub hash: String,
}

pub struct VerifiedTrustReport {
    pub report_id: String,
    pub target_participant: String,
    pub action_type: ActionType,
    pub impact_score: i32,
    pub description: String,
    pub evidence: Option<String>,
    pub reporters: Vec<TrustReporter>,
    pub validation_score: f64, // Consensus score from stakers
    pub timestamp: DateTime<Utc>,
}

pub struct TrustReporter {
    pub participant_id: String,
    pub stake_amount: u32,
    pub reputation_weight: f64,
    pub report_timestamp: DateTime<Utc>,
}

pub struct TrustBalance {
    pub participant_id: String,
    pub total_points: u32,
    pub available_points: u32, // Not currently staked
    pub staked_points: u32,
    pub last_activity: DateTime<Utc>,
    pub decay_rate: f64, // Percentage decay per month
}

pub struct TrustStake {
    pub stake_id: String,
    pub participant_id: String,
    pub report_id: String,
    pub stake_amount: u32,
    pub stake_timestamp: DateTime<Utc>,
    pub supporting: bool, // true = supporting report, false = disputing
}

pub struct PendingTrustReport {
    pub report_id: String,
    pub target_participant: String,
    pub action_type: ActionType,
    pub impact_score: i32,
    pub description: String,
    pub evidence: Option<String>,
    pub initial_reporter: String,
    pub created_at: DateTime<Utc>,
    pub expiry: DateTime<Utc>,
    pub stakes_supporting: Vec<TrustStake>,
    pub stakes_disputing: Vec<TrustStake>,
    pub minimum_stake_required: u32,
    pub consensus_threshold: f64, // e.g., 0.67 for 67% consensus
}

impl SynapseBlockchain {
    /// Submit a new trust report with initial stake
    pub fn submit_trust_report(
        &mut self,
        reporter_id: &str,
        target_participant: &str,
        action_type: ActionType,
        impact_score: i32,
        description: String,
        evidence: Option<String>,
        initial_stake: u32,
    ) -> Result<String> {
        // Check if reporter has enough available trust points
        let balance = self.trust_balances.get(reporter_id)
            .ok_or_else(|| anyhow::anyhow!("Reporter not found"))?;
        
        if balance.available_points < initial_stake {
            return Err(anyhow::anyhow!("Insufficient trust points for stake"));
        }
        
        // Minimum stake requirement based on impact score
        let min_stake = (impact_score.abs() as u32 / 10).max(5);
        if initial_stake < min_stake {
            return Err(anyhow::anyhow!("Stake too low for reported impact"));
        }
        
        let report_id = uuid::Uuid::new_v4().to_string();
        
        // Create pending report
        let pending_report = PendingTrustReport {
            report_id: report_id.clone(),
            target_participant: target_participant.to_string(),
            action_type,
            impact_score,
            description,
            evidence,
            initial_reporter: reporter_id.to_string(),
            created_at: Utc::now(),
            expiry: Utc::now() + Duration::days(7), // 7 days to reach consensus
            stakes_supporting: vec![TrustStake {
                stake_id: uuid::Uuid::new_v4().to_string(),
                participant_id: reporter_id.to_string(),
                report_id: report_id.clone(),
                stake_amount: initial_stake,
                stake_timestamp: Utc::now(),
                supporting: true,
            }],
            stakes_disputing: vec![],
            minimum_stake_required: min_stake,
            consensus_threshold: 0.67,
        };
        
        // Lock the reporter's trust points
        self.stake_trust_points(reporter_id, initial_stake)?;
        
        // Add to pending reports
        self.pending_reports.insert(report_id.clone(), pending_report);
        
        Ok(report_id)
    }
    
    /// Stake trust points to support or dispute a pending report
    pub fn stake_on_report(
        &mut self,
        staker_id: &str,
        report_id: &str,
        stake_amount: u32,
        supporting: bool,
    ) -> Result<()> {
        // Check staker's available balance
        let balance = self.trust_balances.get(staker_id)
            .ok_or_else(|| anyhow::anyhow!("Staker not found"))?;
        
        if balance.available_points < stake_amount {
            return Err(anyhow::anyhow!("Insufficient trust points"));
        }
        
        // Find pending report
        let pending_report = self.pending_reports.get_mut(report_id)
            .ok_or_else(|| anyhow::anyhow!("Report not found"))?;
        
        // Check if report hasn't expired
        if Utc::now() > pending_report.expiry {
            return Err(anyhow::anyhow!("Report has expired"));
        }
        
        // Create stake
        let stake = TrustStake {
            stake_id: uuid::Uuid::new_v4().to_string(),
            participant_id: staker_id.to_string(),
            report_id: report_id.to_string(),
            stake_amount,
            stake_timestamp: Utc::now(),
            supporting,
        };
        
        // Add stake to appropriate pool
        if supporting {
            pending_report.stakes_supporting.push(stake);
        } else {
            pending_report.stakes_disputing.push(stake);
        }
        
        // Lock staker's trust points
        self.stake_trust_points(staker_id, stake_amount)?;
        
        // Check if consensus reached
        self.check_consensus_and_finalize(report_id)?;
        
        Ok(())
    }
    
    /// Check if consensus is reached and finalize the report
    fn check_consensus_and_finalize(&mut self, report_id: &str) -> Result<()> {
        let pending_report = self.pending_reports.get(report_id)
            .ok_or_else(|| anyhow::anyhow!("Report not found"))?
            .clone();
        
        // Calculate weighted consensus
        let supporting_weight = self.calculate_weighted_stake(&pending_report.stakes_supporting);
        let disputing_weight = self.calculate_weighted_stake(&pending_report.stakes_disputing);
        let total_weight = supporting_weight + disputing_weight;
        
        // Minimum participation threshold
        let min_total_stake = pending_report.minimum_stake_required * 3;
        if supporting_weight + disputing_weight < min_total_stake as f64 {
            return Ok(()); // Not enough participation yet
        }
        
        let consensus_ratio = supporting_weight / total_weight;
        
        // Check if consensus threshold reached
        if consensus_ratio >= pending_report.consensus_threshold || 
           consensus_ratio <= (1.0 - pending_report.consensus_threshold) {
            
            let consensus_reached = consensus_ratio >= pending_report.consensus_threshold;
            
            if consensus_reached {
                // Report accepted - create verified report
                let verified_report = VerifiedTrustReport {
                    report_id: pending_report.report_id.clone(),
                    target_participant: pending_report.target_participant.clone(),
                    action_type: pending_report.action_type.clone(),
                    impact_score: pending_report.impact_score,
                    description: pending_report.description.clone(),
                    evidence: pending_report.evidence.clone(),
                    reporters: pending_report.stakes_supporting.iter().map(|stake| {
                        TrustReporter {
                            participant_id: stake.participant_id.clone(),
                            stake_amount: stake.stake_amount,
                            reputation_weight: self.get_reputation_weight(&stake.participant_id),
                            report_timestamp: stake.stake_timestamp,
                        }
                    }).collect(),
                    validation_score: consensus_ratio,
                    timestamp: Utc::now(),
                };
                
                // Add to blockchain
                self.add_verified_report_to_blockchain(verified_report)?;
                
                // Return stakes to supporters, winners get bonus
                self.distribute_stake_rewards(&pending_report, true)?;
                
                // Update target's trust score
                self.apply_trust_score_change(
                    &pending_report.target_participant,
                    pending_report.impact_score,
                )?;
            } else {
                // Report rejected - return stakes to disputers
                self.distribute_stake_rewards(&pending_report, false)?;
            }
            
            // Remove from pending
            self.pending_reports.remove(report_id);
        }
        
        Ok(())
    }
    
    /// Calculate weighted stake considering staker reputation
    fn calculate_weighted_stake(&self, stakes: &[TrustStake]) -> f64 {
        stakes.iter().map(|stake| {
            let weight = self.get_reputation_weight(&stake.participant_id);
            stake.stake_amount as f64 * weight
        }).sum()
    }
    
    /// Get reputation weight for a participant (1.0 = normal, higher = more trusted)
    fn get_reputation_weight(&self, participant_id: &str) -> f64 {
        if let Some(balance) = self.trust_balances.get(participant_id) {
            // Higher trust points = higher weight, but with diminishing returns
            (balance.total_points as f64 / 100.0).sqrt().min(2.0).max(0.1)
        } else {
            0.5 // New participants have reduced weight
        }
    }
    
    /// Apply trust point decay over time
    pub fn apply_trust_decay(&mut self) -> Result<()> {
        let now = Utc::now();
        
        for (participant_id, balance) in self.trust_balances.iter_mut() {
            let days_since_activity = (now - balance.last_activity).num_days();
            
            if days_since_activity > 30 {
                // Apply monthly decay
                let months_elapsed = days_since_activity as f64 / 30.0;
                let decay_factor = (1.0 - balance.decay_rate).powf(months_elapsed);
                
                let new_total = (balance.total_points as f64 * decay_factor) as u32;
                let decay_amount = balance.total_points - new_total;
                
                balance.total_points = new_total;
                balance.available_points = balance.available_points.saturating_sub(decay_amount);
                
                println!("Applied decay to {}: {} points ({}%)", 
                         participant_id, decay_amount, balance.decay_rate * 100.0);
            }
        }
        
        Ok(())
    }
    
    /// Award trust points for verified positive actions
    pub fn award_trust_points(
        &mut self,
        participant_id: &str,
        points: u32,
        reason: &str,
    ) -> Result<()> {
        let balance = self.trust_balances.entry(participant_id.to_string())
            .or_insert_with(|| TrustBalance {
                participant_id: participant_id.to_string(),
                total_points: 0,
                available_points: 0,
                staked_points: 0,
                last_activity: Utc::now(),
                decay_rate: 0.02, // 2% per month default decay
            });
        
        balance.total_points += points;
        balance.available_points += points;
        balance.last_activity = Utc::now();
        
        println!("Awarded {} trust points to {} for: {}", points, participant_id, reason);
        
        Ok(())
    }
}
```

#### Phase 3: Smart Suggestions and Learning
```rust
impl IdentityRegistry {
    /// Learn from user interactions to improve suggestions
    pub fn learn_from_interaction(&mut self, interaction: ContactInteraction) {
        match interaction {
            ContactInteraction::Successful { from, to, context } => {
                // Increase trust between entities
                self.strengthen_relationship(&from, &to);
                
                // Learn domain patterns
                self.learn_domain_patterns(&from, &to, &context);
                
                // Update entity type classifications
                self.update_entity_classification(&to, &context);
            }
            ContactInteraction::Failed { from, to, reason } => {
                // Adjust confidence in discovery methods
                self.adjust_discovery_confidence(&to, &reason);
            }
        }
    }
    
    /// Suggest likely contacts based on context
    pub fn suggest_contacts(&self, context: &ConversationContext) -> Vec<ContactSuggestion> {
        let mut suggestions = Vec::new();
        
        // Analyze context for contact hints
        if let Some(hints) = self.extract_contact_hints(context) {
            suggestions.extend(self.generate_contextual_suggestions(&hints));
        }
        
        // Suggest based on recent interactions
        suggestions.extend(self.suggest_recent_contacts(context));
        
        // Suggest based on network analysis
        suggestions.extend(self.suggest_network_contacts(context));
        
        // Rank and deduplicate
        suggestions.sort_by_key(|s| s.confidence);
        suggestions.reverse();
        suggestions.truncate(10);
        
        suggestions
    }
}
```

### 🎛️ Configuration and Privacy Controls

```rust
pub struct DiscoveryConfig {
    // What discovery methods to enable
    pub enabled_methods: Vec<DiscoveryMethod>,
    
    // Privacy controls
    pub allow_being_discovered: bool,
    pub discovery_permissions: DiscoveryPermissions,
    
    // Auto-approval rules
    pub auto_approval_rules: Vec<AutoApprovalRule>,
    
    // Rate limiting
    pub max_discovery_requests_per_hour: u32,
    pub max_pending_requests: u32,
}

pub struct DiscoveryPermissions {
    pub discoverable_by_domain: Vec<String>,
    pub discoverable_by_entity_type: Vec<EntityType>,
    pub require_introduction: bool,
    pub public_profile_info: ProfileInfo,
}

pub struct AutoApprovalRule {
    pub condition: ApprovalCondition,
    pub action: ApprovalAction,
    pub priority: u32,
}

pub enum ApprovalCondition {
    FromDomain(String),
    FromEntityType(EntityType),
    WithPurpose(String),
    FromTrustedContact,
    EmergencyKeyword(String),
}
```

### 📝 Usage Examples

#### Example 1: AI Assistant Looking for Collaborator
```rust
// AI assistant wants to contact an unknown researcher
let lookup_request = ContactLookupRequest {
    name: "Dr. Sarah Chen".to_string(),
    hints: vec![
        ContactHint::Organization("Stanford AI Lab".to_string()),
        ContactHint::Role("Machine Learning Researcher".to_string()),
        ContactHint::Expertise("Computer Vision".to_string()),
    ],
    requester_context: RequesterContext {
        from_entity: "ResearchBot@myuniversity.edu".to_string(),
        purpose: "Collaboration on vision transformer research".to_string(),
        urgency: MessageUrgency::Background,
    },
};

match router.resolve_contact_with_context(lookup_request).await? {
    ResolutionResult::Direct(global_id) => {
        // Found exact match, send directly
        router.send_message_smart(&global_id, message, ...).await?;
    }
    
    ResolutionResult::ContactRequestRequired(candidates) => {
        // Found potential matches, request permission
        for candidate in candidates {
            let request_id = router.send_contact_request(
                &candidate,
                "Hello Dr. Chen, I'm a research assistant working on vision transformers. 
                 Would you be interested in discussing potential collaboration?",
                vec![Permission::Conversation(Duration::weeks(1))]
            ).await?;
            
            println!("Contact request sent: {}", request_id);
        }
    }
    
    ResolutionResult::Suggestions(similar) => {
        // Show user similar names for clarification
        println!("Did you mean one of these?");
        for suggestion in similar {
            println!("  - {}", suggestion);
        }
    }
    
    ResolutionResult::NotFound => {
        println!("Could not find contact matching that description");
    }
}
```

#### Example 2: Human Contacting Team
```rust
// Human wants to contact "the deployment team"
let lookup_request = ContactLookupRequest {
    name: "deployment team".to_string(),
    hints: vec![
        ContactHint::EntityType(EntityType::Service),
        ContactHint::Role("DevOps".to_string()),
        ContactHint::Organization("MyCompany".to_string()),
    ],
    requester_context: RequesterContext {
        from_entity: "john.doe@mycompany.com".to_string(),
        purpose: "Need help with production deployment issue".to_string(),
        urgency: MessageUrgency::RealTime,
    },
};

// System might discover:
// - deploy-bot@mycompany.com
// - devops-team@mycompany.com  
// - on-call@mycompany.com
```

### 🔒 Privacy and Security Considerations

1. **Consent-First**: All contact attempts require explicit permission
2. **Rate Limiting**: Prevent discovery spam and abuse
3. **Trust Networks**: Use existing relationships to validate new contacts
4. **Audit Trails**: Log all discovery attempts for security analysis
5. **Granular Controls**: Fine-grained permissions for different types of contact

This enhanced system would make EMRP much more powerful for natural, human-friendly communication while maintaining strong privacy and security controls.

---

# Expert Availability and Topic-Based Addressing

## Overview

To facilitate effective communication and collaboration, especially in research and professional settings, it's essential to not only resolve identities but also to understand the availability and topic expertise of potential contacts. This section describes the enhancements made to the EMRP to incorporate expert availability controls and contact preferences for topic-based addressing.

## Key Components

1. **Topic Subscription**: Participants can subscribe to topics with specific preferences and availability settings.
2. **Expert Availability**: Detailed availability status of participants, including working hours, time zone, and vacation status.
3. **Expert Contact Preferences**: Participants can define how they prefer to be contacted, including filtering options based on trust scores and requester types.
4. **Expert Rate Limits**: Controls to limit the number of questions or contact attempts from others, preventing spam and managing engagement levels.

## Detailed Design

### Topic Subscription
```rust
// Expert Availability Controls
pub struct TopicSubscription {
    pub topic: String,
    pub subscription_type: SubscriptionType,
    pub expertise_level: ExpertiseLevel,
    
    // Expert availability controls
    pub availability: ExpertAvailability,
    pub contact_preferences: ExpertContactPreferences,
    pub rate_limits: ExpertRateLimits,
    
    // Geographic and organizational scope
    pub geographic_scope: Option<Vec<String>>,
    pub organization_scope: Option<Vec<String>>,
}

pub struct ExpertAvailability {
    pub status: AvailabilityStatus,
    pub available_hours: Option<BusinessHours>,
    pub time_zone: String,
    pub vacation_mode: bool,
    pub auto_response_message: Option<String>,
}

pub enum AvailabilityStatus {
    Available,           // Open to questions/collaboration
    Busy,               // Limited availability, urgent only
    DoNotDisturb,       // No unsolicited contact
    MentorMode,         // Only accepting mentoring requests
    CollabMode,         // Only accepting collaboration requests
    Offline,            // Completely unavailable
}

pub struct ExpertContactPreferences {
    pub accepts_unsolicited_questions: bool,
    pub requires_introduction: bool,
    pub preferred_contact_method: ContactMethod,
    
    // Filtering preferences
    pub min_requester_trust_score: Option<f64>,
    pub allowed_requester_types: Vec<EntityType>,
    pub preferred_organizations: Vec<String>,
    pub blocked_domains: Vec<String>,
    
    // Question complexity preferences
    pub accepts_beginner_questions: bool,
    pub accepts_advanced_questions: bool,
    pub requires_prior_research: bool, // Must show they've done homework
}

pub struct ExpertRateLimits {
    pub max_questions_per_day: Option<u32>,
    pub max_questions_per_week: Option<u32>,
    pub max_questions_per_month: Option<u32>,
    pub max_concurrent_conversations: Option<u32>,
    pub cooldown_period: Option<chrono::Duration>, // Time between questions from same person
}

pub enum SubscriptionType {
    Expert {
        mentoring_available: bool,
        consultation_available: bool,
        collaboration_available: bool,
    },
    Interested {
        notifications_enabled: bool,
        discussion_participation: bool,
    },
    Learning {
        seeking_mentorship: bool,
        study_group_participation: bool,
    },
    Monitoring {
        alerts_enabled: bool,
        summary_frequency: SummaryFrequency,
    },
}
```

### Trust-based expert filtering
```rust
// Trust-based expert filtering
impl ExpertContactPreferences {
    pub fn allows_contact(
        &self,
        requester: &ParticipantProfile,
        trust_info: &TrustInfo,
        question_complexity: QuestionComplexity,
    ) -> ContactDecision {
        // Check if unsolicited questions are allowed
        if !self.accepts_unsolicited_questions && !self.has_prior_relationship(requester) {
            return ContactDecision::RequiresIntroduction;
        }
        
        // Check trust score requirements
        if let Some(min_trust) = self.min_requester_trust_score {
            if trust_info.composite_score < min_trust {
                return ContactDecision::InsufficientTrust;
            }
        }
        
        // Check entity type allowlist
        if !self.allowed_requester_types.is_empty() {
            if !self.allowed_requester_types.contains(&requester.entity_type) {
                return ContactDecision::EntityTypeNotAllowed;
            }
        }
        
        // Check question complexity preferences
        match question_complexity {
            QuestionComplexity::Beginner if !self.accepts_beginner_questions => {
                return ContactDecision::ComplexityNotAccepted;
            }
            QuestionComplexity::Advanced if !self.accepts_advanced_questions => {
                return ContactDecision::ComplexityNotAccepted;
            }
            _ => {}
        }
        
        ContactDecision::Allowed
    }
}

pub enum ContactDecision {
    Allowed,
    RequiresIntroduction,
    InsufficientTrust,
    EntityTypeNotAllowed,
    ComplexityNotAccepted,
    RateLimitExceeded,
    ExpertUnavailable,
}

pub enum QuestionComplexity {
    Beginner,      // Basic questions, learning fundamentals
    Intermediate,  // Practical application questions
    Advanced,      // Complex technical or research questions
    Research,      // Cutting-edge research collaboration
}
```

## Integration with EMRP

- **Discovery Process**: When discovering contacts, the system will also retrieve and consider the topic subscriptions and availability of participants.
- **Contact Requests**: The contact request system will include checks for the availability and preferences of the expert being contacted.
- **Rate Limiting**: Integrated into the contact request and messaging systems to prevent abuse.

## Usage Scenarios

1. **Research Collaboration**: A researcher can find and contact experts in their field who are currently available for collaboration, based on their topic subscriptions and availability status.
2. **Mentorship**: Individuals seeking mentorship can be matched with potential mentors who have indicated their availability and willingness to mentor.
3. **Service Requests**: Users can request services or consultations from experts who are available and have the relevant expertise.

## Privacy and Security Considerations

- All existing privacy and security measures apply, with additional considerations for the handling of availability information and topic subscriptions.
- Users must explicitly opt-in to be discoverable based on their topic subscriptions and availability.
- Fine-grained permissions control who can see and request to contact users based on their availability and expertise.

This enhancement to the EMRP provides a powerful way to not only resolve identities but also to engage with the right experts at the right time, significantly improving the utility and user experience of the EMRP system.
