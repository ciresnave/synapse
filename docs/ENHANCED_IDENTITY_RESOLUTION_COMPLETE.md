# 🔍 Enhanced Identity Resolution for Unknown Names

## Current State Analysis

EMRP currently handles identity resolution through:
1. **Local Name Registry**: Simple name → Global ID mappings
2. **Manual Registration**: Explicit peer registration
3. **Basic Discovery**: mDNS for local network peers

## Problem: Unknown Name Resolution Gap

When someone wants to contact an unknown EMRP entity, the current system requires:
- Manual registration of every contact
- Prior knowledge of exact global IDs
- No discovery mechanism for other EMRP participants

## Comprehensive Solution: EMRP Participant Registry

### 🎯 Vision: Contextual, Privacy-Aware EMRP Discovery

Allow natural contact patterns while respecting privacy:

- "Send a message to Alice from the AI Lab" (contextual lookup)
- "Contact the deployment bot for the web team" (role-based addressing)
- "Find the research assistant at Stanford" (organizational discovery)
- "Reach out to anyone working on climate modeling" (topic/interest-based)
- Priority routing for important relationships (boss, family, close collaborators)

### 🏗️ Multi-Layer Architecture

#### Layer 1: Enhanced Data Structures

```rust
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantProfile {
    pub global_id: String,
    pub identities: Vec<IdentityContext>,
    pub discovery_permissions: DiscoveryPermissions,
    pub forwarding_rules: Vec<ForwardingRule>,
    pub trust_ratings: TrustRatings,
    pub relationships: Vec<Relationship>,
    pub topic_subscriptions: Vec<TopicSubscription>,
    pub organizational_context: Option<OrganizationalContext>,
    pub priority_settings: PrioritySettings,
    pub federation_config: FederationConfig,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityContext {
    pub name: String,
    pub email_address: Option<String>,
    pub context_type: IdentityType,
    pub role: Option<String>,
    pub organization: Option<String>,
    pub department: Option<String>,
    pub capabilities: Vec<String>,
    pub public_profile: PublicProfile,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IdentityType {
    Personal,
    Professional,
    Service,
    Bot,
    Organization,
    Department,
    Role,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryPermissions {
    pub discoverability: DiscoverabilityLevel,
    pub searchable_fields: Vec<SearchableField>,
    pub contact_methods: Vec<ContactMethod>,
    pub require_introduction: bool,
    pub auto_accept_from: Vec<String>, // Global IDs or patterns
    pub public_profile_fields: PublicProfileFields,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiscoverabilityLevel {
    /// Profile appears in search results, anyone can find and contact
    /// Like a listed phone number or public social media profile
    Public,
    
    /// Profile doesn't appear in general searches but can be contacted if someone knows exact address
    /// Supports referral-based discovery (friend can share your contact)
    /// Like an unlisted phone number - private but reachable if known
    Unlisted,
    
    /// Only people you've explicitly allowed can contact you
    /// Requires mutual connection or invitation
    /// Like requiring approval for all contact requests
    Private,
    
    /// Completely invisible unless pre-authorized
    /// No trace in any searches or referrals
    /// Maximum privacy mode
    Stealth,
}
```

