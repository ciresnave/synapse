// Synapse Trust System - Models for dual trust architecture

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use dashmap::DashMap;

/// Comprehensive trust information for a participant
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrustRatings {
    // Entity-to-entity trust (subjective, personal experience)
    pub entity_trust: EntityTrustRatings,
    
    // Entity-to-network trust (objective, blockchain-verified)
    pub network_trust: NetworkTrustRating,
    
    // Network proximity (degrees of separation)
    pub network_proximity: NetworkProximity,
    
    // Identity verification status
    pub identity_verification: IdentityVerification,
}

/// Direct trust ratings between participants (subjective)
#[derive(Debug, Clone)]
pub struct EntityTrustRatings {
    /// Trust ratings received from other participants
    pub received_ratings: DashMap<String, DirectTrustScore>,
    
    /// Trust ratings given to other participants  
    pub given_ratings: DashMap<String, DirectTrustScore>,
    
    /// Aggregated statistics
    pub average_received: f64,
    pub total_ratings_received: u32,
    pub last_updated: DateTime<Utc>,
}

impl Default for EntityTrustRatings {
    fn default() -> Self {
        Self {
            received_ratings: DashMap::new(),
            given_ratings: DashMap::new(),
            average_received: 0.0,
            total_ratings_received: 0,
            last_updated: Utc::now(),
        }
    }
}

/// A direct trust rating between two participants
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectTrustScore {
    pub score: u8, // 0-100
    pub category: TrustCategory,
    pub given_by: String,
    pub given_at: DateTime<Utc>,
    pub comment: Option<String>,
    pub relationship_context: Option<super::participant::RelationshipType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TrustCategory {
    Communication,  // Clear, respectful, responsive
    Technical,      // Competent, knowledgeable, helpful
    Collaboration,  // Good team player, reliable
    Reliability,    // Follows through on commitments
    Privacy,        // Respects confidentiality and data
    Overall,        // General trustworthiness
}

/// Network-wide trust rating (objective, blockchain-verified)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkTrustRating {
    /// Current network trust score (0-100)
    pub network_score: f64,
    
    /// Trust point balance for blockchain staking
    pub trust_balance: TrustBalance,
    
    /// Participation metrics
    pub participation_metrics: ParticipationMetrics,
    
    /// Recent verifiable actions
    pub recent_actions: Vec<VerifiableActionSummary>,
    
    /// Last score calculation
    pub last_calculated: DateTime<Utc>,
}

/// Trust point balance for staking system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustBalance {
    pub participant_id: String,
    pub total_points: u32,
    pub available_points: u32, // Not currently staked
    pub staked_points: u32,
    pub earned_lifetime: u32,
    pub last_activity: DateTime<Utc>,
    pub decay_rate: f64, // Percentage decay per month (default 2%)
}

/// Participation metrics for network trust calculation
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ParticipationMetrics {
    // Communication metrics
    pub messages_sent: u64,
    pub messages_received: u64,
    pub response_rate: f64, // Percentage of messages that got responses
    
    // Collaboration metrics
    pub successful_collaborations: u64,
    pub knowledge_contributions: u64,
    pub helpful_responses_given: u64,
    
    // Trust system participation
    pub trust_reports_submitted: u64,
    pub trust_reports_validated: u64,
    pub trust_stakes_won: u64,
    pub trust_stakes_lost: u64,
    
    // Negative metrics
    pub reports_against: u64,        // Times reported for bad behavior
    pub reports_validated_against: u64, // Times reports were confirmed
    pub spam_reports: u64,
    
    // Account metrics
    pub account_age: chrono::Duration,
    pub days_active: u64,
    pub last_active: DateTime<Utc>,
}

/// Summary of a verifiable action (from blockchain)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiableActionSummary {
    pub action_id: String,
    pub action_type: ActionType,
    pub impact_score: i32,
    pub timestamp: DateTime<Utc>,
    pub blockchain_hash: String,
    pub consensus_score: f64, // How strong the consensus was
}

/// Types of actions that can be recorded on blockchain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActionType {
    // Positive actions (increase network trust)
    HelpfulResponse,      // Provided useful assistance
    KnowledgeSharing,     // Shared valuable information
    BugReport,           // Reported and helped fix issues
    Mentoring,           // Helped others learn
    Collaboration,       // Successful project collaboration
    TrustValidation,     // Successfully validated a trust report
    
    // Negative actions (decrease network trust)
    Spam,                // Sent unsolicited/irrelevant messages
    Harassment,          // Inappropriate behavior
    Misinformation,      // Spread false information
    BadFaith,            // Failed to honor commitments
    Abuse,               // Misused the system
    FalseReport,         // Filed false trust report
}

