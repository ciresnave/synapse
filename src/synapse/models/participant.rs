// Synapse Participant Registry - Core Data Models

use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use super::trust::TrustRatings;

/// Core participant profile in the Synapse network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantProfile {
    // Core identity
    pub global_id: String,              // "alice.work@ai-lab.com"
    pub display_name: String,           // "Alice Smith (Work)"
    pub entity_type: EntityType,        // Human, AiModel, Service
    pub identities: Vec<IdentityContext>,

    // Discovery and privacy
    pub discovery_permissions: DiscoveryPermissions,
    pub availability: AvailabilityStatus,
    pub contact_preferences: ContactPreferences,

    // Trust and relationships
    pub trust_ratings: TrustRatings,
    pub relationships: Vec<Relationship>,

    // Capabilities and interests
    pub topic_subscriptions: Vec<TopicSubscription>,
    pub organizational_context: Option<OrganizationalContext>,

    // Technical details
    pub public_key: Option<Vec<u8>>,
    pub supported_protocols: Vec<String>,
    pub last_seen: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EntityType {
    Human,
    AiModel,
    Service,
    Bot,
    Organization,
    Department,
}

impl FromStr for EntityType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "human" => Ok(EntityType::Human),
            "ai_model" => Ok(EntityType::AiModel),
            "service" => Ok(EntityType::Service),
            "bot" => Ok(EntityType::Bot),
            "organization" => Ok(EntityType::Organization),
            "department" => Ok(EntityType::Department),
            _ => Err(format!("Unknown entity type: {}", s)),
        }
    }
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
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IdentityType {
    Personal,
    Professional,
    Service,
    Research,
}

