// Synapse Participant Registry Service
// Core registry for managing participant profiles and relationships

use crate::synapse::models::{ParticipantProfile, DiscoverabilityLevel};
use crate::blockchain::serialization::DateTimeWrapper;
#[cfg(feature = "database")]
use crate::synapse::storage::Database;
#[cfg(feature = "cache")]
use crate::synapse::storage::Cache;
use crate::synapse::services::trust_manager::TrustManager;
use anyhow::{Context, Result};
use chrono::Utc;
use std::sync::Arc;
use tracing::info;

/// Main participant registry service
pub struct ParticipantRegistry {
    #[cfg(feature = "database")]
    database: Arc<Database>,
    #[cfg(feature = "cache")]
    cache: Arc<Cache>,
    trust_manager: Arc<TrustManager>,
}

impl ParticipantRegistry {
    /// Create new registry instance
    #[cfg(all(feature = "database", feature = "cache"))]
    pub async fn new(
        database: Arc<Database>,
        cache: Arc<Cache>,
        trust_manager: Arc<TrustManager>,
    ) -> Result<Self> {
        Ok(Self {
            database,
            cache,
            trust_manager,
        })
    }
    
    /// Create new registry instance without cache
    #[cfg(all(feature = "database", not(feature = "cache")))]
    pub async fn new(
        database: Arc<Database>,
        trust_manager: Arc<TrustManager>,
    ) -> Result<Self> {
        Ok(Self {
            database,
            trust_manager,
        })
    }
    
    /// Create new registry instance without database
    #[cfg(all(feature = "cache", not(feature = "database")))]
    pub async fn new(
        cache: Arc<Cache>,
        trust_manager: Arc<TrustManager>,
    ) -> Result<Self> {
        Ok(Self {
            cache,
            trust_manager,
        })
    }
    
    /// Create new registry instance without database and cache
    #[cfg(not(any(feature = "database", feature = "cache")))]
    pub async fn new(
        trust_manager: Arc<TrustManager>,
    ) -> Result<Self> {
        Ok(Self {
            trust_manager,
        })
    }
    
    /// Register a new participant with full storage
    #[cfg(all(feature = "database", feature = "cache"))]
    pub async fn register_participant(&self, mut profile: ParticipantProfile) -> Result<()> {
        // Set timestamps
        let now = DateTimeWrapper::new(Utc::now());
        profile.created_at = now.clone().into_inner();
        profile.updated_at = now.clone().into_inner();
        profile.last_seen = now.into_inner();

        // Validate the profile
        self.validate_profile(&profile)?;
        
        // Store in database
        self.database.upsert_participant(&profile).await
            .context("Failed to store participant in database")?;
        
        // Cache the profile
        let cache_key = format!("participant:{}", profile.global_id);
        self.cache.cache_participant(&cache_key, &profile, 3600).await // 1 hour TTL
            .context("Failed to cache participant")?;
        
        // Initialize trust balance
        self.trust_manager.initialize_participant(&profile.global_id).await
            .context("Failed to initialize trust balance")?;
        
        info!("Registered new participant: {}", profile.global_id);
        Ok(())
    }
    
    /// Register a new participant with database only
    #[cfg(all(feature = "database", not(feature = "cache")))]
    pub async fn register_participant(&self, mut profile: ParticipantProfile) -> Result<()> {
        // Set timestamps
        let now = DateTimeWrapper::new(Utc::now());
        profile.created_at = now;
        profile.updated_at = now;
        profile.last_seen = now;
        
        // Validate the profile
        self.validate_profile(&profile)?;
        
        // Store in database
        self.database.upsert_participant(&profile).await
            .context("Failed to store participant in database")?;
        
        // Initialize trust balance
        self.trust_manager.initialize_participant(&profile.global_id).await
            .context("Failed to initialize trust balance")?;
        
        info!("Registered new participant: {}", profile.global_id);
        Ok(())
    }
    
