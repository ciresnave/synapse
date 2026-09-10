// SPDX-License-Identifier: MIT OR Apache-2.0
use crate::blockchain::serialization::DateTimeWrapper;
use crate::synapse::models::ParticipantProfile;
use crate::synapse::models::TrustBalance;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

/// Advanced search results with metadata and facets
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResults {
    pub participants: Vec<ParticipantProfile>,
    pub total_count: usize,
    pub facets: SearchFacets,
    pub query_time_ms: u64,
}

/// Search facets for filtering and navigation
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchFacets {
    pub entity_types: Vec<(String, usize)>, // (type, count)
    pub topics: Vec<(String, usize)>,
    pub organizations: Vec<(String, usize)>,
    pub availability_status: Vec<(String, usize)>,
}

/// Main database interface for Synapse
pub struct Database {
    pub pool: PgPool,
}

impl Database {
    /// Retrieve a single participant profile by global_id
    pub async fn get_participant_profile(&self, global_id: &str) -> Result<ParticipantProfile> {
        let query = r#"
            SELECT global_id, display_name, entity_type, identities,
                   discovery_permissions, availability, contact_preferences,
                   trust_ratings, topic_subscriptions, organizational_context,
                   public_key, supported_protocols, last_seen, created_at, updated_at
            FROM participants WHERE global_id = $1
        "#;

        let row = sqlx::query(query)
            .bind(global_id)
            .fetch_one(&self.pool)
            .await
            .context("Failed to fetch participant profile")?;

        let profile = ParticipantProfile {
            global_id: row.get("global_id"),
            display_name: row.get("display_name"),
            entity_type: serde_json::from_value(row.get("entity_type"))?,
            identities: serde_json::from_value(row.get("identities"))?,
            discovery_permissions: serde_json::from_value(row.get("discovery_permissions"))?,
            availability: serde_json::from_value(row.get("availability"))?,
            contact_preferences: serde_json::from_value(row.get("contact_preferences"))?,
            trust_ratings: serde_json::from_value(row.get("trust_ratings"))?,
            relationships: vec![], // Loaded separately if needed
            topic_subscriptions: serde_json::from_value(row.get("topic_subscriptions"))?,
            organizational_context: serde_json::from_value(row.get("organizational_context"))?,
            public_key: row.get("public_key"),
            supported_protocols: serde_json::from_value(row.get("supported_protocols"))?,
            last_seen: row.get("last_seen"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            metadata: dashmap::DashMap::new(),
        };
        Ok(profile)
    }
    /// Retrieve all participant profiles from the database
    pub async fn get_all_participants(&self) -> Result<Vec<ParticipantProfile>> {
        let query = r#"
            SELECT global_id, display_name, entity_type, identities,
                   discovery_permissions, availability, contact_preferences,
                   trust_ratings, topic_subscriptions, organizational_context,
                   public_key, supported_protocols, last_seen, created_at, updated_at
            FROM participants
        "#;

        let rows = sqlx::query(query)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch all participants")?;

        let mut results = Vec::new();
        for row in rows {
            let profile = ParticipantProfile {
                global_id: row.get("global_id"),
                display_name: row.get("display_name"),
                entity_type: serde_json::from_value(row.get("entity_type"))?,
                identities: serde_json::from_value(row.get("identities"))?,
                discovery_permissions: serde_json::from_value(row.get("discovery_permissions"))?,
                availability: serde_json::from_value(row.get("availability"))?,
                contact_preferences: serde_json::from_value(row.get("contact_preferences"))?,
                trust_ratings: serde_json::from_value(row.get("trust_ratings"))?,
                relationships: vec![], // Loaded separately if needed
                topic_subscriptions: serde_json::from_value(row.get("topic_subscriptions"))?,
                organizational_context: serde_json::from_value(row.get("organizational_context"))?,
                public_key: row.get("public_key"),
                supported_protocols: serde_json::from_value(row.get("supported_protocols"))?,
                last_seen: row.get("last_seen"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                metadata: dashmap::DashMap::new(),
            };
            results.push(profile);
        }
        Ok(results)
    }
    /// Create new database connection
    pub async fn new(database_url: &str) -> Result<Self> {
        let pool = PgPool::connect(database_url)
            .await
            .context("Failed to connect to database")?;

        // Run migrations
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .context("Failed to run database migrations")?;

        Ok(Self { pool })
    }

    /// Store or update a participant profile
    pub async fn upsert_participant(&self, profile: &ParticipantProfile) -> Result<()> {
        let query = r#"
            INSERT INTO participants (
                global_id, display_name, entity_type, identities,
                discovery_permissions, availability, contact_preferences,
                trust_ratings, topic_subscriptions, organizational_context,
                public_key, supported_protocols, last_seen, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            ON CONFLICT (global_id) DO UPDATE SET
                display_name = EXCLUDED.display_name,
                entity_type = EXCLUDED.entity_type,
                identities = EXCLUDED.identities,
                discovery_permissions = EXCLUDED.discovery_permissions,
                availability = EXCLUDED.availability,
                contact_preferences = EXCLUDED.contact_preferences,
                trust_ratings = EXCLUDED.trust_ratings,
                topic_subscriptions = EXCLUDED.topic_subscriptions,
                organizational_context = EXCLUDED.organizational_context,
                public_key = EXCLUDED.public_key,
                supported_protocols = EXCLUDED.supported_protocols,
                last_seen = EXCLUDED.last_seen,
                updated_at = EXCLUDED.updated_at
        "#;

        sqlx::query(query)
            .bind(&profile.global_id)
            .bind(&profile.display_name)
            .bind(serde_json::to_value(&profile.entity_type)?)
            .bind(serde_json::to_value(&profile.identities)?)
            .bind(serde_json::to_value(&profile.discovery_permissions)?)
            .bind(serde_json::to_value(&profile.availability)?)
            .bind(serde_json::to_value(&profile.contact_preferences)?)
            .bind(serde_json::to_value(&profile.trust_ratings)?)
            .bind(serde_json::to_value(&profile.topic_subscriptions)?)
            .bind(serde_json::to_value(&profile.organizational_context)?)
            .bind(&profile.public_key)
            .bind(serde_json::to_value(&profile.supported_protocols)?)
            .bind(profile.last_seen)
            .bind(profile.created_at)
            .bind(profile.updated_at)
            .execute(&self.pool)
            .await
            .context("Failed to upsert participant")?;

        Ok(())
    }

    /// Retrieve a participant by global ID
    pub async fn get_participant(&self, global_id: &str) -> Result<Option<ParticipantProfile>> {
        let query = r#"
            SELECT global_id, display_name, entity_type, identities,
                   discovery_permissions, availability, contact_preferences,
                   trust_ratings, topic_subscriptions, organizational_context,
                   public_key, supported_protocols, last_seen, created_at, updated_at
            FROM participants
            WHERE global_id = $1
        "#;

        let row = sqlx::query(query)
            .bind(global_id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch participant")?;

        match row {
            Some(row) => {
                let profile = ParticipantProfile {
                    global_id: row.get("global_id"),
                    display_name: row.get("display_name"),
                    entity_type: serde_json::from_value(row.get("entity_type"))?,
                    identities: serde_json::from_value(row.get("identities"))?,
                    discovery_permissions: serde_json::from_value(
                        row.get("discovery_permissions"),
                    )?,
                    availability: serde_json::from_value(row.get("availability"))?,
                    contact_preferences: serde_json::from_value(row.get("contact_preferences"))?,
                    trust_ratings: serde_json::from_value(row.get("trust_ratings"))?,
                    relationships: vec![], // Loaded separately if needed
                    topic_subscriptions: serde_json::from_value(row.get("topic_subscriptions"))?,
                    organizational_context: serde_json::from_value(
                        row.get("organizational_context"),
                    )?,
                    public_key: row.get("public_key"),
                    supported_protocols: serde_json::from_value(row.get("supported_protocols"))?,
                    last_seen: row.get("last_seen"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                    metadata: dashmap::DashMap::new(),
                };
                Ok(Some(profile))
            }
            None => Ok(None),
        }
    }

    /// Search participants with privacy filtering
    pub async fn search_participants(
        &self,
        query: &str,
        _requester_id: &str,
        max_results: usize,
    ) -> Result<Vec<ParticipantProfile>> {
        // This is a simplified search - in production we'd use full-text search
        let sql_query = r#"
            SELECT global_id, display_name, entity_type, identities,
                   discovery_permissions, availability, contact_preferences,
                   trust_ratings, topic_subscriptions, organizational_context,
                   public_key, supported_protocols, last_seen, created_at, updated_at
            FROM participants
            WHERE (display_name ILIKE $1 OR global_id ILIKE $1)
            AND (discovery_permissions->>'discoverability' = 'Public'
                 OR discovery_permissions->>'discoverability' = 'Unlisted')
            LIMIT $2
        "#;

        let rows = sqlx::query(sql_query)
            .bind(format!("%{query}%"))
            .bind(max_results as i64)
            .fetch_all(&self.pool)
            .await
            .context("Failed to search participants")?;

        let mut results = Vec::new();
        for row in rows {
            let profile = ParticipantProfile {
                global_id: row.get("global_id"),
                display_name: row.get("display_name"),
                entity_type: serde_json::from_value(row.get("entity_type"))?,
                identities: serde_json::from_value(row.get("identities"))?,
                discovery_permissions: serde_json::from_value(row.get("discovery_permissions"))?,
                availability: serde_json::from_value(row.get("availability"))?,
                contact_preferences: serde_json::from_value(row.get("contact_preferences"))?,
                trust_ratings: serde_json::from_value(row.get("trust_ratings"))?,
                relationships: vec![], // Loaded separately if needed
                topic_subscriptions: serde_json::from_value(row.get("topic_subscriptions"))?,
                organizational_context: serde_json::from_value(row.get("organizational_context"))?,
                public_key: row.get("public_key"),
                supported_protocols: serde_json::from_value(row.get("supported_protocols"))?,
                last_seen: row.get("last_seen"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                metadata: dashmap::DashMap::new(),
            };
            results.push(profile);
        }

        Ok(results)
    }

    /// Store trust balance for a participant
    pub async fn upsert_trust_balance(&self, balance: &TrustBalance) -> Result<()> {
        let query = r#"
            INSERT INTO trust_balances (
                participant_id, total_points, available_points, staked_points,
                earned_lifetime, last_activity, decay_rate
            ) VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (participant_id) DO UPDATE SET
                total_points = EXCLUDED.total_points,
                available_points = EXCLUDED.available_points,
                staked_points = EXCLUDED.staked_points,
                earned_lifetime = EXCLUDED.earned_lifetime,
                last_activity = EXCLUDED.last_activity,
                decay_rate = EXCLUDED.decay_rate
        "#;

        sqlx::query(query)
            .bind(&balance.participant_id)
            .bind(balance.total_points as i32)
            .bind(balance.available_points as i32)
            .bind(balance.staked_points as i32)
            .bind(balance.earned_lifetime as i32)
            .bind(Some(balance.last_activity.clone().into_inner()))
            .bind(balance.decay_rate)
            .execute(&self.pool)
            .await
            .context("Failed to upsert trust balance")?;

        Ok(())
    }

    /// Get trust balance for a participant
    pub async fn get_trust_balance(&self, participant_id: &str) -> Result<Option<TrustBalance>> {
        let query = r#"
            SELECT participant_id, total_points, available_points, staked_points,
                   earned_lifetime, last_activity, decay_rate
            FROM trust_balances
            WHERE participant_id = $1
        "#;

        let row = sqlx::query(query)
            .bind(participant_id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch trust balance")?;

        match row {
            Some(row) => {
                let balance = TrustBalance {
                    participant_id: row.get("participant_id"),
                    total_points: row.get::<i32, _>("total_points") as u32,
                    available_points: row.get::<i32, _>("available_points") as u32,
                    staked_points: row.get::<i32, _>("staked_points") as u32,
                    earned_lifetime: row.get::<i32, _>("earned_lifetime") as u32,
                    last_activity: DateTimeWrapper::new(row.get("last_activity")),
                    decay_rate: row.get("decay_rate"),
                };
                Ok(Some(balance))
            }
            None => Ok(None),
        }
    }

    /// Execute a custom SQL query to retrieve participants
    pub async fn query_participants(
        &self,
        query: &str,
        params: &[&str],
    ) -> Result<Vec<ParticipantProfile>> {
        let mut query_builder = sqlx::query(query);
        for param in params {
            query_builder = query_builder.bind(*param);
        }

        let rows = query_builder
            .fetch_all(&self.pool)
            .await
            .context("Failed to execute participant query")?;

        let mut results = Vec::new();
        for row in rows {
            let profile = ParticipantProfile {
                global_id: row.get("global_id"),
                display_name: row.get("display_name"),
                entity_type: serde_json::from_value(row.get("entity_type"))?,
                identities: serde_json::from_value(row.get("identities"))?,
                discovery_permissions: serde_json::from_value(row.get("discovery_permissions"))?,
                availability: serde_json::from_value(row.get("availability"))?,
                contact_preferences: serde_json::from_value(row.get("contact_preferences"))?,
                trust_ratings: serde_json::from_value(row.get("trust_ratings"))?,
                relationships: vec![], // Loaded separately if needed
                topic_subscriptions: serde_json::from_value(row.get("topic_subscriptions"))?,
                organizational_context: serde_json::from_value(row.get("organizational_context"))?,
                public_key: row.get("public_key"),
                supported_protocols: serde_json::from_value(row.get("supported_protocols"))?,
                last_seen: row.get("last_seen"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                metadata: dashmap::DashMap::new(),
            };
            results.push(profile);
        }

        Ok(results)
    }

    /// Get all trust balances that need decay processing
    pub async fn get_balances_for_decay(
        &self,
        cutoff_time: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<TrustBalance>> {
        let query = r#"
            SELECT participant_id, total_points, available_points, staked_points,
                   earned_lifetime, last_activity, decay_rate
            FROM trust_balances
            WHERE last_activity < $1 AND total_points > 0
        "#;

        let rows = sqlx::query(query)
            .bind(Some(cutoff_time))
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch balances for decay")?;

        let mut results = Vec::new();
        for row in rows {
            let balance = TrustBalance {
                participant_id: row.get("participant_id"),
                total_points: row.get::<i32, _>("total_points") as u32,
                available_points: row.get::<i32, _>("available_points") as u32,
                staked_points: row.get::<i32, _>("staked_points") as u32,
                earned_lifetime: row.get::<i32, _>("earned_lifetime") as u32,
                last_activity: DateTimeWrapper::new(row.get("last_activity")),
                decay_rate: row.get("decay_rate"),
            };
            results.push(balance);
        }

        Ok(results)
    }

    /// Search participants by name
    pub async fn search_participants_by_name(
        &self,
        query: &str,
        requester_id: &str,
        max_results: usize,
    ) -> Result<Vec<ParticipantProfile>> {
        tracing::debug!(
            "Searching participants by name: '{}' for requester: {}",
            query,
            requester_id
        );

        let sql_query = r#"
            SELECT global_id, display_name, entity_type, identities,
                   discovery_permissions, availability, contact_preferences,
                   trust_ratings, topic_subscriptions, organizational_context,
                   public_key, supported_protocols, last_seen, created_at, updated_at
            FROM participants
            WHERE (display_name ILIKE $1 OR global_id ILIKE $1)
            AND global_id != $2
            LIMIT $3
        "#;

        let search_pattern = format!("%{}%", query);
        let rows = sqlx::query(sql_query)
            .bind(&search_pattern)
            .bind(requester_id)
            .bind(max_results as i64)
            .fetch_all(&self.pool)
            .await
            .with_context(|| format!("Failed to search participants by name: {}", query))?;

        let mut participants = Vec::new();
        for row in rows {
            let profile = ParticipantProfile {
                global_id: row.get("global_id"),
                display_name: row.get("display_name"),
                entity_type: serde_json::from_value(row.get("entity_type"))?,
                identities: serde_json::from_value(row.get("identities"))?,
                discovery_permissions: serde_json::from_value(row.get("discovery_permissions"))?,
                availability: serde_json::from_value(row.get("availability"))?,
                contact_preferences: serde_json::from_value(row.get("contact_preferences"))?,
                trust_ratings: serde_json::from_value(row.get("trust_ratings"))?,
                relationships: vec![], // Loaded separately if needed
                topic_subscriptions: serde_json::from_value(row.get("topic_subscriptions"))?,
                organizational_context: serde_json::from_value(row.get("organizational_context"))?,
                public_key: row.get("public_key"),
                supported_protocols: serde_json::from_value(row.get("supported_protocols"))?,
                last_seen: row.get("last_seen"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                metadata: dashmap::DashMap::new(),
            };
            participants.push(profile);
        }

        tracing::debug!(
            "Found {} participants matching name query",
            participants.len()
        );
        Ok(participants)
    }

    /// Search participants by capabilities
    pub async fn search_participants_by_capabilities(
        &self,
        capabilities: &[String],
        requester_id: &str,
        max_results: usize,
    ) -> Result<Vec<ParticipantProfile>> {
        tracing::debug!(
            "Searching participants by capabilities: {:?} for requester: {}",
            capabilities,
            requester_id
        );

        if capabilities.is_empty() {
            return Ok(Vec::new());
        }

        // Build dynamic query for capability search
        let capability_conditions: Vec<String> = capabilities
            .iter()
            .enumerate()
            .map(|(i, _)| format!("supported_protocols::text ILIKE ${}", i + 3))
            .collect();

        let sql_query = format!(
            r#"
            SELECT global_id, display_name, entity_type, identities,
                   discovery_permissions, availability, contact_preferences,
                   trust_ratings, topic_subscriptions, organizational_context,
                   public_key, supported_protocols, last_seen, created_at, updated_at
            FROM participants
            WHERE ({})
            AND global_id != $1
            LIMIT $2
            "#,
            capability_conditions.join(" OR ")
        );

        let mut query_builder = sqlx::query(&sql_query)
            .bind(requester_id)
            .bind(max_results as i64);

        for capability in capabilities {
            let pattern = format!("%{}%", capability);
            query_builder = query_builder.bind(pattern);
        }

        let rows = query_builder.fetch_all(&self.pool).await.with_context(|| {
            format!(
                "Failed to search participants by capabilities: {:?}",
                capabilities
            )
        })?;

        let mut participants = Vec::new();
        for row in rows {
            let profile = ParticipantProfile {
                global_id: row.get("global_id"),
                display_name: row.get("display_name"),
                entity_type: serde_json::from_value(row.get("entity_type"))?,
                identities: serde_json::from_value(row.get("identities"))?,
                discovery_permissions: serde_json::from_value(row.get("discovery_permissions"))?,
                availability: serde_json::from_value(row.get("availability"))?,
                contact_preferences: serde_json::from_value(row.get("contact_preferences"))?,
                trust_ratings: serde_json::from_value(row.get("trust_ratings"))?,
                relationships: vec![], // Loaded separately if needed
                topic_subscriptions: serde_json::from_value(row.get("topic_subscriptions"))?,
                organizational_context: serde_json::from_value(row.get("organizational_context"))?,
                public_key: row.get("public_key"),
                supported_protocols: serde_json::from_value(row.get("supported_protocols"))?,
                last_seen: row.get("last_seen"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                metadata: dashmap::DashMap::new(),
            };
            participants.push(profile);
        }

        tracing::debug!(
            "Found {} participants with matching capabilities",
            participants.len()
        );
        Ok(participants)
    }

    /// Search participants by organization
    pub async fn search_participants_by_organization(
        &self,
        organization: &str,
        requester_id: &str,
        max_results: usize,
    ) -> Result<Vec<ParticipantProfile>> {
        tracing::debug!(
            "Searching participants by organization: '{}' for requester: {}",
            organization,
            requester_id
        );

        let sql_query = r#"
            SELECT global_id, display_name, entity_type, identities,
                   discovery_permissions, availability, contact_preferences,
                   trust_ratings, topic_subscriptions, organizational_context,
                   public_key, supported_protocols, last_seen, created_at, updated_at
            FROM participants
            WHERE (global_id ILIKE $1 OR organizational_context::text ILIKE $1)
            AND global_id != $2
            LIMIT $3
        "#;

        let org_pattern = format!("%{}%", organization);
        let rows = sqlx::query(sql_query)
            .bind(&org_pattern)
            .bind(requester_id)
            .bind(max_results as i64)
            .fetch_all(&self.pool)
            .await
            .with_context(|| {
                format!(
                    "Failed to search participants by organization: {}",
                    organization
                )
            })?;

        let mut participants = Vec::new();
        for row in rows {
            let profile = ParticipantProfile {
                global_id: row.get("global_id"),
                display_name: row.get("display_name"),
                entity_type: serde_json::from_value(row.get("entity_type"))?,
                identities: serde_json::from_value(row.get("identities"))?,
                discovery_permissions: serde_json::from_value(row.get("discovery_permissions"))?,
                availability: serde_json::from_value(row.get("availability"))?,
                contact_preferences: serde_json::from_value(row.get("contact_preferences"))?,
                trust_ratings: serde_json::from_value(row.get("trust_ratings"))?,
                relationships: vec![], // Loaded separately if needed
                topic_subscriptions: serde_json::from_value(row.get("topic_subscriptions"))?,
                organizational_context: serde_json::from_value(row.get("organizational_context"))?,
                public_key: row.get("public_key"),
                supported_protocols: serde_json::from_value(row.get("supported_protocols"))?,
                last_seen: row.get("last_seen"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                metadata: dashmap::DashMap::new(),
            };
            participants.push(profile);
        }

        tracing::debug!(
            "Found {} participants from organization",
            participants.len()
        );
        Ok(participants)
    }

    /// Search participants by proximity in trust network
    pub async fn search_participants_by_proximity(
        &self,
        _requester_id: &str,
        _max_distance: f64,
        _max_results: usize,
    ) -> Result<Vec<ParticipantProfile>> {
        // Placeholder implementation
        Ok(vec![])
    }

    /// Get discovery recommendations
    pub async fn get_discovery_recommendations(
        &self,
        _requester_id: &str,
        _max_results: usize,
    ) -> Result<Vec<ParticipantProfile>> {
        // Placeholder implementation
        Ok(vec![])
    }

    /// Delete participant
    pub async fn delete_participant(&self, participant_id: &str) -> Result<()> {
        let query = "DELETE FROM participants WHERE global_id = $1";

        sqlx::query(query)
            .bind(participant_id)
            .execute(&self.pool)
            .await
            .context("Failed to delete participant")?;

        Ok(())
    }

    /// Count reports submitted by a reporter since a specific time
    pub async fn count_reports_since(
        &self,
        reporter_id: &str,
        since: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64> {
        let query = r#"
            SELECT COUNT(*) as report_count
            FROM trust_reports
            WHERE reporter_id = $1
            AND timestamp > $2
        "#;

        let row = sqlx::query(query)
            .bind(reporter_id)
            .bind(Some(since))
            .fetch_one(&self.pool)
            .await
            .context("Failed to query recent reports")?;

        let count: i64 = row.get("report_count");
        Ok(count as u64)
    }

    /// Check if two participants have direct message interactions
    pub async fn has_direct_interaction(&self, user_a: &str, user_b: &str) -> Result<bool> {
        // Check for direct messages exchanged
        let query = r#"
            SELECT COUNT(*) as message_count
            FROM messages
            WHERE (sender_id = $1 AND recipient_id = $2)
            OR (sender_id = $2 AND recipient_id = $1)
        "#;

        let row = sqlx::query(query)
            .bind(user_a)
            .bind(user_b)
            .fetch_one(&self.pool)
            .await
            .context("Failed to query message history")?;

        let count: i64 = row.get("message_count");
        Ok(count > 0)
    }

    /// Check if two participants have been in same network events
    pub async fn has_shared_events(&self, user_a: &str, user_b: &str) -> Result<bool> {
        // Check for participation in same network events
        let query = r#"
            SELECT COUNT(*) as event_count
            FROM network_events
            WHERE $1 = ANY(participant_ids) AND $2 = ANY(participant_ids)
        "#;

        let row = sqlx::query(query)
            .bind(user_a)
            .bind(user_b)
            .fetch_one(&self.pool)
            .await
            .context("Failed to query event history")?;

        let count: i64 = row.get("event_count");
        Ok(count > 0)
    }

    /// Record a new trust report submission
    pub async fn record_trust_report(
        &self,
        reporter_id: &str,
        subject_id: &str,
        score: i8,
        category: &str,
        transaction_id: &str,
        timestamp: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        let query = r#"
            INSERT INTO trust_reports (
                reporter_id, subject_id, score, category,
                transaction_id, timestamp
            ) VALUES ($1, $2, $3, $4, $5, $6)
        "#;

        sqlx::query(query)
            .bind(reporter_id)
            .bind(subject_id)
            .bind(score as i16)
            .bind(category)
            .bind(transaction_id)
            .bind(Some(timestamp))
            .execute(&self.pool)
            .await
            .context("Failed to record trust report")?;

        Ok(())
    }

    /// Execute a raw SQL query with parameters for strings
    pub async fn query_raw_string(
        &self,
        query: &str,
        params: &[&str],
    ) -> Result<Vec<sqlx::postgres::PgRow>> {
        let mut query_builder = sqlx::query(query);
        for param in params {
            query_builder = query_builder.bind(*param);
        }

        let rows = query_builder
            .fetch_all(&self.pool)
            .await
            .context("Failed to execute raw query")?;

        Ok(rows)
    }

    /// Count all participants in the system
    pub async fn count_all_participants(&self) -> Result<u64> {
        let row: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM participants")
            .fetch_one(&self.pool)
            .await
            .context("Failed to count participants")?;
        Ok(row as u64)
    }

    /// Count participants active in the last 24 hours
    pub async fn count_active_participants_24h(&self) -> Result<u64> {
        let row: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM participants WHERE last_seen > NOW() - INTERVAL '24 hours'",
        )
        .fetch_one(&self.pool)
        .await
        .context("Failed to count active participants")?;
        Ok(row as u64)
    }

    /// Count new participants registered today
    pub async fn count_new_participants_today(&self) -> Result<u64> {
        let row: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM participants WHERE created_at > CURRENT_DATE")
                .fetch_one(&self.pool)
                .await
                .context("Failed to count new participants")?;
        Ok(row as u64)
    }

    /// Count trust reports submitted today
    pub async fn count_trust_reports_today(&self) -> Result<u64> {
        let row: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM trust_reports WHERE created_at > CURRENT_DATE",
        )
        .fetch_one(&self.pool)
        .await
        .context("Failed to count trust reports")?;
        Ok(row as u64)
    }

    /// Get average trust score across all participants
    pub async fn get_average_trust_score(&self) -> Result<f64> {
        let row: Option<f64> = sqlx::query_scalar(
            "SELECT AVG(trust_score) FROM participants WHERE trust_score IS NOT NULL",
        )
        .fetch_one(&self.pool)
        .await
        .context("Failed to get average trust score")?;
        Ok(row.unwrap_or(50.0))
    }
}