/// Network proximity calculations
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkProximity {
    /// Degrees of separation to other participants
    pub degrees_of_separation: HashMap<String, u32>,
    
    /// Connection paths through the network
    pub connection_paths: HashMap<String, Vec<String>>,
    
    /// Trust propagation scores (trust through connections)
    pub propagated_trust: HashMap<String, f64>,
    
    /// Last calculation timestamp
    pub last_calculated: DateTime<Utc>,
}

/// Identity verification information
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IdentityVerification {
    pub verification_level: VerificationLevel,
    pub verification_method: Option<VerificationMethod>,
    pub verified_at: Option<DateTime<Utc>>,
    pub verified_by: Option<String>, // Verifier participant ID
    pub verification_expires: Option<DateTime<Utc>>,
    pub verified_claims: Vec<VerifiedClaim>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VerificationLevel {
    Unverified,   // No verification
    Basic,        // Basic email/domain verification
    Enhanced,     // OAuth2 or corporate directory
    Trusted,      // Verified by trusted participants
    Authoritative, // Official organization verification
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VerificationMethod {
    EmailVerification,
    DomainOwnership,
    OAuth2(String), // Provider name
    CorporateDirectory,
    PeerVerification,
    CryptographicProof,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiedClaim {
    pub claim_type: ClaimType,
    pub claim_value: String,
    pub verified_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClaimType {
    EmailAddress,
    DomainOwnership,
    OrganizationMembership,
    ProfessionalRole,
    EducationalCredential,
    Expertise,
}

/// Composite trust information for decision making
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositeTrustScore {
    pub composite_score: f64,
    pub direct_trust: Option<f64>,
    pub network_trust: f64,
    pub network_proximity: u32,
    pub verification_bonus: f64,
    pub components: TrustComponents,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustComponents {
    pub personal_experience: f64,    // Weight: 40%
    pub network_reputation: f64,     // Weight: 30%
    pub network_proximity: f64,      // Weight: 20%
    pub identity_verification: f64,  // Weight: 10%
}

impl TrustBalance {
    pub fn new(participant_id: String) -> Self {
        Self {
            participant_id,
            total_points: 0,
            available_points: 0,
            staked_points: 0,
            earned_lifetime: 0,
            last_activity: Utc::now(),
            decay_rate: 0.02, // 2% per month
        }
    }
    
    /// Award trust points for positive actions
    pub fn award_points(&mut self, points: u32, reason: &str) {
        self.total_points += points;
        self.available_points += points;
        self.earned_lifetime += points;
        self.last_activity = Utc::now();
        
        tracing::info!(
            participant_id = %self.participant_id,
            points = points,
            reason = reason,
            new_total = self.total_points,
            "Trust points awarded"
        );
    }
    
    /// Stake trust points (lock them for blockchain voting)
    pub fn stake_points(&mut self, amount: u32) -> Result<(), String> {
        if self.available_points < amount {
            return Err("Insufficient available trust points".to_string());
        }
        
        self.available_points -= amount;
        self.staked_points += amount;
        Ok(())
    }
    
    /// Return staked points (after blockchain consensus)
    pub fn return_stake(&mut self, amount: u32) {
        self.staked_points = self.staked_points.saturating_sub(amount);
        self.available_points += amount;
    }
    
    /// Apply time-based decay to prevent hoarding
    pub fn apply_decay(&mut self) -> u32 {
        let now = Utc::now();
        let days_since_activity = (now - self.last_activity).num_days();
        
        if days_since_activity > 30 {
            let months_elapsed = days_since_activity as f64 / 30.0;
            let decay_factor = (1.0 - self.decay_rate).powf(months_elapsed);
            
            let new_total = (self.total_points as f64 * decay_factor) as u32;
            let decay_amount = self.total_points - new_total;
            
            self.total_points = new_total;
            self.available_points = self.available_points.saturating_sub(decay_amount);
            
            tracing::info!(
                participant_id = %self.participant_id,
                decay_amount = decay_amount,
                new_total = self.total_points,
                months_elapsed = months_elapsed,
                "Trust points decayed due to inactivity"
            );
            
            decay_amount
        } else {
            0
        }
    }
    
    pub fn can_stake(&self, amount: u32) -> bool {
        self.available_points >= amount
    }
    
    pub fn reputation_weight(&self) -> f64 {
        // Higher trust points = higher weight, but with diminishing returns
        (self.total_points as f64 / 100.0).sqrt().min(2.0).max(0.1)
    }
}

impl NetworkTrustRating {
    /// Calculate current network trust score from participation metrics
    pub fn calculate_network_score(&mut self) -> f64 {
        let metrics = &self.participation_metrics;
        let mut score = 50.0; // Start with neutral score
        
        // Positive factors
        score += (metrics.helpful_responses_given as f64 * 0.5).min(15.0);
        score += (metrics.successful_collaborations as f64 * 2.0).min(10.0);
        score += (metrics.knowledge_contributions as f64 * 1.0).min(10.0);
        score += (metrics.trust_stakes_won as f64 * 3.0).min(15.0);
        
        // Response rate bonus
        if metrics.messages_received > 0 {
            score += (metrics.response_rate * 10.0).min(5.0);
        }
        
        // Account age and activity bonus
        let days_old = metrics.account_age.num_days() as f64;
        score += (days_old / 365.0 * 2.0).min(5.0); // Up to 5 points for being around
        
        let activity_ratio = if days_old > 0.0 {
            metrics.days_active as f64 / days_old
        } else {
            1.0
        };
        score += (activity_ratio * 5.0).min(5.0);
        
        // Negative factors
        score -= (metrics.reports_validated_against as f64 * 5.0).min(20.0);
        score -= (metrics.spam_reports as f64 * 2.0).min(15.0);
        score -= (metrics.trust_stakes_lost as f64 * 1.0).min(10.0);
        
        // Keep score in valid range
        self.network_score = score.max(0.0).min(100.0);
        self.last_calculated = Utc::now();
        
        self.network_score
    }
}

impl Default for NetworkTrustRating {
    fn default() -> Self {
        Self {
            network_score: 50.0, // Start with neutral score
            trust_balance: TrustBalance::new("".to_string()),
            participation_metrics: ParticipationMetrics::default(),
            recent_actions: Vec::new(),
            last_calculated: Utc::now(),
        }
    }
}

impl Default for VerificationLevel {
    fn default() -> Self {
        VerificationLevel::Unverified
    }
}

/// Trust calculation utilities
pub struct TrustCalculator;

impl TrustCalculator {
    /// Calculate composite trust score combining all factors
    pub fn calculate_composite_trust(
        target_trust: &TrustRatings,
        requester_id: &str,
    ) -> CompositeTrustScore {
        // Get direct trust rating if exists
        let direct_trust = target_trust.entity_trust.received_ratings
            .get(requester_id)
            .map(|score| score.score as f64);
        
        // Get network trust score
        let network_trust = target_trust.network_trust.network_score;
        
        // Get network proximity
        let network_proximity = target_trust.network_proximity.degrees_of_separation
            .get(requester_id)
            .cloned()
            .unwrap_or(u32::MAX);
        
        // Calculate proximity score (closer = higher score)
        let proximity_score = if network_proximity == 0 {
            100.0 // Direct connection
        } else if network_proximity < 3 {
            80.0 - (network_proximity as f64 * 10.0)
        } else if network_proximity < 6 {
            50.0 - ((network_proximity - 3) as f64 * 5.0)
        } else {
            20.0 // Distant or no connection
        };
        
        // Verification bonus
        let verification_bonus = match target_trust.identity_verification.verification_level {
            VerificationLevel::Unverified => 0.0,
            VerificationLevel::Basic => 5.0,
            VerificationLevel::Enhanced => 10.0,
            VerificationLevel::Trusted => 15.0,
            VerificationLevel::Authoritative => 20.0,
        };
        
        // Weighted composite calculation
        let personal_weight = if direct_trust.is_some() { 0.4 } else { 0.0 };
        let network_weight = 0.3 + (0.4 - personal_weight); // Compensate for missing personal
        let proximity_weight = 0.2;
        let verification_weight = 0.1;
        
        let composite_score = personal_weight * direct_trust.unwrap_or(50.0) +
            network_weight * network_trust +
            proximity_weight * proximity_score +
            verification_weight * (50.0 + verification_bonus);
        
        CompositeTrustScore {
            composite_score,
            direct_trust,
            network_trust,
            network_proximity,
            verification_bonus,
            components: TrustComponents {
                personal_experience: direct_trust.unwrap_or(50.0),
                network_reputation: network_trust,
                network_proximity: proximity_score,
                identity_verification: 50.0 + verification_bonus,
            },
        }
    }
}

// Custom Serialize/Deserialize implementations for EntityTrustRatings
impl Serialize for EntityTrustRatings {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        
        // Convert DashMaps to HashMaps for serialization
        let received_ratings: HashMap<String, DirectTrustScore> = 
            self.received_ratings.iter().map(|entry| (entry.key().clone(), entry.value().clone())).collect();
        let given_ratings: HashMap<String, DirectTrustScore> = 
            self.given_ratings.iter().map(|entry| (entry.key().clone(), entry.value().clone())).collect();
        
        let mut state = serializer.serialize_struct("EntityTrustRatings", 5)?;
        state.serialize_field("received_ratings", &received_ratings)?;
        state.serialize_field("given_ratings", &given_ratings)?;
        state.serialize_field("average_received", &self.average_received)?;
        state.serialize_field("total_ratings_received", &self.total_ratings_received)?;
        state.serialize_field("last_updated", &self.last_updated)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for EntityTrustRatings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, MapAccess, Visitor};
        use std::fmt;

        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "snake_case")]
        enum Field {
            ReceivedRatings,
            GivenRatings,
            AverageReceived,
            TotalRatingsReceived,
            LastUpdated,
        }

        struct EntityTrustRatingsVisitor;

        impl<'de> Visitor<'de> for EntityTrustRatingsVisitor {
            type Value = EntityTrustRatings;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct EntityTrustRatings")
            }

            fn visit_map<V>(self, mut map: V) -> Result<EntityTrustRatings, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut received_ratings: Option<HashMap<String, DirectTrustScore>> = None;
                let mut given_ratings: Option<HashMap<String, DirectTrustScore>> = None;
                let mut average_received = None;
                let mut total_ratings_received = None;
                let mut last_updated = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::ReceivedRatings => {
                            if received_ratings.is_some() {
                                return Err(de::Error::duplicate_field("received_ratings"));
                            }
                            received_ratings = Some(map.next_value()?);
                        }
                        Field::GivenRatings => {
                            if given_ratings.is_some() {
                                return Err(de::Error::duplicate_field("given_ratings"));
                            }
                            given_ratings = Some(map.next_value()?);
                        }
                        Field::AverageReceived => {
                            if average_received.is_some() {
                                return Err(de::Error::duplicate_field("average_received"));
                            }
                            average_received = Some(map.next_value()?);
                        }
                        Field::TotalRatingsReceived => {
                            if total_ratings_received.is_some() {
                                return Err(de::Error::duplicate_field("total_ratings_received"));
                            }
                            total_ratings_received = Some(map.next_value()?);
                        }
                        Field::LastUpdated => {
                            if last_updated.is_some() {
                                return Err(de::Error::duplicate_field("last_updated"));
                            }
                            last_updated = Some(map.next_value()?);
                        }
                    }
                }

                let received_ratings = received_ratings.ok_or_else(|| de::Error::missing_field("received_ratings"))?;
                let given_ratings = given_ratings.ok_or_else(|| de::Error::missing_field("given_ratings"))?;
                let average_received = average_received.ok_or_else(|| de::Error::missing_field("average_received"))?;
                let total_ratings_received = total_ratings_received.ok_or_else(|| de::Error::missing_field("total_ratings_received"))?;
                let last_updated = last_updated.ok_or_else(|| de::Error::missing_field("last_updated"))?;

                // Convert HashMaps to DashMaps
                let received_dash = DashMap::new();
                for (k, v) in received_ratings {
                    received_dash.insert(k, v);
                }
                
                let given_dash = DashMap::new();
                for (k, v) in given_ratings {
                    given_dash.insert(k, v);
                }

                Ok(EntityTrustRatings {
                    received_ratings: received_dash,
                    given_ratings: given_dash,
                    average_received,
                    total_ratings_received,
                    last_updated,
                })
            }
        }

        const FIELDS: &'static [&'static str] = &["received_ratings", "given_ratings", "average_received", "total_ratings_received", "last_updated"];
        deserializer.deserialize_struct("EntityTrustRatings", FIELDS, EntityTrustRatingsVisitor)
    }
}
