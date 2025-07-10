# 🧠 **Synapse**: Intelligent Communication Network

## Project Renaming Proposal: EMRP → Synapse

### Why "Synapse"?

**Synapse** perfectly captures the essence of this project as a neural communication network:

- **Scientific Foundation**: Synapses are junctions between neurons that enable communication and information transfer
- **Network Intelligence**: Creates connections between thinking entities (humans, AIs, services)
- **Information Transfer**: Like synapses transmit neurotransmitters, Synapse transmits messages and knowledge
- **Distributed Intelligence**: Forms a "neural network" of intelligent participants
- **Memorable & Professional**: Short, scientific, brandable, not overused in tech

### Biological Analogy

| Neuroscience | Synapse Project |
|-------------|-----------------|
| **Neurons** | Participants (humans, AIs, bots, services) |
| **Synapses** | Communication protocol and participant registry |
| **Neural Networks** | Federated server networks |
| **Neurotransmitters** | Messages and shared knowledge |
| **Action Potentials** | Priority levels and urgency |
| **Myelin Sheaths** | Encryption and security |
| **Neural Plasticity** | Learning and adaptation |

## Enhanced Trust Architecture

### Dual Trust System

#### 1. Entity-to-Entity Trust (Subjective)
```rust
pub struct DirectTrustScore {
    pub score: u8, // 0-100
    pub category: TrustCategory,
    pub given_by: String,
    pub given_at: DateTime<Utc>,
    pub comment: Option<String>,
    pub relationship_context: RelationshipType,
}

pub enum TrustCategory {
    Communication,  // Clear, respectful, responsive
    Technical,      // Competent, knowledgeable, helpful
    Collaboration,  // Good team player, reliable
    Reliability,    // Follows through on commitments
    Privacy,        // Respects confidentiality and data
    Overall,        // General trustworthiness
}
```

**Characteristics**:
- Personal experience-based
- Subjective and contextual
- Private between entities
- Relationship-specific

#### 2. Entity-to-Network Trust (Objective)
```rust
pub struct NetworkTrustRating {
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
```

**Characteristics**:
- Blockchain-verified immutable records
- Publicly auditable
- Community-witnessed actions
- Objective scoring based on verifiable behavior

### Trust Integration in Discovery

```rust
impl ContactResolver {
    pub async fn calculate_composite_trust(
        &self,
        target_profile: &ParticipantProfile,
        requester_profile: &ParticipantProfile,
    ) -> Result<CompositeTrustScore> {
        let network_proximity = self.calculate_network_proximity(
            &target_profile.global_id, 
            &requester_profile.global_id
        ).await?;
        
        let direct_trust = target_profile.trust_ratings.entity_trust
            .received_ratings
            .get(&requester_profile.global_id)
            .map(|score| score.score as f64)
            .unwrap_or(50.0); // Neutral if no direct experience
        
        let network_trust = target_profile.trust_ratings.network_trust.network_score;
        
        // Weighted composite calculation
        let composite_score = (
            0.4 * direct_trust +                    // Personal experience most important
            0.3 * network_trust +                   // Network reputation significant
            0.2 * proximity_score(network_proximity) + // Network proximity matters
            0.1 * verification_bonus(target_profile)   // Verification bonus
        );
        
        Ok(CompositeTrustScore {
            composite: composite_score,
            direct_trust,
            network_trust,
            network_proximity,
            components: TrustComponents {
                personal_experience: direct_trust,
                network_reputation: network_trust,
                network_proximity,
                identity_verification: target_profile.federation_config.identity_verification.verification_level,
            }
        })
    }
}
```

### Blockchain Integration for Network Trust

```rust
pub struct BlockchainTrustManager {
    blockchain_client: BlockchainClient,
    trust_contract: SmartContract,
}

impl BlockchainTrustManager {
    /// Record a verifiable action on the blockchain
    pub async fn record_verifiable_action(
        &self,
        action: &VerifiableAction,
        witnesses: &[String],
    ) -> Result<String> { // Returns blockchain hash
        let action_record = ActionRecord {
            participant_id: action.participant_id.clone(),
            action_type: action.action_type.clone(),
            timestamp: action.timestamp,
            witnesses: witnesses.to_vec(),
            impact_score: action.impact_score,
            description: action.description.clone(),
        };
        
        // Submit to blockchain with witness signatures
        let transaction = self.trust_contract
            .record_action(action_record, witnesses)
            .await?;
        
        Ok(transaction.hash)
    }
    
    /// Verify an action's authenticity
    pub async fn verify_action(
        &self,
        blockchain_hash: &str,
    ) -> Result<Option<VerifiedAction>> {
        self.blockchain_client
            .get_transaction(blockchain_hash)
            .await
    }
    
    /// Calculate network trust score from blockchain history
    pub async fn calculate_network_trust(
        &self,
        participant_id: &str,
    ) -> Result<f64> {
        let actions = self.get_participant_actions(participant_id).await?;
        
        let mut total_score = 0i32;
        let mut action_count = 0u32;
        let mut recency_weight = 1.0f64;
        
        // Weight recent actions more heavily
        for action in actions.iter().rev() { // Most recent first
            total_score += (action.impact_score as f64 * recency_weight) as i32;
            action_count += 1;
            recency_weight *= 0.95; // Decay factor for older actions
        }
        
        // Normalize to 0-100 scale
        let base_score = if action_count > 0 {
            (total_score as f64 / action_count as f64).max(0.0).min(100.0)
        } else {
            50.0 // Neutral for new participants
        };
        
        // Apply participation bonus
        let participation_bonus = self.calculate_participation_bonus(participant_id).await?;
        
        Ok((base_score + participation_bonus).min(100.0))
    }
}
```

