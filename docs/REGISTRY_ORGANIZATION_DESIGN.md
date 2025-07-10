# 🗂️ EMRP Participant Registry Design

> **Comprehensive design for organizing, discovering, and managing EMRP participants with privacy, delegation, and context awareness**

## 🎯 **Core Design Principles**

1. **Context-Aware Identities**: Same person can have multiple identities for different contexts
2. **Privacy by Design**: Granular control over discoverability and information sharing
3. **Flexible Delegation**: Smart forwarding and delegation with conditions
4. **Consent-First**: All interactions require appropriate permissions
5. **Abuse-Resistant**: Rate limiting, blocking, and moderation capabilities

## 🏗️ **Registry Data Structure**

### **Core Participant Profile**

```rust
pub struct ParticipantProfile {
    // Identity
    pub global_id: String,                    // "alice.work@ai-lab.com"
    pub display_name: String,                 // "Alice Smith (Work)"
    pub entity_type: EntityType,              // Human, AiModel, Service
    pub identity_context: IdentityContext,    // Work, Personal, Research
    pub linked_identities: Vec<String>,       // Other IDs for same person (optional)
    
    // Organization and capabilities
    pub organization: Option<String>,         // "AI Lab"
    pub role: Option<String>,                // "Senior Research Scientist"
    pub capabilities: Vec<String>,            // ["machine_learning", "computer_vision"]
    pub specialties: Vec<String>,            // ["vision_transformers", "medical_imaging"]
    
    // Discovery and privacy controls
    pub discovery_permissions: DiscoveryPermissions,
    pub contact_preferences: ContactPreferences,
    pub forwarding_rules: Vec<ForwardingRule>,
    pub moderation_settings: ModerationSettings,
    
    // Availability and scheduling
    pub availability: AvailabilityRules,
    pub timezone: String,                     // "America/New_York"
    pub preferred_languages: Vec<String>,     // ["en", "es"]
    
    // Technical details
    pub public_key: Option<PublicKey>,
    pub supported_protocols: Vec<String>,     // ["emrp-v1", "email"]
    pub last_seen: DateTime<Utc>,
    pub registration_date: DateTime<Utc>,
    pub trust_score: Option<f64>,            // Reputation/trust metric
}
```

### **Identity Context System**

```rust
pub enum IdentityContext {
    Professional {
        organization: String,
        department: Option<String>,
        role: String,
        business_hours: BusinessHours,
        work_contact_policy: WorkContactPolicy,
    },
    Personal {
        casual_contact_ok: bool,
        family_friendly: bool,
        hobby_interests: Vec<String>,
    },
    Research {
        institution: String,
        research_areas: Vec<String>,
        collaboration_openness: CollaborationLevel,
        publication_sharing: bool,
    },
    Service {
        service_type: String,
        automation_level: AutomationLevel,
        service_hours: ServiceHours,
        escalation_contacts: Vec<String>,
    },
    Community {
        community_name: String,
        role_in_community: String,
        community_guidelines: Vec<String>,
    },
}

pub enum WorkContactPolicy {
    WorkHoursOnly,
    UrgentAnytime,
    EmergencyOnly,
    AlwaysAvailable,
}

pub enum CollaborationLevel {
    OpenToAll,
    SameInstitution,
    InviteOnly,
    NotAccepting,
}
```

## 🔒 **Privacy and Discovery Controls**

### **Granular Discovery Permissions**

```rust
pub struct DiscoveryPermissions {
    pub discoverable: DiscoverabilityLevel,
    pub discovery_rules: Vec<DiscoveryRule>,
    pub search_visibility: SearchVisibility,
    pub profile_sharing: ProfileSharingLevel,
}

pub enum DiscoverabilityLevel {
    Public,          // Anyone can discover and see basic info
    Restricted,      // Only approved entities can discover
    Private,         // Not discoverable, direct contact only
    Unlisted,        // Discoverable if you know specific search terms
    Stealth,         // Completely hidden, invitation-only
}

pub struct DiscoveryRule {
    pub matcher: EntityMatcher,
    pub allowed_info: ProfileInfoLevel,
    pub contact_permissions: Vec<Permission>,
    pub auto_approve: bool,
}

pub enum EntityMatcher {
    Domain(String),                      // "@company.com"
    Organization(String),                // "AI Lab"
    SpecificEntity(String),             // "bob@company.com"
    EntityType(EntityType),             // All AI models
    Role(String),                       // "Research Scientist"
    TrustNetwork { degrees: u32 },      // Friends of friends (2 degrees)
    Capability(String),                 // "machine_learning"
    Geographic(String),                 // "San Francisco Bay Area"
}

pub enum ProfileInfoLevel {
    Minimal,         // Just name and "available for contact"
    Basic,           // + organization, role, general availability
    Standard,        // + capabilities, specialties, contact preferences  
    Detailed,        // + research interests, collaboration info
    Full,            // All public information
}
```

### **Contact Preferences and Filtering**

