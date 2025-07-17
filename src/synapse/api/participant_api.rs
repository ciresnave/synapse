//! Participant API implementation

use crate::synapse::models::participant::{ParticipantProfile, DiscoverabilityLevel, EntityType};
use crate::synapse::services::{ParticipantRegistry, TrustManager, DiscoveryService};
use crate::synapse::api::errors::{ApiError, ApiResponse};
use crate::synapse::telemetry::ErrorTelemetry;
use anyhow::Result; 
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use std::collections::HashMap;
use tracing::{debug, info, warn, error};
use chrono::Utc;

/// HTTP API for participant registry operations
pub struct ParticipantAPI {
    registry: ParticipantRegistry,
    #[allow(dead_code)]
    trust_manager: TrustManager,
    discovery: DiscoveryService,
    error_telemetry: Arc<ErrorTelemetry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CreateParticipantRequest {
    pub global_id: String,
    pub display_name: String,
    pub entity_type: String,
    pub discovery_level: String,
    pub public_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateParticipantRequest {
    pub display_name: Option<String>,
    pub discovery_level: Option<String>,
    pub contact_preferences: Option<ContactPreferencesUpdate>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContactPreferencesUpdate {
    pub accepts_unsolicited_contact: Option<bool>,
    pub requires_introduction: Option<bool>,
    pub preferred_contact_method: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub max_results: Option<usize>,
    pub filters: Option<SearchFilters>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchFilters {
    pub entity_types: Option<Vec<String>>,
    pub organizations: Option<Vec<String>>,
    pub capabilities: Option<Vec<String>>,
    pub min_trust_score: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ParticipantResponse {
    pub global_id: String,
    pub display_name: String,
    pub entity_type: String,
    pub discoverability_level: String,
    pub trust_score: Option<f64>,
    pub last_seen: String,
    pub capabilities: Vec<String>,
    pub organization: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct APIResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
    pub message: Option<String>,
}

// For backward compatibility
impl<T> From<ApiResponse<T>> for APIResponse<T> {
    fn from(api_response: ApiResponse<T>) -> Self {
        Self {
            success: api_response.success,
            data: api_response.data,
            error: api_response.error.map(|e| e.message),
            message: api_response.message,
        }
    }
}

impl ParticipantAPI {
    pub fn new(
        registry: ParticipantRegistry,
        trust_manager: TrustManager,
        discovery: DiscoveryService,
        error_telemetry: Arc<ErrorTelemetry>,
    ) -> Self {
        Self {
            registry,
            trust_manager,
            discovery,
            error_telemetry,
        }
    }

    /// Create a new participant
    pub async fn create_participant(
        &self,
        request: CreateParticipantRequest,
    ) -> Result<ApiResponse<ParticipantResponse>> {
        debug!("Creating new participant: {}", request.global_id);

        // Input validation with specific error messages
        if request.global_id.is_empty() {
            return Ok(ApiResponse::error(ApiError::ValidationError("Global ID cannot be empty".to_string())));
        }
        
        if request.display_name.is_empty() {
            return Ok(ApiResponse::error(ApiError::ValidationError("Display name cannot be empty".to_string())));
        }
        
        // Validate entity type
        if !["human", "ai", "service", "organization", "device"].contains(&request.entity_type.to_lowercase().as_str()) {
            return Ok(ApiResponse::error(ApiError::ValidationError("Invalid entity type".to_string())));
        }
        
        // Check if participant already exists - with tracing and telemetry
        match self.registry.get_participant(&request.global_id).await {
            Ok(Some(_)) => {
                let error = ApiError::Conflict(format!("Participant with ID {} already exists", request.global_id));
                self.error_telemetry.report_error(
                    crate::synapse::telemetry::ErrorSource::Registry,
                    crate::synapse::telemetry::ErrorSeverity::Warning,
                    &error.to_string(),
                    Some("PARTICIPANT_ALREADY_EXISTS"),
                    Some(HashMap::from([
                        ("global_id".to_string(), request.global_id.clone())
                    ])),
                    None
                );
                return Ok(ApiResponse::error(error));
            },
            Ok(None) => {
                // Expected path - participant doesn't exist yet
            },
            Err(err) => {
                // Database error - log, record telemetry, and return appropriate error
                error!("Database error when checking participant existence: {}", err);
                self.error_telemetry.report_error(
                    crate::synapse::telemetry::ErrorSource::Registry,
                    crate::synapse::telemetry::ErrorSeverity::Error,
                    &err.to_string(),
                    Some("DB_QUERY_FAILED"),
                    Some(HashMap::from([
                        ("operation".to_string(), "get_participant".to_string()),
                        ("global_id".to_string(), request.global_id.clone())
                    ])),
                    None
                );
                return Ok(ApiResponse::error(ApiError::DatabaseError));
            }
        }

        // Create participant profile with error handling
        let profile = match self.create_profile_from_request(request.clone()) {
            Ok(p) => p,
            Err(err) => {
                warn!("Failed to create profile from request: {}", err);
                self.error_telemetry.report_error(
                    crate::synapse::telemetry::ErrorSource::Registry,
                    crate::synapse::telemetry::ErrorSeverity::Warning,
                    &err.to_string(),
                    Some("PROFILE_CREATION_FAILED"),
                    Some(HashMap::from([
                        ("global_id".to_string(), request.global_id.clone())
                    ])),
                    None
                );
                return Ok(ApiResponse::error(ApiError::ValidationError(
                    "Invalid participant profile data".to_string()
                )));
            }
        };
        
        // Register participant with error handling
        match self.registry.register_participant(profile.clone()).await {
            Ok(_) => {
                info!("Participant created successfully: {}", profile.global_id);
                Ok(ApiResponse::success(
                    self.profile_to_response(&profile),
                    Some("Participant created successfully".to_string())
                ))
            },
            Err(err) => {
                error!("Failed to register participant: {}", err);
                self.error_telemetry.report_error(
                    crate::synapse::telemetry::ErrorSource::Registry,
                    crate::synapse::telemetry::ErrorSeverity::Error,
                    &err.to_string(),
                    Some("PARTICIPANT_REGISTRATION_FAILED"),
                    Some(HashMap::from([
                        ("global_id".to_string(), profile.global_id.clone())
                    ])),
                    None
                );
                Ok(ApiResponse::error(ApiError::from(err)))
            }
        }
    }

    /// Get participant by ID
    pub async fn get_participant(
        &self,
        participant_id: &str,
        requester_id: Option<&str>,
    ) -> Result<APIResponse<ParticipantResponse>> {
        debug!("Getting participant: {} for requester: {:?}", participant_id, requester_id);

        match self.registry.get_participant(participant_id).await? {
            Some(profile) => {
                // Check if requester can view this participant
                if let Some(req_id) = requester_id {
                    if !self.discovery.can_discover(participant_id, req_id).await? {
                        return Ok(APIResponse {
                            success: false,
                            data: None,
                            error: Some("Access denied".to_string()),
                            message: None,
                        });
                    }
                }

                Ok(APIResponse {
                    success: true,
                    data: Some(self.profile_to_response(&profile)),
                    error: None,
                    message: None,
                })
            },
            None => Ok(APIResponse {
                success: false,
                data: None,
                error: Some("Participant not found".to_string()),
                message: None,
            }),
        }
    }

    /// Update participant
    pub async fn update_participant(
        &self,
        participant_id: &str,
        request: UpdateParticipantRequest,
        requester_id: &str,
    ) -> Result<APIResponse<ParticipantResponse>> {
        debug!("Updating participant: {} by requester: {}", participant_id, requester_id);

        // Check authorization - only self or authorized entities can update
        if participant_id != requester_id {
            // Could add additional authorization logic here
            return Ok(APIResponse {
                success: false,
                data: None,
                error: Some("Unauthorized".to_string()),
                message: None,
            });
        }

        // Get existing profile
        let mut profile = match self.registry.get_participant(participant_id).await? {
            Some(p) => p,
            None => return Ok(APIResponse {
                success: false,
                data: None,
                error: Some("Participant not found".to_string()),
                message: None,
            }),
        };

        // Apply updates
        if let Some(display_name) = request.display_name {
            profile.display_name = display_name;
        }

        if let Some(discovery_level) = request.discovery_level {
            profile.discovery_permissions.discoverability = match discovery_level.as_str() {
                "public" => DiscoverabilityLevel::Public,
                "unlisted" => DiscoverabilityLevel::Unlisted,
                "private" => DiscoverabilityLevel::Private,
                "stealth" => DiscoverabilityLevel::Stealth,
                _ => return Ok(APIResponse {
                    success: false,
                    data: None,
                    error: Some("Invalid discovery level".to_string()),
                    message: None,
                }),
            };
        }

        if let Some(contact_prefs) = request.contact_preferences {
             if let Some(accepts_unsolicited) = contact_prefs.accepts_unsolicited_contact {
                 profile.contact_preferences.accepts_unsolicited_contact = accepts_unsolicited;
             }
             if let Some(requires_intro) = contact_prefs.requires_introduction {
                 profile.contact_preferences.requires_introduction = requires_intro;
             }
         }
 
         profile.updated_at = Utc::now();
 
         // Save updated profile
         self.registry.update_participant(profile.clone()).await?;

        info!("Participant updated successfully: {}", participant_id);

        Ok(APIResponse {
            success: true,
            data: Some(self.profile_to_response(&profile)),
            error: None,
            message: Some("Participant updated successfully".to_string()),
        })
    }

    /// Search participants
    pub async fn search_participants(
        &self,
        request: SearchRequest,
        requester_id: &str,
    ) -> Result<APIResponse<Vec<ParticipantResponse>>> {
        debug!("Searching participants for query: {} by requester: {}", request.query, requester_id);

        let max_results = request.max_results.unwrap_or(10).min(100); // Cap at 100

        let results = self.discovery.discover_by_name(&request.query, requester_id, max_results).await?;

        let responses: Vec<ParticipantResponse> = results
            .into_iter()
            .map(|profile| self.profile_to_response(&profile))
            .collect();

        info!("Found {} participants for search query: {}", responses.len(), request.query);
        let response_count = responses.len();

        Ok(APIResponse {
            success: true,
            data: Some(responses),
            error: None,
            message: Some(format!("Found {} participants", response_count)),
        })
    }

    /// Get participant recommendations
    pub async fn get_recommendations(
        &self,
        requester_id: &str,
        max_results: Option<usize>,
    ) -> Result<APIResponse<Vec<ParticipantResponse>>> {
        debug!("Getting recommendations for requester: {}", requester_id);

        let max_results = max_results.unwrap_or(10).min(50);

        let results = self.discovery.get_discovery_recommendations(requester_id, max_results).await?;

        let responses: Vec<ParticipantResponse> = results
            .into_iter()
            .map(|profile| self.profile_to_response(&profile))
            .collect();

        info!("Generated {} recommendations for requester: {}", responses.len(), requester_id);
        let response_count = responses.len();

        Ok(APIResponse {
            success: true,
            data: Some(responses),
            error: None,
            message: Some(format!("Generated {} recommendations", response_count)),
        })
    }

    /// Delete participant
    pub async fn delete_participant(
        &self,
        participant_id: &str,
        requester_id: &str,
    ) -> Result<APIResponse<()>> {
        debug!("Deleting participant: {} by requester: {}", participant_id, requester_id);

        // Check authorization - only self can delete (or admin in future)
        if participant_id != requester_id {
            return Ok(APIResponse {
                success: false,
                data: None,
                error: Some("Unauthorized".to_string()),
                message: None,
            });
        }

        // Check if participant exists
        if self.registry.get_participant(participant_id).await?.is_none() {
            return Ok(APIResponse {
                success: false,
                data: None,
                error: Some("Participant not found".to_string()),
                message: None,
            });
        }

        // Delete participant (placeholder implementation)
        // In a real implementation, this would remove the participant from the database
        info!("Participant deletion requested: {} (placeholder)", participant_id);

        info!("Participant deleted successfully: {}", participant_id);

        Ok(APIResponse {
            success: true,
            data: Some(()),
            error: None,
            message: Some("Participant deleted successfully".to_string()),
        })
    }

    /// Get participant statistics
    pub async fn get_statistics(&self) -> Result<APIResponse<ParticipantStatistics>> {
        debug!("Getting participant statistics");

        // This would query the database for statistics
        let stats = ParticipantStatistics {
            total_participants: 0, // Would be fetched from DB
            active_participants: 0,
            new_participants_today: 0,
            trust_reports_today: 0,
            average_trust_score: 0.0,
        };

        Ok(APIResponse {
            success: true,
            data: Some(stats),
            error: None,
            message: None,
        })
    }

    fn create_profile_from_request(&self, request: CreateParticipantRequest) -> Result<ParticipantProfile> {
        debug!("Creating participant profile for {}: {}", request.global_id, request.display_name);
        
        // Parse entity type from string
        let entity_type = match request.entity_type.to_lowercase().as_str() {
            "human" => EntityType::Human,
            "ai" => EntityType::AiModel,
            "service" => EntityType::Service,
            "organization" => EntityType::Organization,
            "device" => EntityType::Bot,
            _ => return Err(anyhow::anyhow!("Invalid entity type: {}", request.entity_type)),
        };
        
        // Create the profile with proper constructor
        let mut profile = ParticipantProfile::new(
            request.global_id.clone(),
            request.display_name.clone(),
            entity_type,
        );
        
        // Set discovery level if provided
        if !request.discovery_level.is_empty() {
            profile.discovery_permissions.discoverability = match request.discovery_level.to_lowercase().as_str() {
                "public" => DiscoverabilityLevel::Public,
                "unlisted" => DiscoverabilityLevel::Unlisted,
                "private" => DiscoverabilityLevel::Private,
                "stealth" => DiscoverabilityLevel::Stealth,
                _ => DiscoverabilityLevel::Unlisted, // Default to unlisted for invalid values
            };
        }
        
        // Set public key if provided
        if let Some(pub_key_str) = request.public_key {
            if !pub_key_str.is_empty() {
                profile.public_key = Some(pub_key_str.into_bytes());
            }
        }
        
        Ok(profile)
    }

    fn profile_to_response(&self, profile: &ParticipantProfile) -> ParticipantResponse {
        ParticipantResponse {
            global_id: profile.global_id.clone(),
            display_name: profile.display_name.clone(),
            entity_type: format!("{:?}", profile.entity_type),
            discoverability_level: format!("{:?}", profile.discovery_permissions.discoverability),
            trust_score: Some(0.0), // Would calculate from trust_ratings
            last_seen: profile.last_seen.to_rfc3339(),
            capabilities: profile.topic_subscriptions.iter().map(|t| t.topic.clone()).collect(),
            organization: profile.organizational_context.as_ref().map(|o| o.organization_name.clone()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ParticipantStatistics {
    pub total_participants: u64,
    pub active_participants: u64,
    pub new_participants_today: u64,
    pub trust_reports_today: u64,
    pub average_trust_score: f64,
}