## Expert Availability Controls

### Granular Expert Preferences

```rust
pub struct ExpertContactPreferences {
    // Core availability
    pub accepts_unsolicited_questions: bool,
    pub requires_introduction: bool,
    pub preferred_contact_method: ContactMethod,
    
    // Trust-based filtering
    pub min_requester_trust_score: Option<f64>,
    pub min_network_trust_score: Option<f64>,
    pub allowed_requester_types: Vec<EntityType>,
    
    // Content filtering
    pub question_complexity_preferences: ComplexityPreferences,
    pub topic_scope_limits: Vec<String>, // Sub-topics within expertise
    pub requires_prior_research: bool,   // Must show they've done homework
    
    // Rate limiting
    pub rate_limits: ExpertRateLimits,
    
    // Time-based availability
    pub availability_schedule: AvailabilitySchedule,
}

pub struct ComplexityPreferences {
    pub accepts_beginner_questions: bool,
    pub accepts_intermediate_questions: bool,
    pub accepts_advanced_questions: bool,
    pub accepts_research_collaboration: bool,
    
    // Automatic responses for different complexity levels
    pub beginner_auto_response: Option<String>,
    pub advanced_requirements: Option<String>,
}

pub struct AvailabilitySchedule {
    pub time_zone: String,
    pub business_hours: Option<BusinessHours>,
    pub available_days: Vec<chrono::Weekday>,
    pub vacation_periods: Vec<DateRange>,
    pub office_hours: Option<BusinessHours>, // Different from general availability
}

pub struct ExpertRateLimits {
    pub max_questions_per_day: Option<u32>,
    pub max_questions_per_week: Option<u32>,
    pub max_concurrent_conversations: Option<u32>,
    pub cooldown_period_per_requester: Option<chrono::Duration>,
    pub cooldown_period_general: Option<chrono::Duration>,
    
    // Dynamic rate limiting based on trust
    pub rate_multiplier_for_trusted: f64,    // Higher limits for trusted users
    pub rate_multiplier_for_untrusted: f64,  // Lower limits for untrusted users
}
```

### Expert Availability Engine