    /// Register a new participant with cache only
    #[cfg(all(feature = "cache", not(feature = "database")))]
    pub async fn register_participant(&self, mut profile: ParticipantProfile) -> Result<()> {
        // Set timestamps
        let now = DateTimeWrapper::new(Utc::now());
        profile.created_at = now;
        profile.updated_at = now;
        profile.last_seen = now;
        
        // Validate the profile
        self.validate_profile(&profile)?;
        
        // Cache the profile
        let cache_key = format!("participant:{}", profile.global_id);
        self.cache.cache_participant(&cache_key, &profile, 3600).await // 1 hour TTL
            .context("Failed to cache participant")?;
        
        // Initialize trust balance
        self.trust_manager.initialize_participant(&profile.global_id).await
            .context("Failed to initialize trust balance")?;
        
        info!("Registered new participant: {}", profile.global_id);
        Ok(())
    }
    
    /// Register a new participant with memory only
    #[cfg(not(any(feature = "database", feature = "cache")))]
    pub async fn register_participant(&self, mut profile: ParticipantProfile) -> Result<()> {
        // Set timestamps
        let now = DateTimeWrapper::new(Utc::now());
        profile.created_at = now;
        profile.updated_at = now;
        profile.last_seen = now;
        
        // Validate the profile
        self.validate_profile(&profile)?;
        
        // Initialize trust balance
        self.trust_manager.initialize_participant(&profile.global_id).await
            .context("Failed to initialize trust balance")?;
        
        info!("Registered new participant (memory only): {}", profile.global_id);
        Ok(())
    }
    
    /// Update an existing participant profile with full storage
    #[cfg(all(feature = "database", feature = "cache"))]
    pub async fn update_participant(&self, mut profile: ParticipantProfile) -> Result<()> {
        // Check if participant exists
        let existing = self.get_participant(&profile.global_id).await?;
        if existing.is_none() {
            return Err(anyhow::anyhow!("Participant not found: {}", profile.global_id));
        }
        
        // Update timestamp
        profile.updated_at = Utc::now();
        
        // Validate the profile
        self.validate_profile(&profile)?;
        
        // Store in database
        self.database.upsert_participant(&profile).await
            .context("Failed to update participant in database")?;
        
        // Invalidate cache
        let cache_key = format!("participant:{}", profile.global_id);
        self.cache.invalidate(&cache_key).await
            .context("Failed to invalidate cache")?;
        
        info!("Updated participant: {}", profile.global_id);
        Ok(())
    }
    
    /// Update an existing participant profile with database only
    #[cfg(all(feature = "database", not(feature = "cache")))]
    pub async fn update_participant(&self, mut profile: ParticipantProfile) -> Result<()> {
        // Check if participant exists
        let existing = self.get_participant(&profile.global_id).await?;
        if existing.is_none() {
            return Err(anyhow::anyhow!("Participant not found: {}", profile.global_id));
        }
        
        // Update timestamp
        profile.updated_at = DateTimeWrapper::new(Utc::now());
        
        // Validate the profile
        self.validate_profile(&profile)?;
        
        // Store in database
        self.database.upsert_participant(&profile).await
            .context("Failed to update participant in database")?;
        
        info!("Updated participant: {}", profile.global_id);
        Ok(())
    }
    
    /// Update an existing participant profile with cache only
    #[cfg(all(feature = "cache", not(feature = "database")))]
    pub async fn update_participant(&self, mut profile: ParticipantProfile) -> Result<()> {
        // Check if participant exists
        let existing = self.get_participant(&profile.global_id).await?;
        if existing.is_none() {
            return Err(anyhow::anyhow!("Participant not found: {}", profile.global_id));
        }
        
        // Update timestamp
        profile.updated_at = DateTimeWrapper::new(Utc::now());
        
        // Validate the profile
        self.validate_profile(&profile)?;
        
        // Update cache
        let cache_key = format!("participant:{}", profile.global_id);
        self.cache.cache_participant(&cache_key, &profile, 3600).await // 1 hour TTL
            .context("Failed to update participant in cache")?;
        
        info!("Updated participant (cache only): {}", profile.global_id);
        Ok(())
    }
    
    /// Update an existing participant profile with memory only
    #[cfg(not(any(feature = "database", feature = "cache")))]
    pub async fn update_participant(&self, mut profile: ParticipantProfile) -> Result<()> {
        // Update timestamp
        profile.updated_at = DateTimeWrapper::new(Utc::now());
        
        // Validate the profile
        self.validate_profile(&profile)?;
        
        info!("Updated participant (memory only): {}", profile.global_id);
        Ok(())
    }
    