```rust
pub struct ContactPreferences {
    pub preferred_contact_methods: Vec<ContactMethod>,
    pub message_filtering: MessageFiltering,
    pub response_expectations: ResponseExpectations,
    pub introduction_requirements: IntroductionRequirements,
}

pub struct MessageFiltering {
    pub urgency_filter: UrgencyFilter,
    pub content_filters: Vec<ContentFilter>,
    pub sender_reputation_threshold: Option<f64>,
    pub require_purpose_statement: bool,
}

pub enum UrgencyFilter {
    AcceptAll,
    HighUrgencyOnly,
    WorkingHoursOnly,
    EmergencyOnly,
    Custom(UrgencyRule),
}

pub struct ContentFilter {
    pub filter_type: FilterType,
    pub keywords: Vec<String>,
    pub action: FilterAction,
}

pub enum FilterType {
    Block,           // Block messages containing these keywords
    Require,         // Require messages to contain these keywords
    Priority,        // Prioritize messages with these keywords
    Flag,            // Flag for manual review
}
```

## 🔄 **Delegation and Forwarding System**

### **Smart Forwarding Rules**

```rust
pub struct ForwardingRule {
    pub id: String,
    pub name: String,                       // "Vacation Delegation to Bob"
    pub from_identity: String,              // "alice.work@ai-lab.com"
    pub active: bool,
    
    // Forwarding targets and conditions
    pub forwarding_targets: Vec<ForwardingTarget>,
    pub conditions: ForwardingConditions,
    pub time_constraints: TimeConstraints,
    
    // Forwarding behavior
    pub forward_type: ForwardType,
    pub notification_settings: NotificationSettings,
    pub expiry: Option<DateTime<Utc>>,
}

pub struct ForwardingTarget {
    pub target_identity: String,            // "bob.work@ai-lab.com"
    pub target_role: ForwardingRole,        // Primary, Backup, Specialist
    pub conditions: Option<ForwardingConditions>,
}

pub enum ForwardingRole {
    Primary,        // Main delegate
    Backup,         // If primary is unavailable
    Specialist,     // For specific types of messages
    Supervisor,     // For escalation
    Team,           // Broadcast to team
}

pub struct ForwardingConditions {
    pub message_urgency: Option<UrgencyCondition>,
    pub sender_criteria: Option<SenderCriteria>,
    pub content_criteria: Option<ContentCriteria>,
    pub time_criteria: Option<TimeCriteria>,
}

pub enum UrgencyCondition {
    MinimumUrgency(MessageUrgency),
    UrgencyRange(MessageUrgency, MessageUrgency),
    ExactUrgency(MessageUrgency),
}

pub enum ForwardType {
    // Copy to delegate, keep original recipient
    Copy {
        include_original_recipient: bool,
        add_forwarding_note: bool,
    },
    
    // Redirect to delegate, remove original recipient  
    Redirect {
        notify_original: bool,
        forwarding_message: Option<String>,
    },
    
    // Auto-reply first, then optionally forward
    AutoReplyThenForward {
        auto_reply_message: String,
        forward_after_reply: bool,
        delay_forwarding: Option<Duration>,
    },
    
    // Conditional forwarding based on response
    ConditionalForward {
        conditions: Vec<ConditionalRule>,
        default_action: ForwardAction,
    },
}
```

### **Cross-Context Forwarding**

```rust
// Example: Forward urgent personal messages to work account
ForwardingRule {
    name: "Urgent Personal to Work".to_string(),
    from_identity: "alice@home.com".to_string(),
    forwarding_targets: vec![
        ForwardingTarget {
            target_identity: "alice.work@ai-lab.com".to_string(),
            target_role: ForwardingRole::Primary,
            conditions: None,
        }
    ],
    conditions: ForwardingConditions {
        message_urgency: Some(UrgencyCondition::MinimumUrgency(MessageUrgency::High)),
        time_criteria: Some(TimeCriteria::BusinessHours),
        // ...
    },
    forward_type: ForwardType::Copy {
        include_original_recipient: false,
        add_forwarding_note: true,
    },
}

// Example: Work vacation delegation
ForwardingRule {
    name: "Vacation Coverage".to_string(),
    from_identity: "alice.work@ai-lab.com".to_string(),
    forwarding_targets: vec![
        ForwardingTarget {
            target_identity: "bob.work@ai-lab.com".to_string(),
            target_role: ForwardingRole::Primary,
            conditions: None,
        },
        ForwardingTarget {
            target_identity: "team-lead@ai-lab.com".to_string(),
            target_role: ForwardingRole::Backup,
            conditions: Some(ForwardingConditions {
                message_urgency: Some(UrgencyCondition::MinimumUrgency(MessageUrgency::Emergency)),
                // ...
            }),
        }
    ],
    forward_type: ForwardType::AutoReplyThenForward {
        auto_reply_message: "I'm on vacation until Dec 15. For urgent matters, your message has been forwarded to Bob.".to_string(),
        forward_after_reply: true,
        delay_forwarding: None,
    },
}
```

## 🛡️ **Moderation and Abuse Prevention**

### **Blocking and Rate Limiting**