/// Privacy and discoverability controls
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryPermissions {
    pub discoverability: DiscoverabilityLevel,
    pub searchable_fields: Vec<String>,
    pub require_introduction: bool,
    pub min_trust_score: Option<f64>,
    pub min_network_score: Option<f64>,
    pub allowed_domains: Vec<String>,
    pub blocked_domains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiscoverabilityLevel {
    /// Anyone can discover, appears in searches
    Public,
    /// Discoverable through referrals/hints but not in general searches  
    Unlisted,
    /// Not discoverable, direct contact only (requires exact contact info)
    Private,
    /// Completely invisible, even direct attempts fail unless pre-authorized
    Stealth,
}

/// Universal availability status (not just for experts)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailabilityStatus {
    pub status: Status,
    pub status_message: Option<String>,
    pub available_hours: Option<BusinessHours>,
    pub time_zone: String,
    pub last_updated: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Status {
    Available,      // Open to contact
    Busy,          // Limited availability
    DoNotDisturb,  // No unsolicited contact
    Away,          // Temporarily unavailable
    Offline,       // Completely unavailable
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessHours {
    pub monday: Option<TimeRange>,
    pub tuesday: Option<TimeRange>,
    pub wednesday: Option<TimeRange>,
    pub thursday: Option<TimeRange>,
    pub friday: Option<TimeRange>,
    pub saturday: Option<TimeRange>,
    pub sunday: Option<TimeRange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: String, // "09:00"
    pub end: String,   // "17:00"
}

/// Contact preferences for filtering incoming requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactPreferences {
    pub accepts_unsolicited_contact: bool,
    pub requires_introduction: bool,
    pub preferred_contact_method: ContactMethod,
    pub rate_limits: RateLimits,
    pub filtering: ContactFiltering,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContactMethod {
    Direct,
    IntroductionRequired,
    PublicMessage,
    ScheduleOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimits {
    pub max_contacts_per_hour: Option<u32>,
    pub max_contacts_per_day: Option<u32>,
    pub cooldown_period: Option<chrono::Duration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactFiltering {
    pub min_trust_score: Option<f64>,
    pub allowed_entity_types: Vec<EntityType>,
    pub preferred_organizations: Vec<String>,
    pub blocked_participants: Vec<String>,
    pub emergency_keywords: Vec<String>, // Always get through
}

/// Relationship between participants
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    pub with_participant: String,
    pub relationship_type: RelationshipType,
    pub priority_level: PriorityLevel,
    pub established_at: DateTime<Utc>,
    pub mutual: bool,
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
    High,       // Gets through during available hours, prioritized
    Normal,     // Standard priority
    Low,        // Can be delayed, batched
    Background, // Minimal priority, batch processing
}

/// Topic subscription for expertise and interests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicSubscription {
    pub topic: String,
    pub subscription_type: SubscriptionType,
    pub expertise_level: ExpertiseLevel,
    pub contact_preferences: Option<TopicContactPreferences>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubscriptionType {
    Expert,      // Can help with this topic
    Interested,  // Want to know about this topic
    Learning,    // Learning about this topic
    Monitoring,  // Track this for awareness
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExpertiseLevel {
    Beginner,
    Intermediate,
    Advanced,
    Expert,
    Authority,
}

/// Topic-specific contact preferences (for experts)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicContactPreferences {
    pub accepts_questions: bool,
    pub accepts_collaboration: bool,
    pub accepts_mentoring: bool,
    pub max_complexity: QuestionComplexity,
    pub requires_research_shown: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuestionComplexity {
    Beginner,
    Intermediate,
    Advanced,
    Research,
}

/// Organizational context and boundaries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationalContext {
    pub organization_id: String,
    pub organization_name: String,
    pub department: Option<String>,
    pub team: Option<String>,
    pub role: String,
    pub access_level: AccessLevel,
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
    pub partner_organizations: Vec<String>,
    pub blocked_organizations: Vec<String>,
}

impl ParticipantProfile {
    pub fn new(global_id: String, display_name: String, entity_type: EntityType) -> Self {
        Self {
            global_id,
            display_name,
            entity_type,
            identities: Vec::new(),
            discovery_permissions: DiscoveryPermissions::default(),
            availability: AvailabilityStatus::default(),
            contact_preferences: ContactPreferences::default(),
            trust_ratings: TrustRatings::default(),
            relationships: Vec::new(),
            topic_subscriptions: Vec::new(),
            organizational_context: None,
            public_key: None,
            supported_protocols: vec!["synapse-v1".to_string()],
            last_seen: Utc::now(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /// Check if this participant can be discovered by another participant
    pub fn can_be_discovered_by(&self, requester: &ParticipantProfile) -> bool {
        match self.discovery_permissions.discoverability {
            DiscoverabilityLevel::Public => true,
            DiscoverabilityLevel::Unlisted => {
                // Unlisted requires some connection or hint
                self.has_relationship_with(&requester.global_id) ||
                self.shares_organization_with(requester)
            }
            DiscoverabilityLevel::Private => {
                // Private requires explicit permission or direct relationship
                self.has_relationship_with(&requester.global_id)
            }
            DiscoverabilityLevel::Stealth => {
                // Stealth requires pre-authorization
                false // TODO: Implement pre-authorization system
            }
        }
    }

    pub fn has_relationship_with(&self, participant_id: &str) -> bool {
        self.relationships.iter()
            .any(|rel| rel.with_participant == participant_id)
    }

    pub fn shares_organization_with(&self, other: &ParticipantProfile) -> bool {
        if let (Some(self_org), Some(other_org)) = (
            &self.organizational_context,
            &other.organizational_context
        ) {
            self_org.organization_id == other_org.organization_id
        } else {
            false
        }
    }

    pub fn is_available_for_contact(&self) -> bool {
        matches!(self.availability.status, Status::Available | Status::Busy)
    }
}

impl Default for DiscoveryPermissions {
    fn default() -> Self {
        Self {
            discoverability: DiscoverabilityLevel::Unlisted,
            searchable_fields: vec!["name".to_string(), "organization".to_string()],
            require_introduction: false,
            min_trust_score: None,
            min_network_score: None,
            allowed_domains: Vec::new(),
            blocked_domains: Vec::new(),
        }
    }
}

impl Default for AvailabilityStatus {
    fn default() -> Self {
        Self {
            status: Status::Available,
            status_message: None,
            available_hours: None,
            time_zone: "UTC".to_string(),
            last_updated: Utc::now(),
        }
    }
}

impl Default for ContactPreferences {
    fn default() -> Self {
        Self {
            accepts_unsolicited_contact: true,
            requires_introduction: false,
            preferred_contact_method: ContactMethod::Direct,
            rate_limits: RateLimits::default(),
            filtering: ContactFiltering::default(),
        }
    }
}

impl Default for RateLimits {
    fn default() -> Self {
        Self {
            max_contacts_per_hour: Some(10),
            max_contacts_per_day: Some(50),
            cooldown_period: Some(chrono::Duration::minutes(5)),
        }
    }
}

impl Default for ContactFiltering {
    fn default() -> Self {
        Self {
            min_trust_score: None,
            allowed_entity_types: Vec::new(), // Empty = allow all
            preferred_organizations: Vec::new(),
            blocked_participants: Vec::new(),
            emergency_keywords: vec!["urgent".to_string(), "emergency".to_string()],
        }
    }
}