    /// Get participant by global ID with full storage
    #[cfg(all(feature = "database", feature = "cache"))]
    pub async fn get_participant(&self, global_id: &str) -> Result<Option<ParticipantProfile>> {
        let cache_key = format!("participant:{}", global_id);
        
        // Try cache first
        if let Ok(Some(cached)) = self.cache.get_cached::<ParticipantProfile>(&cache_key).await {
            return Ok(Some(cached));
        }
        
        // Fall back to database
        let profile = self.database.get_participant(global_id).await
            .context("Failed to get participant from database")?;
        
        // Cache the result if found
        if let Some(ref p) = profile {
            let _ = self.cache.cache_participant(&cache_key, p, 3600).await;
        }
        
        Ok(profile)
    }
    
    /// Get participant by global ID with database only
    #[cfg(all(feature = "database", not(feature = "cache")))]
    pub async fn get_participant(&self, global_id: &str) -> Result<Option<ParticipantProfile>> {
        // Get from database
        let profile = self.database.get_participant(global_id).await
            .context("Failed to get participant from database")?;
        
        Ok(profile)
    }
    
    /// Get participant by global ID with cache only
    #[cfg(all(feature = "cache", not(feature = "database")))]
    pub async fn get_participant(&self, global_id: &str) -> Result<Option<ParticipantProfile>> {
        let cache_key = format!("participant:{}", global_id);
        
        // Try cache
        let cached = self.cache.get_cached::<ParticipantProfile>(&cache_key).await
            .context("Failed to get participant from cache")?;
        
        Ok(cached)
    }
    
    /// Get participant by global ID with memory only
    #[cfg(not(any(feature = "database", feature = "cache")))]
    pub async fn get_participant(&self, _global_id: &str) -> Result<Option<ParticipantProfile>> {
        // In memory-only mode, we don't have persistent storage
        Ok(None)
    }
    
    /// Search for participants with privacy filtering (full storage)
    #[cfg(all(feature = "database", feature = "cache"))]
    pub async fn search_participants(
        &self,
        query: &ContactSearchQuery,
    ) -> Result<Vec<ParticipantProfile>> {
        // Generate cache key for search
        let query_hash = self.generate_search_hash(query);
        let _cache_key = format!("search:{}", query_hash); // TODO: Use cache_key or remove if not needed
        
        // Try cache first
        if let Ok(Some(cached_ids)) = self.cache.get_cached_search_results(&query_hash).await {
            let mut results = Vec::new();
            for id in cached_ids {
                if let Ok(Some(profile)) = self.get_participant(&id).await {
                    results.push(profile);
                }
            }
            if !results.is_empty() {
                return Ok(results);
            }
        }
        
        // Search database
        let mut results = self.database.search_participants(
            &query.query,
            &query.requester_id,
            query.max_results,
        ).await.context("Failed to search database")?;
        
        // Apply privacy filtering
        results = self.apply_privacy_filters(results, query).await?;
        
        // Apply trust filtering
        results = self.apply_trust_filters(results, query).await?;
        
        // Cache the search results
        let result_ids: Vec<String> = results.iter().map(|p| p.global_id.clone()).collect();
        let _ = self.cache.cache_search_results(&query_hash, &result_ids, 600).await; // 10 min TTL
        
        Ok(results)
    }

    /// Search for participants with privacy filtering (database only)
    #[cfg(all(feature = "database", not(feature = "cache")))]
    pub async fn search_participants(
        &self,
        query: &ContactSearchQuery,
    ) -> Result<Vec<ParticipantProfile>> {
        // Search database
        let mut results = self.database.search_participants(
            &query.query,
            &query.requester_id,
            query.max_results,
        ).await.context("Failed to search database")?;
        
        // Apply privacy filtering
        results = self.apply_privacy_filters(results, query).await?;
        
        // Apply trust filtering
        results = self.apply_trust_filters(results, query).await?;
        
        Ok(results)
    }

    /// Search for participants with privacy filtering (cache only)
    #[cfg(all(feature = "cache", not(feature = "database")))]
    pub async fn search_participants(
        &self,
        query: &ContactSearchQuery,
    ) -> Result<Vec<ParticipantProfile>> {
        // With cache-only we can't do a proper search
        Ok(Vec::new())
    }
    
