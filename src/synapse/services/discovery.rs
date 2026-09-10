// SPDX-License-Identifier: MIT OR Apache-2.0
use crate::storage::Database;
use crate::synapse::models::DiscoverabilityLevel;
use crate::synapse::models::ParticipantProfile;
use crate::synapse::storage::Cache;
use anyhow::Result;
use std::sync::Arc;
use tracing::{debug, info};

/// Participant discovery service with privacy-aware search
pub struct DiscoveryService {
    database: Arc<Database>,
    cache: Arc<Cache>,
}

impl DiscoveryService {
    pub fn new(database: Arc<Database>, cache: Arc<Cache>) -> Self {
        Self { database, cache }
    }

    /// Discover participants by name with privacy respect
    pub async fn discover_by_name(
        &self,
        query: &str,
        requester_id: &str,
        max_results: usize,
    ) -> Result<Vec<ParticipantProfile>> {
        debug!(
            "Discovering participants by name: {} for requester: {}",
            query, requester_id
        );

        // Check cache first
        let cache_key = format!("discovery:name:{query}:{requester_id}");
        if let Ok(Some(cached)) = self
            .cache
            .get_participant::<Vec<ParticipantProfile>>(&cache_key)
            .await
        {
            return Ok(cached);
        }

        // Perform database search with privacy filtering
        let results = self
            .database
            .search_participants_by_name(query, requester_id, max_results)
            .await?;

        // Cache results for a short time
        self.cache
            .cache_participant(&cache_key, &results, 300)
            .await?;

        info!("Found {} participants matching '{}'", results.len(), query);
        Ok(results)
    }

    /// Discover participants by capabilities
    pub async fn discover_by_capabilities(
        &self,
        capabilities: &[String],
        requester_id: &str,
        max_results: usize,
    ) -> Result<Vec<ParticipantProfile>> {
        debug!(
            "Discovering participants by capabilities: {:?} for requester: {}",
            capabilities, requester_id
        );

        // Check cache first
        let cache_key = format!("discovery:cap:{}:{}", capabilities.join(","), requester_id);
        if let Ok(Some(cached)) = self
            .cache
            .get_participant::<Vec<ParticipantProfile>>(&cache_key)
            .await
        {
            return Ok(cached);
        }

        let results = self
            .database
            .search_participants_by_capabilities(capabilities, requester_id, max_results)
            .await?;
        self.cache
            .cache_participant(&cache_key, &results, 600)
            .await?;

        info!(
            "Found {} participants with capabilities: {:?}",
            results.len(),
            capabilities
        );
        Ok(results)
    }

    /// Discover participants by organization
    pub async fn discover_by_organization(
        &self,
        organization: &str,
        requester_id: &str,
        max_results: usize,
    ) -> Result<Vec<ParticipantProfile>> {
        debug!(
            "Discovering participants by organization: {} for requester: {}",
            organization, requester_id
        );

        // Check cache first
        let cache_key = format!("discovery:org:{organization}:{requester_id}");
        if let Ok(Some(cached)) = self
            .cache
            .get_participant::<Vec<ParticipantProfile>>(&cache_key)
            .await
        {
            return Ok(cached);
        }

        let results = self
            .database
            .search_participants_by_organization(organization, requester_id, max_results)
            .await?;
        self.cache
            .cache_participant(&cache_key, &results, 900)
            .await?;

        info!(
            "Found {} participants in organization: {}",
            results.len(),
            organization
        );
        Ok(results)
    }

    /// Check if a participant can be discovered by another participant
    // Removed feature gating
    pub async fn can_discover(&self, target_id: &str, requester_id: &str) -> Result<bool> {
        let target_profile = self
            .database
            .get_participant(target_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Target participant not found"))?;

        // Check discoverer's permissions
        if requester_id == target_id {
            return Ok(true); // Can always discover yourself
        }

        match target_profile.discovery_permissions.discoverability {
            DiscoverabilityLevel::Public => Ok(true),
            DiscoverabilityLevel::Unlisted => {
                // Check if requester has permission based on relationship or organization
                self.has_discovery_permission(target_id, requester_id).await
            }
            DiscoverabilityLevel::Private => Ok(false),
            DiscoverabilityLevel::Stealth => Ok(false),
        }
    }

    /// Check if requester has discovery permission for target
    async fn has_discovery_permission(
        &self,
        _target_id: &str,
        _requester_id: &str,
    ) -> Result<bool> {
        // This would check:
        // 1. Are they in the same organization?
        // 2. Do they have an existing trust relationship?
        // 3. Are they connected through mutual contacts?
        // For now, simplified implementation
        Ok(false) // Default to no permission
    }

    /// Discover nearby participants (geolocation-based)
    pub async fn discover_by_proximity(
        &self,
        requester_id: &str,
        max_distance: f64,
        max_results: usize,
    ) -> Result<Vec<ParticipantProfile>> {
        debug!(
            "Discovering participants by proximity for requester: {} within distance: {}",
            requester_id, max_distance
        );

        let results = self
            .database
            .search_participants_by_proximity(requester_id, max_distance, max_results)
            .await?;

        info!(
            "Found {} participants within proximity distance: {}",
            results.len(),
            max_distance
        );
        Ok(results)
    }

    /// Get personalized discovery recommendations
    pub async fn get_discovery_recommendations(
        &self,
        requester_id: &str,
        max_results: usize,
    ) -> Result<Vec<ParticipantProfile>> {
        debug!(
            "Getting discovery recommendations for requester: {}",
            requester_id
        );

        // Check cache first
        let cache_key = format!("discovery:recommendations:{requester_id}");
        if let Ok(Some(cached)) = self
            .cache
            .get_participant::<Vec<ParticipantProfile>>(&cache_key)
            .await
        {
            return Ok(cached);
        }

        // Get personalized recommendations based on the requester's profile,
        // previous interactions, and similarity with other participants
        let results = self
            .database
            .get_discovery_recommendations(requester_id, max_results)
            .await?;

        // Cache recommendations for longer time
        self.cache
            .cache_participant(&cache_key, &results, 3600)
            .await?;

        info!(
            "Generated {} discovery recommendations for {}",
            results.len(),
            requester_id
        );
        Ok(results)
    }
}