```rust
pub struct ExpertAvailabilityEngine {
    database: DatabaseManager,
    rate_limiter: RateLimitManager,
    trust_calculator: TrustCalculator,
}

impl ExpertAvailabilityEngine {
    /// Check if an expert can be contacted for a specific topic
    pub async fn check_expert_availability(
        &self,
        expert_id: &str,
        topic: &str,
        requester_id: &str,
        question_complexity: QuestionComplexity,
    ) -> Result<ExpertAvailabilityResult> {
        let expert_profile = self.database.get_participant(expert_id).await?;
        let requester_profile = self.database.get_participant(requester_id).await?;
        
        // Find relevant topic subscription
        let topic_subscription = expert_profile.topic_subscriptions
            .iter()
            .find(|sub| sub.topic == topic)
            .ok_or_else(|| anyhow::anyhow!("Expert not subscribed to this topic"))?;
        
        // Check availability status
        match topic_subscription.availability.status {
            AvailabilityStatus::Offline => {
                return Ok(ExpertAvailabilityResult::Unavailable(
                    "Expert is currently offline".to_string()
                ));
            }
            AvailabilityStatus::DoNotDisturb => {
                return Ok(ExpertAvailabilityResult::Unavailable(
                    "Expert is in do-not-disturb mode".to_string()
                ));
            }
            AvailabilityStatus::Busy => {
                // Only allow high-priority or trusted contacts
                let trust_info = self.trust_calculator
                    .calculate_composite_trust(expert_id, requester_id)
                    .await?;
                
                if trust_info.composite_score < 80.0 {
                    return Ok(ExpertAvailabilityResult::RequiresApproval(
                        "Expert is busy, requires high trust score".to_string()
                    ));
                }
            }
            _ => {} // Available, MentorMode, CollabMode - continue checking
        }
        
        // Check contact preferences
        let contact_decision = topic_subscription.contact_preferences
            .allows_contact(
                &requester_profile,
                &trust_info,
                question_complexity,
            );
        
        match contact_decision {
            ContactDecision::Allowed => {},
            ContactDecision::RequiresIntroduction => {
                return Ok(ExpertAvailabilityResult::RequiresIntroduction);
            }
            ContactDecision::InsufficientTrust => {
                return Ok(ExpertAvailabilityResult::InsufficientTrust(trust_info.composite_score));
            }
            ContactDecision::ComplexityNotAccepted => {
                return Ok(ExpertAvailabilityResult::ComplexityRejected(question_complexity));
            }
            _ => {
                return Ok(ExpertAvailabilityResult::Rejected(format!("{:?}", contact_decision)));
            }
        }
        
        // Check rate limits
        if !self.rate_limiter.check_expert_rate_limit(expert_id, requester_id).await? {
            return Ok(ExpertAvailabilityResult::RateLimited);
        }
        
        // Check time-based availability
        if !self.is_within_availability_window(expert_id).await? {
            let next_available = self.get_next_availability_window(expert_id).await?;
            return Ok(ExpertAvailabilityResult::OutsideHours(next_available));
        }
        
        Ok(ExpertAvailabilityResult::Available {
            estimated_response_time: self.estimate_response_time(expert_id).await?,
            preferred_contact_method: topic_subscription.contact_preferences.preferred_contact_method.clone(),
            auto_response: topic_subscription.availability.auto_response_message.clone(),
        })
    }
    
    /// Find available experts for a topic with filtering
    pub async fn find_available_experts(
        &self,
        topic: &str,
        requester_id: &str,
        filters: &ExpertSearchFilters,
    ) -> Result<Vec<AvailableExpert>> {
        let mut available_experts = Vec::new();
        
        // Get all experts for this topic
        let topic_experts = self.database.find_topic_experts(topic).await?;
        
        for expert in topic_experts {
            match self.check_expert_availability(
                &expert.participant_id,
                topic,
                requester_id,
                filters.question_complexity.unwrap_or(QuestionComplexity::Intermediate),
            ).await? {
                ExpertAvailabilityResult::Available { 
                    estimated_response_time,
                    preferred_contact_method,
                    auto_response 
                } => {
                    available_experts.push(AvailableExpert {
                        profile: expert,
                        estimated_response_time,
                        preferred_contact_method,
                        auto_response,
                    });
                }
                _ => {} // Expert not available, skip
            }
        }
        
        // Sort by availability, expertise, and trust
        available_experts.sort_by(|a, b| {
            self.calculate_expert_ranking_score(a, b, requester_id)
        });
        
        Ok(available_experts)
    }
}

pub enum ExpertAvailabilityResult {
    Available {
        estimated_response_time: Option<chrono::Duration>,
        preferred_contact_method: ContactMethod,
        auto_response: Option<String>,
    },
    Unavailable(String),
    RequiresIntroduction,
    RequiresApproval(String),
    InsufficientTrust(f64),
    ComplexityRejected(QuestionComplexity),
    RateLimited,
    OutsideHours(Option<DateTime<Utc>>),
    Rejected(String),
}
```

## Project Roadmap with New Features

### Phase 1: Foundation + Trust System (Weeks 1-4)
- Implement dual trust architecture
- Blockchain integration for network trust
- Basic expert availability controls

### Phase 2: Expert Management + Advanced Discovery (Weeks 5-8)
- Complete expert preference system
- Topic-based discovery with availability filtering
- Trust-based routing decisions

### Phase 3: Network Intelligence + Federation (Weeks 9-12)
- Intelligent expert matching
- Federated trust propagation
- Cross-protocol expert discovery

### Phase 4: Production + Analytics (Weeks 13-16)
- Trust analytics and reporting
- Expert utilization optimization
- Network health monitoring

## Benefits of This Architecture

### For Experts
- **Granular Control**: Precise control over who can contact them and when
- **Quality Filtering**: Avoid low-quality or repetitive questions
- **Trust-Based Access**: Higher trust users get better access
- **Time Management**: Respect availability and rate limits

### For Question Askers
- **Quality Matching**: Find experts who actually want to help
- **Trust Transparency**: Understand expert preferences before contacting
- **Efficient Discovery**: Don't waste time on unavailable experts
- **Learning Pathways**: Guidance on complexity requirements

### For the Network
- **Reputation Integrity**: Blockchain-verified trust prevents gaming
- **Expert Sustainability**: Prevents expert burnout through rate limiting
- **Quality Assurance**: Trust scores improve overall interaction quality
- **Network Growth**: Positive reputation incentivizes good behavior

This enhanced architecture transforms Synapse into a sophisticated intelligence network that respects both expert availability and network trust, creating a sustainable ecosystem for knowledge sharing and collaboration between humans and AI.