    /// Search for participants with privacy filtering (no storage)
    #[cfg(not(any(feature = "database", feature = "cache")))]
    pub async fn search_participants(
        &self,
        _query: &ContactSearchQuery,
    ) -> Result<Vec<ParticipantProfile>> {
        // With no storage we can't do a proper search
        Ok(Vec::new())
    }
    
    /// Get participants by organization (full storage)
    #[cfg(all(feature = "database", feature = "cache"))]
    pub async fn get_participants_by_organization(&self, organization: &str) -> Result<Vec<ParticipantProfile>> {
        // Generate cache key
        let cache_key = format!("org:{}", organization);
        
        // Try cache first
        if let Ok(Some(cached_ids)) = self.cache.get_cached::<Vec<String>>(&cache_key).await {
            let mut results = Vec::new();
            for id in cached_ids {
                if let Ok(Some(profile)) = self.get_participant(&id).await {
                    results.push(profile);
                }
            }
            if !results.is_empty() {
                return Ok(results);
            }
        }
        
        // Query database
        let sql_query = r#"
            SELECT global_id, display_name, entity_type, identities,
                   discovery_permissions, availability, contact_preferences,
                   trust_ratings, topic_subscriptions, organizational_context,
                   public_key, supported_protocols, last_seen, created_at, updated_at
            FROM participants 
            WHERE 
                (organizational_context->>'organization' = $1 OR
                 EXISTS (
                     SELECT 1 FROM jsonb_array_elements(identities) AS identity
                     WHERE identity->>'organization' = $1
                 ))
                AND 
                (discovery_permissions->>'discoverability' = 'Public' OR
                 discovery_permissions->>'discoverability' = 'Unlisted')
            LIMIT 100
        "#;
        
        let results = self.database.query_participants(sql_query, &[&organization]).await
            .context("Failed to query participants by organization")?;
            
        // Cache the results
        let result_ids: Vec<String> = results.iter().map(|p| p.global_id.clone()).collect();
        let _ = self.cache.cache_participant(&cache_key, &result_ids, 600).await; // 10 min TTL
        
        Ok(results)
    }
    
    /// Get participants by organization (database only)
    #[cfg(all(feature = "database", not(feature = "cache")))]
    pub async fn get_participants_by_organization(&self, organization: &str) -> Result<Vec<ParticipantProfile>> {
        // Simplified version for database-only mode
        let result_ids = self.database.get_participants_by_organization(organization).await
            .context("Failed to get participants by organization from database")?;
        
        let mut results = Vec::new();
        for id in result_ids {
            if let Ok(Some(profile)) = self.get_participant(&id).await {
                results.push(profile);
            }
        }
        
        Ok(results)
    }

    /// Get participants by organization (cache only)
    #[cfg(all(feature = "cache", not(feature = "database")))]
    pub async fn get_participants_by_organization(&self, organization: &str) -> Result<Vec<ParticipantProfile>> {
        // Generate cache key
        let cache_key = format!("org:{}", organization);
        
        // Try cache
        if let Ok(Some(cached_ids)) = self.cache.get_cached::<Vec<String>>(&cache_key).await {
            let mut results = Vec::new();
            for id in cached_ids {
                if let Ok(Some(profile)) = self.get_participant(&id).await {
                    results.push(profile);
                }
            }
            return Ok(results);
        }
        
        Ok(Vec::new())
    }
    
    /// Get participants by organization (no storage)
    #[cfg(not(any(feature = "database", feature = "cache")))]
    pub async fn get_participants_by_organization(&self, _organization: &str) -> Result<Vec<ParticipantProfile>> {
        // With no storage we can't get participants by organization
        Ok(Vec::new())
    }
    
