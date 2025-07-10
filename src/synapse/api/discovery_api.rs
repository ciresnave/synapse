use crate::synapse::services::DiscoveryService;
use crate::synapse::models::ParticipantProfile;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{info, debug};

/// HTTP API for participant discovery operations
pub struct DiscoveryAPI {
    discovery: DiscoveryService,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DiscoveryRequest {
    pub query: String,
    pub discovery_type: String, // "name", "capabilities", "organization", "proximity"
    pub max_results: Option<usize>,
    pub filters: Option<DiscoveryFilters>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DiscoveryFilters {
    pub entity_types: Option<Vec<String>>,
    pub organizations: Option<Vec<String>>,
    pub capabilities: Option<Vec<String>>,
    pub min_trust_score: Option<f64>,
    pub max_distance: Option<f64>,
    pub discoverability_levels: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProximityDiscoveryRequest {
    pub max_distance: f64, // Degrees of separation in trust network
    pub max_results: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RecommendationRequest {
    pub recommendation_type: String, // "similar_interests", "mutual_connections", "activity_based"
    pub max_results: Option<usize>,
    pub include_explanations: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DiscoveryResult {
    pub participant_id: String,
    pub display_name: String,
    pub entity_type: String,
    pub organization: Option<String>,
    pub capabilities: Vec<String>,
    pub trust_score: Option<f64>,
    pub distance: Option<f64>, // Degrees of separation
    pub match_reason: Option<String>,
    pub last_seen: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RecommendationResult {
    pub participant: DiscoveryResult,
    pub recommendation_score: f64,
    pub explanation: Option<String>,
    pub connection_path: Option<Vec<String>>, // Path through trust network
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DiscoveryStatistics {
    pub total_discoverable_participants: u64,
    pub participants_by_discoverability: DiscoverabilityBreakdown,
    pub popular_capabilities: Vec<CapabilityCount>,
    pub active_organizations: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DiscoverabilityBreakdown {
    pub public: u64,
    pub unlisted: u64,
    pub private: u64,
    pub stealth: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CapabilityCount {
    pub capability: String,
    pub count: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct APIResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
    pub message: Option<String>,
}

impl DiscoveryAPI {
    pub fn new(discovery: DiscoveryService) -> Self {
        Self { discovery }
    }

    /// Perform general discovery search
    pub async fn discover_participants(
        &self,
        requester_id: &str,
        request: DiscoveryRequest,
    ) -> Result<APIResponse<Vec<DiscoveryResult>>> {
        debug!("Discovery request from {}: type={}, query={}", 
               requester_id, request.discovery_type, request.query);

        if request.query.is_empty() {
            return Ok(APIResponse {
                success: false,
                data: None,
                error: Some("Query cannot be empty".to_string()),
                message: None,
            });
        }

        let max_results = request.max_results.unwrap_or(10).min(100);

        let profiles = match request.discovery_type.as_str() {
            "name" => {
                self.discovery.discover_by_name(&request.query, requester_id, max_results).await?
            },
            "capabilities" => {
                let capabilities: Vec<String> = request.query.split(',').map(|s| s.trim().to_string()).collect();
                self.discovery.discover_by_capabilities(&capabilities, requester_id, max_results).await?
            },
            "organization" => {
                self.discovery.discover_by_organization(&request.query, requester_id, max_results).await?
            },
            _ => {
                return Ok(APIResponse {
                    success: false,
                    data: None,
                    error: Some("Invalid discovery type".to_string()),
                    message: None,
                });
            }
        };

        let results: Vec<DiscoveryResult> = profiles.into_iter()
            .map(|profile| self.profile_to_discovery_result(&profile, None))
            .collect();

        let results_count = results.len();
        info!("Discovery found {} results for query: {}", 
              results_count, request.query);

        Ok(APIResponse {
            success: true,
            data: Some(results),
            error: None,
            message: Some(format!("Found {} participants", results_count)),
        })
    }

    /// Discover participants by proximity in trust network
    pub async fn discover_by_proximity(
        &self,
        requester_id: &str,
        request: ProximityDiscoveryRequest,
    ) -> Result<APIResponse<Vec<DiscoveryResult>>> {
        debug!("Proximity discovery request from {} with max distance: {}", 
               requester_id, request.max_distance);

        if request.max_distance <= 0.0 || request.max_distance > 10.0 {
            return Ok(APIResponse {
                success: false,
                data: None,
                error: Some("Max distance must be between 0 and 10".to_string()),
                message: None,
            });
        }

        let max_results = request.max_results.unwrap_or(10).min(50);

        let profiles = self.discovery.discover_by_proximity(
            requester_id, 
            request.max_distance, 
            max_results
        ).await?;

        let results: Vec<_> = profiles.into_iter()
            .map(|profile| self.profile_to_discovery_result(&profile, Some(request.max_distance)))
            .collect();

        let results_count = results.len();
        info!("Proximity discovery found {} results within distance: {}", 
              results_count, request.max_distance);

        Ok(APIResponse {
            success: true,
            data: Some(results),
            error: None,
            message: Some(format!("Found {} participants within proximity", results_count)),
        })
    }

    /// Get personalized recommendations
    pub async fn get_recommendations(
        &self,
        requester_id: &str,
        request: RecommendationRequest,
    ) -> Result<APIResponse<Vec<RecommendationResult>>> {
        debug!("Recommendation request from {} of type: {}", 
               requester_id, request.recommendation_type);

        let max_results = request.max_results.unwrap_or(10).min(20);

        let profiles = self.discovery.get_discovery_recommendations(requester_id, max_results).await?;

        let results: Vec<_> = profiles.into_iter()
            .enumerate()
            .map(|(index, profile)| {
                let discovery_result = self.profile_to_discovery_result(&profile, None);
                RecommendationResult {
                    participant: discovery_result,
                    recommendation_score: 1.0 - (index as f64 * 0.1), // Simple scoring
                    explanation: if request.include_explanations.unwrap_or(false) {
                        Some("Based on similar interests and network connections".to_string())
                    } else {
                        None
                    },
                    connection_path: None, // Would calculate actual path
                }
            })
            .collect();

        let results_count = results.len();
        info!("Generated {} recommendations for requester: {}", 
              results_count, requester_id);

        Ok(APIResponse {
            success: true,
            data: Some(results),
            error: None,
            message: Some(format!("Generated {} recommendations", results_count)),
        })
    }

    /// Check if a participant can be discovered
    pub async fn check_discoverability(
        &self,
        target_id: &str,
        requester_id: &str,
    ) -> Result<APIResponse<DiscoverabilityCheck>> {
        debug!("Checking discoverability of {} for requester: {}", target_id, requester_id);

        let can_discover = self.discovery.can_discover(target_id, requester_id).await?;

        let result = DiscoverabilityCheck {
            target_id: target_id.to_string(),
            requester_id: requester_id.to_string(),
            can_discover,
            reason: if can_discover {
                "Access granted".to_string()
            } else {
                "Access denied due to privacy settings".to_string()
            },
        };

        Ok(APIResponse {
            success: true,
            data: Some(result),
            error: None,
            message: None,
        })
    }

    /// Get discovery statistics
    pub async fn get_discovery_statistics(
        &self,
        requester_id: &str,
    ) -> Result<APIResponse<DiscoveryStatistics>> {
        debug!("Getting discovery statistics for requester: {}", requester_id);

        // This would query the database for actual statistics
        let stats = DiscoveryStatistics {
            total_discoverable_participants: 0,
            participants_by_discoverability: DiscoverabilityBreakdown {
                public: 0,
                unlisted: 0,
                private: 0,
                stealth: 0,
            },
            popular_capabilities: vec![],
            active_organizations: vec![],
        };

        Ok(APIResponse {
            success: true,
            data: Some(stats),
            error: None,
            message: None,
        })
    }

    /// Batch check discoverability for multiple participants
    pub async fn batch_check_discoverability(
        &self,
        target_ids: Vec<String>,
        requester_id: &str,
    ) -> Result<APIResponse<Vec<DiscoverabilityCheck>>> {
        debug!("Batch checking discoverability for {} targets by requester: {}", 
               target_ids.len(), requester_id);

        if target_ids.len() > 100 {
            return Ok(APIResponse {
                success: false,
                data: None,
                error: Some("Too many targets (max 100)".to_string()),
                message: None,
            });
        }

        let mut results = Vec::new();
        for target_id in target_ids {
            let can_discover = self.discovery.can_discover(&target_id, requester_id).await?;
            results.push(DiscoverabilityCheck {
                target_id: target_id.clone(),
                requester_id: requester_id.to_string(),
                can_discover,
                reason: if can_discover {
                    "Access granted".to_string()
                } else {
                    "Access denied".to_string()
                },
            });
        }

        let result_count = results.len();

        Ok(APIResponse {
            success: true,
            data: Some(results),
            error: None,
            message: Some(format!("Checked {} participants", result_count)),
        })
    }

    fn profile_to_discovery_result(&self, profile: &ParticipantProfile, distance: Option<f64>) -> DiscoveryResult {
        DiscoveryResult {
            participant_id: profile.global_id.clone(),
            display_name: profile.display_name.clone(),
            entity_type: format!("{:?}", profile.entity_type),
            organization: profile.organizational_context.as_ref().map(|o| o.organization_name.clone()),
            capabilities: profile.topic_subscriptions.iter().map(|t| t.topic.clone()).collect(),
            trust_score: Some(0.0), // Would calculate from trust_ratings
            distance,
            match_reason: None, // Could add matching explanation
            last_seen: profile.last_seen.to_rfc3339(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DiscoverabilityCheck {
    pub target_id: String,
    pub requester_id: String,
    pub can_discover: bool,
    pub reason: String,
}