```rust
pub struct ModerationSettings {
    pub blocked_entities: Vec<String>,           // Specific blocked global IDs
    pub blocked_domains: Vec<String>,            // Blocked domains
    pub blocked_keywords: Vec<String>,           // Content-based blocking
    pub rate_limits: RateLimitSettings,
    pub spam_detection: SpamDetectionSettings,
    pub escalation_contacts: Vec<String>,        // For abuse reports
}

pub struct RateLimitSettings {
    pub max_contact_requests_per_hour: u32,
    pub max_messages_per_day: u32,
    pub new_contact_cooldown: Duration,          // Time between new contact attempts
    pub burst_protection: BurstProtection,
}

pub struct SpamDetectionSettings {
    pub enabled: bool,
    pub suspicious_patterns: Vec<String>,        // Regex patterns for spam
    pub auto_quarantine_threshold: f64,         // Confidence threshold for auto-quarantine
    pub require_human_review: bool,
}
```

### **Reputation and Trust System**

```rust
pub struct TrustMetrics {
    pub trust_score: f64,                       // 0.0 to 1.0
    pub successful_interactions: u32,
    pub failed_interactions: u32,
    pub spam_reports: u32,
    pub endorsements: Vec<Endorsement>,
    pub verification_status: VerificationStatus,
}

pub struct Endorsement {
    pub from_entity: String,
    pub endorsement_type: EndorsementType,
    pub timestamp: DateTime<Utc>,
    pub comment: Option<String>,
}

pub enum EndorsementType {
    Professional,    // "Good colleague"
    Technical,       // "Knowledgeable in ML"
    Communication,   // "Clear communicator"
    Trustworthy,     // "Reliable and honest"
}

pub enum VerificationStatus {
    Unverified,
    EmailVerified,
    OrganizationVerified,
    IdentityVerified,
    CommunityVerified,
}
```

## 🔍 **Search and Discovery Implementation**

### **Multi-Dimensional Search**

```rust
pub struct ParticipantQuery {
    // Basic search
    pub name: Option<String>,
    pub organization: Option<String>,
    pub role: Option<String>,
    
    // Capability-based search
    pub required_capabilities: Vec<String>,
    pub preferred_capabilities: Vec<String>,
    
    // Context and availability
    pub entity_type: Option<EntityType>,
    pub available_now: Option<bool>,
    pub timezone_compatible: Option<String>,
    
    // Geographic and network
    pub geographic_region: Option<String>,
    pub network_distance: Option<u32>,        // Degrees of separation
    
    // Trust and reputation
    pub minimum_trust_score: Option<f64>,
    pub verified_only: bool,
    
    // Search modifiers
    pub fuzzy_matching: bool,
    pub include_unlisted: bool,
    pub search_linked_identities: bool,
}

pub struct SearchResult {
    pub profile: ParticipantProfile,
    pub match_score: f64,                     // 0.0 to 1.0
    pub match_reasons: Vec<MatchReason>,
    pub available_info: ProfileInfoLevel,     // Based on discovery permissions
    pub contact_path: ContactPath,            // How to reach this person
}

pub enum MatchReason {
    ExactNameMatch,
    FuzzyNameMatch(f64),
    OrganizationMatch,
    CapabilityMatch(String),
    NetworkConnection(u32),                   // Degrees of separation
    GeographicProximity,
    RoleMatch,
}
```

## 📋 **Key Implementation Considerations**

### **1. Data Consistency and Sync**
- **Registry Federation**: Multiple registry nodes with eventual consistency
- **Cache Invalidation**: Smart caching with TTL and update notifications
- **Conflict Resolution**: Last-writer-wins with timestamp ordering

### **2. Privacy Compliance**
- **GDPR Compliance**: Right to be forgotten, data portability
- **Consent Management**: Explicit consent for each type of data sharing
- **Audit Logging**: Complete audit trail of all discovery requests

### **3. Performance and Scalability**
- **Distributed Architecture**: Shard by geographic region or organization
- **Search Optimization**: Elasticsearch or similar for complex queries
- **Rate Limiting**: Distributed rate limiting across registry nodes

### **4. Security**
- **Authentication**: Strong authentication for registry updates
- **Authorization**: Fine-grained permissions for different operations
- **Encryption**: All data encrypted at rest and in transit

## 🚀 **Implementation Phases**

### **Phase 1: Basic Registry (2 weeks)**
- Core data structures and basic CRUD operations
- Simple discovery by name and organization
- Basic privacy controls (public/private)

### **Phase 2: Advanced Discovery (2 weeks)**
- Multi-dimensional search with capabilities and context
- Fuzzy matching and smart suggestions
- Trust scoring and reputation system

### **Phase 3: Delegation and Forwarding (2 weeks)**
- Forwarding rules and conditional logic
- Cross-context forwarding
- Auto-reply and delegation workflows

### **Phase 4: Advanced Privacy and Moderation (2 weeks)**
- Granular discovery permissions
- Advanced blocking and spam detection
- Compliance and audit features

This design provides a robust foundation for organizing EMRP participants while maintaining privacy, enabling natural discovery, and supporting complex real-world use cases like delegation and cross-context communication.