    /// Get participants by organization - database implementation
    #[cfg(all(feature = "database", not(feature = "cache")))]
    async fn query_participants_by_organization(&self, organization: &str) -> Result<Vec<ParticipantProfile>> {
        // Query database
        let sql_query = r#"
            SELECT global_id, display_name, entity_type, identities,
                   discovery_permissions, availability, contact_preferences,
                   trust_ratings, topic_subscriptions, organizational_context,
                   public_key, supported_protocols, last_seen, created_at, updated_at
            FROM participants
            WHERE 
                (organizational_context->>'organization' = $1 OR
                 EXISTS (
                     SELECT 1 FROM jsonb_array_elements(identities) AS identity
                     WHERE identity->>'organization' = $1
                 ))
                AND 
                (discovery_permissions->>'discoverability' = 'Public' OR
                 discovery_permissions->>'discoverability' = 'Unlisted')
            LIMIT 100
        "#;
        
        let results = self.database.query_participants(sql_query, &[&organization]).await
            .context("Failed to query participants by organization")?;
            
        Ok(results)
    }

    /// Get participants by topic/interest with storage configurations
    #[cfg(all(feature = "database", feature = "cache"))]
    pub async fn get_participants_by_topic(&self, topic: &str) -> Result<Vec<ParticipantProfile>> {
        let cache_key = format!("topic:{}", topic);
        
        // Try cache first
        if let Ok(Some(cached_ids)) = self.cache.get_cached::<Vec<String>>(&cache_key).await {
            if let Ok(Some(result)) = self.get_participant(&cached_ids[0]).await {
                return Ok(vec![result]);
            }
        }
        
        let results = self.query_participants_by_topic(topic).await?;
        
        // Cache the results
        let result_ids: Vec<String> = results.iter().map(|p| p.global_id.clone()).collect();
        let _ = self.cache.cache_participant(&cache_key, &result_ids, 600).await;
        
        Ok(results)
    }

    #[cfg(all(feature = "database", feature = "cache"))]
    async fn query_participants_by_topic(&self, topic: &str) -> Result<Vec<ParticipantProfile>> {
        let sql_query = format!(
            "SELECT * FROM participants WHERE topic_subscriptions @> '[\"{}\"]' LIMIT 50",
            topic.replace("\"", "\\\"")
        );
        
        self.database.query_participants(&sql_query, &[]).await
            .context("Failed to query participants by topic")
    }

    /// Get participants by topic with database only
    #[cfg(all(feature = "database", not(feature = "cache")))]
    pub async fn get_participants_by_topic(&self, topic: &str) -> Result<Vec<ParticipantProfile>> {
        self.query_participants_by_topic(topic).await
    }

    #[cfg(all(feature = "database", not(feature = "cache")))]
    async fn query_participants_by_topic(&self, topic: &str) -> Result<Vec<ParticipantProfile>> {
        let sql_query = format!(
            "SELECT * FROM participants WHERE topic_subscriptions @> '[\"{}\"]' LIMIT 50",
            topic.replace("\"", "\\\"")
        );
        
        self.database.query_participants(&sql_query, &[]).await
            .context("Failed to query participants by topic")
    }

    /// Get participants by topic with no storage
    #[cfg(not(feature = "database"))]
    pub async fn get_participants_by_topic(&self, _topic: &str) -> Result<Vec<ParticipantProfile>> {
        Ok(Vec::new())
    }

    /// Get participant by alias
    #[cfg(feature = "database")]
    pub async fn get_participant_by_alias(&self, alias: &str) -> Result<Option<ParticipantProfile>> {
        if let Some(profile) = self.get_participant(alias).await? {
            return Ok(Some(profile));
        }

        let sql_query = r#"
            SELECT * FROM participants
            WHERE EXISTS (
                SELECT 1 FROM jsonb_array_elements(identities) AS identity
                WHERE 
                    (identity->>'name' = $1 OR
                     identity->>'email_address' = $1)
            )
            LIMIT 1
        "#;
        
        let mut results = self.database.query_participants(sql_query, &[&alias]).await
            .context("Failed to query participant by alias")?;
            
        Ok(results.pop())
    }

    #[cfg(not(feature = "database"))]
    pub async fn get_participant_by_alias(&self, _alias: &str) -> Result<Option<ParticipantProfile>> {
        Ok(None)
    }

    /// Update participant's last seen timestamp
    #[cfg(all(feature = "database", feature = "cache"))]
    pub async fn update_last_seen(&self, global_id: &str) -> Result<()> {
        if let Some(mut profile) = self.get_participant(global_id).await? {
            profile.last_seen = Utc::now();
            profile.updated_at = Utc::now();
            
            self.database.upsert_participant(&profile).await?;
            
            let cache_key = format!("participant:{}", global_id);
            let _ = self.cache.invalidate(&cache_key).await;
        }
        Ok(())
    }