#### Layer 2: Trust and Reputation System

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustRatings {
    // Network proximity (separate from trust ratings)
    pub network_proximity: NetworkProximity,
    
    // Explicit trust scores
    pub entity_trust: EntityTrustRatings,
    pub network_trust: NetworkTrustRating,
    
    // Verification status
    pub identity_verification: IdentityVerification,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkProximity {
    /// Degrees of separation in the EMRP network
    /// 0 = direct connection, 1 = friend of friend, etc.
    pub degrees_of_separation: HashMap<String, u32>,
    
    /// Path through the network to reach this entity
    pub connection_paths: HashMap<String, Vec<String>>,
    
    /// Last updated timestamp
    pub last_calculated: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityTrustRatings {
    /// Trust ratings from other entities to this one
    /// Key: entity global_id, Value: trust score (0-100)
    pub received_ratings: HashMap<String, TrustScore>,
    
    /// Trust ratings this entity has given to others
    pub given_ratings: HashMap<String, TrustScore>,
    
    /// Aggregate scores
    pub average_received: f64,
    pub total_ratings: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustScore {
    pub score: u8, // 0-100
    pub category: TrustCategory,
    pub given_by: String,
    pub given_at: DateTime<Utc>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TrustCategory {
    Communication, // Reliable, respectful communication
    Technical,     // Technical competence
    Collaboration, // Good team player
    Reliability,   // Follows through on commitments
    Privacy,       // Respects privacy and data
    Overall,       // General trust level
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkTrustRating {
    /// This entity's overall reputation in the network
    pub network_score: f64,
    
    /// Participation metrics
    pub messages_sent: u64,
    pub messages_received: u64,
    pub collaborations: u64,
    pub reported_issues: u64,
    
    /// Time-based metrics
    pub account_age: chrono::Duration,
    pub last_active: DateTime<Utc>,
}
```

#### Layer 3: Relationship and Priority System

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    pub with_entity: String, // Global ID
    pub relationship_type: RelationshipType,
    pub priority_level: PriorityLevel,
    pub established_at: DateTime<Utc>,
    pub interaction_frequency: InteractionFrequency,
    pub mutual: bool, // Is this relationship acknowledged by both parties?
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RelationshipType {
    // Work relationships
    Boss,
    DirectReport,
    Colleague,
    Collaborator,
    TeamMember,
    
    // Personal relationships
    Family,
    Friend,
    Acquaintance,
    
    // Service relationships
    ServiceProvider,
    Customer,
    Support,
    
    // Automated relationships
    Bot,
    Service,
    Monitor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PriorityLevel {
    Critical,   // Always gets through, immediate notification
    High,       // Gets through during work hours, prioritized
    Normal,     // Standard priority
    Low,        // Can be delayed, batched
    Background, // Minimal priority, batch processing
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrioritySettings {
    pub default_priority: PriorityLevel,
    pub relationship_priorities: HashMap<RelationshipType, PriorityLevel>,
    pub topic_priorities: HashMap<String, PriorityLevel>,
    pub time_based_rules: Vec<TimeBasedPriorityRule>,
    pub emergency_keywords: Vec<String>, // Words that bump priority
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeBasedPriorityRule {
    pub time_range: TimeRange,
    pub days_of_week: Vec<chrono::Weekday>,
    pub priority_modifier: PriorityModifier,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PriorityModifier {
    Increase(u8),   // Bump up priority
    Decrease(u8),   // Lower priority
    Block,          // Don't deliver during this time
    ForceDelivery,  // Always deliver regardless of other rules
}
```

#### Layer 4: Topic-Based Addressing

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicSubscription {
    pub topic: String,
    pub subscription_type: SubscriptionType,
    pub expertise_level: ExpertiseLevel,
    pub availability: AvailabilityStatus,
    pub auto_respond: bool,
    pub filters: Vec<TopicFilter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubscriptionType {
    Expert,      // I can help with this topic
    Interested,  // I want to know about this topic
    Monitoring,  // I track this for awareness
    Learning,    // I'm learning about this
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExpertiseLevel {
    Beginner,
    Intermediate,
    Advanced,
    Expert,
    Authority, // Recognized expert in this field
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicFilter {
    pub filter_type: FilterType,
    pub pattern: String,
    pub action: FilterAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FilterType {
    Keyword,
    Sender,
    UrgencyLevel,
    MessageType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FilterAction {
    Accept,
    Reject,
    RequireApproval,
    AutoRespond(String), // Auto-respond with this message
}

// Topic-based message routing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicBasedRouting {
    pub topic: String,
    pub message_type: TopicMessageType,
    pub required_expertise: Option<ExpertiseLevel>,
    pub max_recipients: Option<u32>,
    pub geographic_constraints: Option<Vec<String>>,
    pub organization_constraints: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TopicMessageType {
    Question,      // Seeking help/information
    Announcement,  // Broadcasting information
    Discussion,    // Starting a conversation
    Collaboration, // Seeking collaboration
    Emergency,     // Urgent assistance needed
}
```

#### Layer 5: Organizational Boundaries

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationalContext {
    pub organization_id: String,
    pub organization_name: String,
    pub department: Option<String>,
    pub team: Option<String>,
    pub role: String,
    pub access_level: AccessLevel,
    pub internal_directory: bool, // Is this an internal directory entry?
    pub cross_org_permissions: CrossOrgPermissions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccessLevel {
    Public,        // Anyone can see this info
    Internal,      // Only organization members
    Department,    // Only department members
    Team,          // Only team members
    Confidential,  // Restricted access
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossOrgPermissions {
    pub allow_external_contact: bool,
    pub external_contact_approval: ApprovalLevel,
    pub partner_organizations: Vec<String>, // Trusted organization IDs
    pub blocked_organizations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ApprovalLevel {
    None,           // No approval needed
    Automatic,      // Auto-approve based on rules
    ManagerApproval,// Requires manager approval
    SecurityApproval,// Requires security team approval
    Manual,         // Manual review required
}
```

#### Layer 6: Federation and Cross-Protocol

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederationConfig {
    pub home_server: String,
    pub federated_servers: Vec<FederatedServer>,
    pub cross_protocol_mappings: Vec<CrossProtocolMapping>,
    pub data_sovereignty: DataSovereigntyRules,
    pub identity_verification: IdentityVerification,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederatedServer {
    pub server_id: String,
    pub server_url: String,
    pub trust_level: ServerTrustLevel,
    pub capabilities: Vec<ServerCapability>,
    pub data_sharing_agreement: DataSharingLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerTrustLevel {
    Trusted,      // Full federation
    Verified,     // Verified but limited
    Provisional,  // Trial period
    Restricted,   // Limited federation
    Blocked,      // No federation
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossProtocolMapping {
    pub protocol: String, // email, matrix, slack, etc.
    pub address: String,
    pub verification_status: VerificationStatus,
    pub sync_enabled: bool,
    pub forwarding_rules: Vec<ForwardingRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSovereigntyRules {
    pub data_residency: Vec<String>, // Allowed countries/regions
    pub encryption_requirements: EncryptionLevel,
    pub retention_policy: RetentionPolicy,
    pub compliance_frameworks: Vec<String>, // GDPR, HIPAA, etc.
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityVerification {
    pub verification_method: VerificationMethod,
    pub verified_at: Option<DateTime<Utc>>,
    pub verified_by: Option<String>,
    pub verification_level: VerificationLevel,
    pub oauth_providers: Vec<OAuthProvider>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VerificationMethod {
    OAuth2(String),      // OAuth 2.0 provider
    EmailVerification,   // Email-based verification
    DomainOwnership,     // Domain ownership verification
    CorporateDirectory,  // Corporate LDAP/AD
    WebOfTrust,          // Peer verification
    CryptographicProof,  // Cryptographic identity proof
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthProvider {
    pub provider_name: String,
    pub provider_url: String,
    pub client_id: String,
    pub verified_claims: Vec<String>,
    pub verification_timestamp: DateTime<Utc>,
}
```

### 🔍 Discovery and Lookup Implementation

#### Smart Contact Resolution

```rust
pub struct ContactResolver {
    registry: ParticipantRegistry,
    trust_calculator: TrustCalculator,
    priority_engine: PriorityEngine,
    topic_router: TopicRouter,
    federation_client: FederationClient,
}

impl ContactResolver {
    /// Resolve a contact using natural language query with context
    pub async fn resolve_contact(
        &self,
        query: &ContactQuery,
        requester: &str,
    ) -> Result<Vec<ContactCandidate>> {
        let mut candidates = Vec::new();
        
        // 1. Try direct lookup first
        if let Some(direct) = self.try_direct_lookup(&query.name).await? {
            candidates.push(direct);
        }
        
        // 2. Contextual lookup using hints
        if !query.hints.is_empty() {
            let contextual = self.contextual_lookup(&query.name, &query.hints).await?;
            candidates.extend(contextual);
        }
        
        // 3. Topic-based routing
        if let Some(topic) = &query.topic {
            let topic_experts = self.find_topic_experts(topic, &query.expertise_level).await?;
            candidates.extend(topic_experts);
        }
        
        // 4. Organizational lookup
        if let Some(org) = &query.organization {
            let org_members = self.find_organization_members(org, &query.role).await?;
            candidates.extend(org_members);
        }
        
        // 5. Federation lookup
        let federated = self.federated_lookup(query).await?;
        candidates.extend(federated);
        
        // 6. Apply privacy filters
        candidates = self.apply_privacy_filters(candidates, requester).await?;
        
        // 7. Calculate trust scores and network proximity
        for candidate in &mut candidates {
            candidate.trust_info = self.calculate_trust_info(&candidate.global_id, requester).await?;
            candidate.network_proximity = self.calculate_network_proximity(&candidate.global_id, requester).await?;
        }
        
        // 8. Sort by relevance, trust, and priority
        candidates.sort_by(|a, b| self.calculate_relevance_score(a, b, query, requester));
        
        Ok(candidates)
    }
    
    /// Find experts on a specific topic
    pub async fn find_topic_experts(
        &self,
        topic: &str,
        min_expertise: &Option<ExpertiseLevel>,
    ) -> Result<Vec<ContactCandidate>> {
        let mut experts = Vec::new();
        
        let subscriptions = self.registry.find_topic_subscriptions(topic).await?;
        
        for subscription in subscriptions {
            // Filter by expertise level
            if let Some(min_level) = min_expertise {
                if subscription.expertise_level < *min_level {
                    continue;
                }
            }
            
            // Check availability
            if !subscription.availability.is_available() {
                continue;
            }
            
            // Check subscription type (prefer experts over interested)
            let relevance_score = match subscription.subscription_type {
                SubscriptionType::Expert => 100,
                SubscriptionType::Interested => 60,
                SubscriptionType::Learning => 30,
                SubscriptionType::Monitoring => 20,
            };
            
            if let Ok(profile) = self.registry.get_profile(&subscription.participant_id).await {
                experts.push(ContactCandidate {
                    global_id: profile.global_id.clone(),
                    name: subscription.participant_name.clone(),
                    relevance_score,
                    contact_method: ContactMethod::Direct,
                    trust_info: TrustInfo::default(),
                    network_proximity: 0,
                    subscription_info: Some(subscription.clone()),
                });
            }
        }
        
        Ok(experts)
    }
    
    /// Calculate comprehensive trust information
    pub async fn calculate_trust_info(
        &self,
        target_id: &str,
        requester_id: &str,
    ) -> Result<TrustInfo> {
        let target_profile = self.registry.get_profile(target_id).await?;
        let requester_profile = self.registry.get_profile(requester_id).await?;
        
        // Calculate network proximity (degrees of separation)
        let network_proximity = self.trust_calculator
            .calculate_network_proximity(target_id, requester_id)
            .await?;
        
        // Get explicit trust ratings
        let trust_ratings = target_profile.trust_ratings.clone();
        
        // Calculate composite trust score
        let composite_score = self.trust_calculator.calculate_composite_trust(
            &trust_ratings,
            network_proximity,
            &requester_profile,
        )?;
        
        Ok(TrustInfo {
            network_proximity,
            direct_trust_rating: trust_ratings.entity_trust.received_ratings
                .get(requester_id)
                .map(|score| score.score),
            network_trust_score: trust_ratings.network_trust.network_score,
            composite_score,
            verification_level: target_profile.federation_config.identity_verification.verification_level,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactQuery {
    pub name: Option<String>,
    pub organization: Option<String>,
    pub department: Option<String>,
    pub role: Option<String>,
    pub topic: Option<String>,
    pub expertise_level: Option<ExpertiseLevel>,
    pub hints: Vec<ContactHint>,
    pub urgency: MessageUrgency,
    pub message_type: MessageType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactCandidate {
    pub global_id: String,
    pub name: String,
    pub relevance_score: u8,
    pub contact_method: ContactMethod,
    pub trust_info: TrustInfo,
    pub network_proximity: u32,
    pub subscription_info: Option<TopicSubscription>,
    pub organizational_context: Option<OrganizationalContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustInfo {
    pub network_proximity: u32,
    pub direct_trust_rating: Option<u8>,
    pub network_trust_score: f64,
    pub composite_score: f64,
    pub verification_level: VerificationLevel,
}
```

### 🔒 Privacy and Consent Management

```rust
pub struct PrivacyManager {
    consent_store: ConsentStore,
    privacy_policies: PolicyEngine,
    audit_log: AuditLogger,
}

impl PrivacyManager {
    /// Check if a contact attempt is allowed
    pub async fn check_contact_permission(
        &self,
        target_id: &str,
        requester_id: &str,
        contact_type: ContactType,
    ) -> Result<ContactPermission> {
        let target_profile = self.get_profile(target_id).await?;
        let requester_profile = self.get_profile(requester_id).await?;
        
        // Check discoverability level
        let discoverable = match target_profile.discovery_permissions.discoverability {
            DiscoverabilityLevel::Public => true,
            DiscoverabilityLevel::Unlisted => {
                self.has_referral_or_hint(target_id, requester_id).await?
            },
            DiscoverabilityLevel::Private => {
                self.has_explicit_permission(target_id, requester_id).await?
            },
            DiscoverabilityLevel::Stealth => {
                self.is_pre_authorized(target_id, requester_id).await?
            },
        };
        
        if !discoverable {
            return Ok(ContactPermission::Denied(
                "Target is not discoverable by this requester".to_string()
            ));
        }
        
        // Check organizational boundaries
        if let Some(org_context) = &target_profile.organizational_context {
            if !self.check_organizational_access(org_context, &requester_profile).await? {
                return Ok(ContactPermission::RequiresApproval(
                    ApprovalType::OrganizationalPolicy
                ));
            }
        }
        
        // Check trust thresholds
        let trust_info = self.calculate_trust_info(target_id, requester_id).await?;
        if trust_info.composite_score < target_profile.discovery_permissions.min_trust_score.unwrap_or(0.0) {
            return Ok(ContactPermission::RequiresApproval(
                ApprovalType::TrustThreshold
            ));
        }
        
        // Check relationship-based permissions
        if let Some(relationship) = self.get_relationship(target_id, requester_id).await? {
            return Ok(self.get_relationship_permission(&relationship, contact_type));
        }
        
        // Default permission based on profile settings
        if target_profile.discovery_permissions.require_introduction {
            Ok(ContactPermission::RequiresApproval(ApprovalType::Introduction))
        } else {
            Ok(ContactPermission::Allowed)
        }
    }
    
    /// Handle consent for data sharing
    pub async fn request_consent(
        &mut self,
        data_subject: &str,
        requester: &str,
        data_types: Vec<DataType>,
        purpose: &str,
        duration: Option<chrono::Duration>,
    ) -> Result<ConsentRequestId> {
        let consent_request = ConsentRequest {
            id: Uuid::new_v4(),
            data_subject: data_subject.to_string(),
            requester: requester.to_string(),
            data_types,
            purpose: purpose.to_string(),
            duration,
            requested_at: Utc::now(),
            expires_at: Utc::now() + chrono::Duration::days(30),
        };
        
        self.consent_store.store_request(consent_request.clone()).await?;
        
        // Send notification to data subject
        self.send_consent_notification(&consent_request).await?;
        
        Ok(consent_request.id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContactPermission {
    Allowed,
    Denied(String),
    RequiresApproval(ApprovalType),
    RequiresConsent(Vec<DataType>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ApprovalType {
    Introduction,
    TrustThreshold,
    OrganizationalPolicy,
    ManualReview,
}
```

### 🌐 Federation Implementation

```rust
pub struct FederationClient {
    http_client: reqwest::Client,
    server_registry: ServerRegistry,
    trust_manager: ServerTrustManager,
    protocol_adapters: HashMap<String, Box<dyn ProtocolAdapter>>,
}

impl FederationClient {
    /// Perform federated lookup across multiple servers
    pub async fn federated_lookup(
        &self,
        query: &ContactQuery,
        requester_server: &str,
    ) -> Result<Vec<ContactCandidate>> {
        let mut all_candidates = Vec::new();
        
        // Get list of federated servers to query
        let servers = self.server_registry.get_federation_servers(requester_server).await?;
        
        // Prepare federated query
        let fed_query = FederatedQuery {
            query: query.clone(),
            requesting_server: requester_server.to_string(),
            max_results: 50,
            privacy_level: PrivacyLevel::Standard,
        };
        
        // Query servers in parallel (with rate limiting)
        let mut query_futures = Vec::new();
        for server in servers {
            if server.trust_level >= ServerTrustLevel::Provisional {
                let future = self.query_federated_server(server, &fed_query);
                query_futures.push(future);
            }
        }
        
        // Collect results
        let results = futures::future::join_all(query_futures).await;
        for result in results {
            if let Ok(candidates) = result {
                all_candidates.extend(candidates);
            }
        }
        
        // Apply federation-specific filtering
        all_candidates = self.apply_federation_filters(all_candidates, requester_server).await?;
        
        Ok(all_candidates)
    }
    
    /// Query a specific federated server
    async fn query_federated_server(
        &self,
        server: FederatedServer,
        query: &FederatedQuery,
    ) -> Result<Vec<ContactCandidate>> {
        let url = format!("{}/api/v1/lookup", server.server_url);
        
        let response = self.http_client
            .post(&url)
            .json(query)
            .header("X-Server-ID", &server.server_id)
            .header("Authorization", &self.get_server_auth_token(&server.server_id)?)
            .send()
            .await?;
        
        if response.status().is_success() {
            let fed_response: FederatedLookupResponse = response.json().await?;
            Ok(fed_response.candidates)
        } else {
            Err(anyhow::anyhow!("Federation query failed: {}", response.status()))
        }
    }
    
    /// Cross-protocol contact resolution
    pub async fn resolve_cross_protocol(
        &self,
        address: &str,
        protocol: &str,
    ) -> Result<Option<String>> {
        if let Some(adapter) = self.protocol_adapters.get(protocol) {
            adapter.resolve_to_emrp(address).await
        } else {
            Ok(None)
        }
    }
}

/// Protocol adapter for cross-protocol compatibility
#[async_trait::async_trait]
pub trait ProtocolAdapter: Send + Sync {
    async fn resolve_to_emrp(&self, external_address: &str) -> Result<Option<String>>;
    async fn resolve_from_emrp(&self, emrp_id: &str) -> Result<Option<String>>;
    fn supported_protocols(&self) -> Vec<String>;
}

/// Email protocol adapter
pub struct EmailProtocolAdapter {
    dns_resolver: DnsResolver,
    mx_cache: Cache<String, Vec<MxRecord>>,
}

#[async_trait::async_trait]
impl ProtocolAdapter for EmailProtocolAdapter {
    async fn resolve_to_emrp(&self, email_address: &str) -> Result<Option<String>> {
        // Check if the email domain supports EMRP
        let domain = email_address.split('@').nth(1).ok_or_else(|| {
            anyhow::anyhow!("Invalid email address format")
        })?;
        
        // Look for EMRP TXT record
        if let Ok(txt_records) = self.dns_resolver.lookup_txt(domain).await {
            for record in txt_records {
                if record.starts_with("emrp=") {
                    let emrp_server = record.strip_prefix("emrp=").unwrap();
                    // Query EMRP server for this email address
                    return self.query_emrp_server(emrp_server, email_address).await;
                }
            }
        }
        
        Ok(None)
    }
    
    async fn resolve_from_emrp(&self, emrp_id: &str) -> Result<Option<String>> {
        // Implementation to find email address for EMRP ID
        todo!("Implement EMRP to email resolution")
    }
    
    fn supported_protocols(&self) -> Vec<String> {
        vec!["email".to_string(), "smtp".to_string()]
    }
}
```

### 📊 Usage Examples

#### Example 1: Topic-Based Expert Finding

```rust
// Find climate modeling experts for collaboration
let query = ContactQuery {
    name: None,
    organization: None,
    department: None,
    role: None,
    topic: Some("climate modeling".to_string()),
    expertise_level: Some(ExpertiseLevel::Advanced),
    hints: vec![
        ContactHint::Geographic("North America".to_string()),
        ContactHint::Language("English".to_string()),
    ],
    urgency: MessageUrgency::Normal,
    message_type: MessageType::Collaboration,
};

let candidates = resolver.resolve_contact(&query, "researcher@university.edu").await?;

for candidate in candidates {
    println!("Found expert: {} (Trust: {:.1}, Proximity: {} hops)", 
             candidate.name, 
             candidate.trust_info.composite_score,
             candidate.network_proximity);
}
```

#### Example 2: Priority-Based Routing

```rust
// Boss sending urgent message to team
let relationship = Relationship {
    with_entity: "employee@company.com".to_string(),
    relationship_type: RelationshipType::DirectReport,
    priority_level: PriorityLevel::High,
    established_at: Utc::now(),
    interaction_frequency: InteractionFrequency::Daily,
    mutual: true,
};

// This message will bypass normal filtering and get immediate delivery
let message = Message {
    from: "boss@company.com".to_string(),
    to: "employee@company.com".to_string(),
    subject: "URGENT: Production issue".to_string(),
    priority: PriorityLevel::Critical,
    keywords: vec!["urgent".to_string(), "production".to_string()],
    ..Default::default()
};
```

#### Example 3: Cross-Protocol Integration

```rust
// Email user finding EMRP participants
let email_adapter = EmailProtocolAdapter::new();
let emrp_id = email_adapter.resolve_to_emrp("researcher@stanford.edu").await?;

if let Some(emrp_id) = emrp_id {
    // Now use EMRP for richer communication
    let profile = registry.get_profile(&emrp_id).await?;
    println!("Found EMRP profile for email user: {}", profile.identities[0].name);
}
```

#### Example 4: Organizational Boundary Enforcement

```rust
// Internal company directory with external partner access
let org_context = OrganizationalContext {
    organization_id: "acme-corp".to_string(),
    organization_name: "Acme Corporation".to_string(),
    department: Some("Engineering".to_string()),
    team: Some("Backend Team".to_string()),
    role: "Senior Developer".to_string(),
    access_level: AccessLevel::Internal,
    internal_directory: true,
    cross_org_permissions: CrossOrgPermissions {
        allow_external_contact: true,
        external_contact_approval: ApprovalLevel::ManagerApproval,
        partner_organizations: vec!["partner-corp".to_string()],
        blocked_organizations: vec!["competitor-corp".to_string()],
    },
};

// Partner can contact with manager approval
// Competitor contact is blocked
// Internal team members have direct access
```

### 🚀 Implementation Roadmap

#### Phase 1: Core Registry (Weeks 1-4)
- [ ] Basic participant profiles and discovery
- [ ] Simple trust/reputation system
- [ ] Privacy controls and discoverability levels
- [ ] Local federation setup

#### Phase 2: Advanced Features (Weeks 5-8)
- [ ] Topic-based routing and expert finding
- [ ] Relationship and priority systems
- [ ] Organizational boundaries
- [ ] Cross-protocol adapters (email, Matrix)

#### Phase 3: Production Features (Weeks 9-12)
- [ ] Full federation implementation
- [ ] OAuth 2.0 identity verification
- [ ] Data sovereignty and compliance
- [ ] Performance optimization and caching

#### Phase 4: Ecosystem Integration (Weeks 13-16)
- [ ] Additional protocol adapters (Slack, Discord, Teams)
- [ ] AI-powered suggestion engine
- [ ] Analytics and monitoring
- [ ] Mobile and web interfaces

### 🔧 Technical Considerations

#### Performance
- **Caching**: Aggressive caching of frequently accessed profiles and trust calculations
- **Indexing**: Elasticsearch/Lucene for fast text search across profiles
- **Federation**: Connection pooling and request batching for federated queries
- **Background Processing**: Async processing for trust calculations and relationship updates

#### Security
- **Encryption**: End-to-end encryption for sensitive profile data
- **Authentication**: mTLS for server-to-server federation
- **Authorization**: Fine-grained permissions with audit trails
- **Rate Limiting**: Prevent abuse of discovery and contact systems

#### Scalability
- **Horizontal Scaling**: Microservices architecture with load balancing
- **Database Sharding**: Partition profiles by geographic region or organization
- **CDN**: Cache public profile data at edge locations
- **Event Streaming**: Apache Kafka for real-time updates across federation

This enhanced identity resolution system transforms EMRP from a simple peer-to-peer protocol into a sophisticated, privacy-respecting communication platform that can compete with and integrate with existing messaging ecosystems while providing unique capabilities for AI and human collaboration.