    #[cfg(all(feature = "database", not(feature = "cache")))]
    pub async fn update_last_seen(&self, global_id: &str) -> Result<()> {
        if let Some(mut profile) = self.get_participant(global_id).await? {
            profile.last_seen = DateTimeWrapper::new(Utc::now());
            profile.updated_at = DateTimeWrapper::new(Utc::now());
            self.database.upsert_participant(&profile).await?;
        }
        Ok(())
    }

    #[cfg(not(feature = "database"))]
    pub async fn update_last_seen(&self, _global_id: &str) -> Result<()> {
        Ok(())
    }
    
    /// Validate a participant profile
    fn validate_profile(&self, profile: &ParticipantProfile) -> Result<()> {
        if profile.global_id.is_empty() {
            return Err(anyhow::anyhow!("Global ID cannot be empty"));
        }
        
        if profile.display_name.is_empty() {
            return Err(anyhow::anyhow!("Display name cannot be empty"));
        }
        
        if profile.identities.is_empty() {
            return Err(anyhow::anyhow!("At least one identity context is required"));
        }
        
        // Validate email addresses in identities
        for identity in &profile.identities {
            if let Some(ref email) = identity.email_address {
                if !email.contains('@') {
                    return Err(anyhow::anyhow!("Invalid email address: {}", email));
                }
            }
        }
        
        Ok(())
    }
    
    /// Apply privacy filters to search results
    async fn apply_privacy_filters(
        &self,
        mut results: Vec<ParticipantProfile>,
        query: &ContactSearchQuery,
    ) -> Result<Vec<ParticipantProfile>> {
        results.retain(|profile| {
            match profile.discovery_permissions.discoverability {
                DiscoverabilityLevel::Public => true,
                DiscoverabilityLevel::Unlisted => {
                    // Unlisted can be found with hints or referrals
                    !query.hints.is_empty() || query.has_referral
                }
                DiscoverabilityLevel::Private => {
                    // Private requires exact match or pre-authorization
                    profile.global_id == query.query || 
                    query.requester_id == profile.global_id ||
                    query.has_referral
                }
                DiscoverabilityLevel::Stealth => {
                    // Stealth requires pre-authorization
                    query.has_referral
                }
            }
        });
        
        Ok(results)
    }

    /// Apply trust-based filters to search results
    async fn apply_trust_filters(
        &self,
        mut results: Vec<ParticipantProfile>,
        query: &ContactSearchQuery,
    ) -> Result<Vec<ParticipantProfile>> {
        if let Some(min_trust) = query.min_trust_score {
            let mut filtered_results = Vec::new();
            
            for profile in results {
                let trust_score = self.trust_manager.get_trust_score(
                    &profile.global_id,
                    &query.requester_id,
                ).await.unwrap_or(0.0);
                
                if trust_score >= min_trust {
                    filtered_results.push(profile);
                }
            }
            
            results = filtered_results;
        }
        
        Ok(results)
    }

    /// Generate hash for search query caching
    fn generate_search_hash(&self, query: &ContactSearchQuery) -> String {
        use sha2::{Digest, Sha256};
        
        let mut hasher = Sha256::new();
        hasher.update(&query.query);
        hasher.update(&query.requester_id);
        hasher.update(query.max_results.to_be_bytes());
        
        for hint in &query.hints {
            hasher.update(hint);
        }
        
        if let Some(trust) = query.min_trust_score {
            hasher.update(trust.to_be_bytes());
        }
        
        format!("{:x}", hasher.finalize())
    }
}

/// Search query for participant discovery
#[derive(Debug, Clone)]
pub struct ContactSearchQuery {
    pub query: String,
    pub requester_id: String,
    pub hints: Vec<String>,
    pub max_results: usize,
    pub min_trust_score: Option<f64>,
    pub has_referral: bool,
}

impl Default for ContactSearchQuery {
    fn default() -> Self {
        Self {
            query: String::new(),
            requester_id: String::new(),
            hints: vec![],
            max_results: 50,
            min_trust_score: None,
            has_referral: false,
        }
    }
}

/// Type alias for compatibility with API
pub type RegistryService = ParticipantRegistry;



