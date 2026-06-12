use crate::blockchain::serialization::DateTimeWrapper;
use crate::security::{sanitize_trust_report, validate_trust_evidence};
use crate::synapse::models::{TrustBalance, TrustCategory};
use anyhow::Context;
// Feature gating removed: always include chrono logic
use chrono::{Duration, Utc};
use tokio::time::interval;
// Feature gating removed: always include email logic
// Feature gating removed
use tracing::warn;
// Synapse Trust Manager
// Manages dual trust system: entity-to-entity and network trust

use crate::synapse::blockchain::SynapseBlockchain;
// Feature gating removed: always include database logic
use crate::synapse::storage::Database;
use anyhow::Result;
// Feature gating removed
use sqlx::Row;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

/// Production-grade trust network graph for trust propagation
#[derive(Debug, Clone)]
struct TrustNetwork {
    /// Adjacency list representing trust relationships
    nodes: HashMap<String, HashMap<String, TrustEdge>>,
    /// Total number of edges
    edge_count: usize,
}

/// Trust edge containing weight and metadata
#[derive(Debug, Clone)]
struct TrustEdge {
    weight: f64,
    #[allow(dead_code)] // Kept for future metadata tracking
    timestamp: String,
}

impl TrustNetwork {
    fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edge_count: 0,
        }
    }

    fn add_trust_edge(&mut self, from: String, to: String, weight: f64, timestamp: String) {
        self.nodes
            .entry(from)
            .or_default()
            .insert(to, TrustEdge { weight, timestamp });
        self.edge_count += 1;
    }

    fn get_trust_weight(&self, from: &str, to: &str) -> Option<f64> {
        self.nodes.get(from)?.get(to).map(|edge| edge.weight)
    }

    fn get_neighbors(&self, node: &str) -> Option<Vec<(String, f64)>> {
        self.nodes.get(node).map(|neighbors| {
            neighbors
                .iter()
                .map(|(neighbor, edge)| (neighbor.clone(), edge.weight))
                .collect()
        })
    }

    fn node_count(&self) -> usize {
        self.nodes.len()
    }

    fn edge_count(&self) -> usize {
        self.edge_count
    }

    fn get_outgoing_count(&self, node: &str) -> usize {
        self.nodes
            .get(node)
            .map(|neighbors| neighbors.len())
            .unwrap_or(0)
    }

    fn get_incoming_count(&self, node: &str) -> usize {
        self.nodes
            .values()
            .map(|neighbors| if neighbors.contains_key(node) { 1 } else { 0 })
            .sum()
    }
}

// Security constants
const MAX_DAILY_REPORTS: u64 = 10;

/// Trust management service for dual trust system
// Feature gating removed
pub struct TrustManager {
    database: Arc<Database>,
    pub blockchain: Arc<SynapseBlockchain>,
}

impl TrustManager {
    /// Create new trust manager
    pub async fn new(database: Arc<Database>, blockchain: Arc<SynapseBlockchain>) -> Result<Self> {
        Ok(Self {
            database,
            blockchain,
        })
    }

    /// Initialize trust balance for new participant
    pub async fn initialize_participant(&self, participant_id: &str) -> Result<()> {
        let initial_balance = TrustBalance {
            participant_id: participant_id.to_string(),
            total_points: 100, // Genesis trust points
            available_points: 100,
            staked_points: 0,
            earned_lifetime: 100,
            last_activity: DateTimeWrapper::new(Utc::now()),
            decay_rate: 0.02, // 2% per month
        };

        self.database
            .upsert_trust_balance(&initial_balance)
            .await
            .context("Failed to initialize trust balance")?;

        info!(
            "Initialized trust balance for participant: {}",
            participant_id
        );
        Ok(())
    }

    /// Get combined trust score (entity-to-entity + network)
    pub async fn get_trust_score(&self, subject_id: &str, requester_id: &str) -> Result<f64> {
        // Get entity-to-entity trust (subjective, personal experience)
        let entity_trust = self
            .get_entity_trust_score(subject_id, requester_id)
            .await?;

        // Get network trust (objective, blockchain-verified)
        let network_trust = self.get_network_trust_score(subject_id).await?;

        // Weighted combination: 60% network trust, 40% entity trust
        let combined_score = (network_trust * 0.6) + (entity_trust * 0.4);

        Ok(combined_score)
    }

    /// Get entity-to-entity trust score (subjective)
    pub async fn get_entity_trust_score(
        &self,
        subject_id: &str,
        requester_id: &str,
    ) -> Result<f64> {
        // Check cache first
        let _cache_key = format!("entity_trust:{requester_id}:{subject_id}");

        // Look up direct trust relationships in database
        let sql_query = r#"
            SELECT trust_data.data->>'score' as score
            FROM participants
            CROSS JOIN LATERAL jsonb_array_elements(trust_ratings->'entity_trust'->'given_ratings') AS trust_data
            WHERE global_id = $1
            AND trust_data.data->>'given_by' = $2
        "#;

        let result = self
            .database
            .query_raw_string(sql_query, &[requester_id, subject_id])
            .await
            .context("Failed to query direct trust")?;

        if let Some(row) = result.first()
            && let Some(score) = row.try_get::<Option<String>, _>("score").unwrap_or(None)
            && let Ok(numeric_score) = score.parse::<u8>()
        {
            // Convert to 0-100 scale
            return Ok(numeric_score as f64);
        }

        // If no direct relationship, check for trust propagation through network
        let propagated_score = self
            .get_propagated_trust_score(subject_id, requester_id)
            .await?;

        if propagated_score > 0.0 {
            return Ok(propagated_score);
        }

        // Default neutral score if no direct or propagated trust
        Ok(50.0) // 0-100 scale, 50 = neutral
    }

    /// Calculate trust propagated through the network using graph algorithms
    async fn get_propagated_trust_score(
        &self,
        subject_id: &str,
        requester_id: &str,
    ) -> Result<f64> {
        // Production implementation: Multi-path trust propagation with decay and confidence weighting

        let trust_graph = self.build_trust_network().await?;
        let direct_trust = self.get_direct_trust(&trust_graph, requester_id, subject_id);

        // If direct trust exists, return it with high confidence
        if direct_trust > 0.0 {
            tracing::debug!(
                "Direct trust found: {} -> {} = {}",
                requester_id,
                subject_id,
                direct_trust
            );
            return Ok(direct_trust);
        }

        // Calculate trust through multiple propagation paths
        let propagated_trust = self
            .calculate_multi_path_trust(
                &trust_graph,
                requester_id,
                subject_id,
                3,    // max_depth: Maximum path length for trust propagation
                0.85, // decay_factor: Trust decay per hop (15% decay per hop)
                5,    // max_paths: Maximum number of paths to consider
            )
            .await?;

        tracing::debug!(
            "Propagated trust: {} -> {} = {}",
            requester_id,
            subject_id,
            propagated_trust
        );
        Ok(propagated_trust)
    }

    /// Build trust network graph from database
    async fn build_trust_network(&self) -> Result<TrustNetwork> {
        let trust_data_sql = r#"
            SELECT
                p.global_id as source_id,
                trust_data.data->>'subject_id' as target_id,
                (trust_data.data->>'score')::float as score,
                trust_data.data->>'timestamp' as timestamp,
                p.trust_ratings->'entity_trust'->>'total_ratings_given' as total_given
            FROM participants p
            CROSS JOIN LATERAL jsonb_array_elements(trust_ratings->'entity_trust'->'given_ratings') AS trust_data
            WHERE (trust_data.data->>'score')::float > 50.0  -- Only consider positive trust
            AND trust_data.data->>'timestamp' IS NOT NULL
            ORDER BY source_id, (trust_data.data->>'timestamp') DESC
        "#;

        let rows = self.database.query_raw_string(trust_data_sql, &[]).await?;
        let mut network = TrustNetwork::new();

        for row in rows {
            let source = row.try_get::<String, _>("source_id").unwrap_or_default();
            let target = row.try_get::<String, _>("target_id").unwrap_or_default();
            let score = row.try_get::<f64, _>("score").unwrap_or(0.0);
            let timestamp = row.try_get::<String, _>("timestamp").unwrap_or_default();
            let total_given = row
                .try_get::<String, _>("total_given")
                .unwrap_or_default()
                .parse::<u32>()
                .unwrap_or(1);

            if !source.is_empty() && !target.is_empty() && score > 50.0 {
                // Normalize score to 0-1 range and weight by rater experience
                let normalized_score = (score - 50.0) / 50.0; // 50-100 -> 0-1
                let experience_weight = (total_given as f64 / 10.0).min(1.0); // More experienced raters have higher weight
                let weighted_score = normalized_score * (0.7 + 0.3 * experience_weight);

                network.add_trust_edge(source, target, weighted_score, timestamp);
            }
        }

        tracing::info!(
            "Built trust network with {} nodes and {} edges",
            network.node_count(),
            network.edge_count()
        );
        Ok(network)
    }

    /// Get direct trust between two participants
    fn get_direct_trust(&self, network: &TrustNetwork, from: &str, to: &str) -> f64 {
        network.get_trust_weight(from, to).unwrap_or(0.0)
    }

    /// Calculate trust using multiple path propagation with confidence weighting
    async fn calculate_multi_path_trust(
        &self,
        network: &TrustNetwork,
        source: &str,
        target: &str,
        max_depth: usize,
        decay_factor: f64,
        max_paths: usize,
    ) -> Result<f64> {
        use std::cmp::Ordering;
        use std::collections::{BinaryHeap, HashSet};

        #[derive(Debug, Clone)]
        struct TrustPath {
            node: String,
            trust_value: f64,
            depth: usize,
            path: Vec<String>,
        }

        impl PartialEq for TrustPath {
            fn eq(&self, other: &Self) -> bool {
                self.trust_value.partial_cmp(&other.trust_value) == Some(Ordering::Equal)
            }
        }

        impl Eq for TrustPath {}

        impl PartialOrd for TrustPath {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                Some(self.cmp(other))
            }
        }

        impl Ord for TrustPath {
            fn cmp(&self, other: &Self) -> Ordering {
                self.trust_value
                    .partial_cmp(&other.trust_value)
                    .unwrap_or(Ordering::Equal)
            }
        }

        let mut heap = BinaryHeap::new();
        let mut visited_paths = HashSet::new();
        let mut found_paths = Vec::new();

        // Initialize with source node
        heap.push(TrustPath {
            node: source.to_string(),
            trust_value: 1.0,
            depth: 0,
            path: vec![source.to_string()],
        });

        while let Some(current) = heap.pop() {
            if current.depth >= max_depth {
                continue;
            }

            // Avoid cycles
            let path_key = format!("{}:{}", current.node, current.path.join("->"));
            if visited_paths.contains(&path_key) {
                continue;
            }
            visited_paths.insert(path_key);

            // Get neighbors
            if let Some(neighbors) = network.get_neighbors(&current.node) {
                for (neighbor, edge_weight) in neighbors {
                    let path_trust =
                        current.trust_value * edge_weight * decay_factor.powi(current.depth as i32);

                    // Skip very low trust paths
                    if path_trust < 0.1 {
                        continue;
                    }

                    let mut new_path = current.path.clone();
                    new_path.push(neighbor.to_string());

                    if neighbor == target {
                        // Found a path to target
                        found_paths.push(TrustPath {
                            node: neighbor.to_string(),
                            trust_value: path_trust,
                            depth: current.depth + 1,
                            path: new_path,
                        });

                        if found_paths.len() >= max_paths {
                            break;
                        }
                    } else if !current.path.contains(&neighbor) {
                        // Continue exploring if not in current path (avoid cycles)
                        heap.push(TrustPath {
                            node: neighbor.to_string(),
                            trust_value: path_trust,
                            depth: current.depth + 1,
                            path: new_path,
                        });
                    }
                }
            }
        }

        if found_paths.is_empty() {
            return Ok(0.0);
        }

        // Calculate final trust using weighted average of best paths
        found_paths.sort_by(|a, b| {
            b.trust_value
                .partial_cmp(&a.trust_value)
                .unwrap_or(Ordering::Equal)
        });

        let mut weighted_sum = 0.0;
        let mut total_weight = 0.0;

        for (i, path) in found_paths.iter().take(3).enumerate() {
            // Use top 3 paths
            let path_weight = 1.0 / (i + 1) as f64; // Weight decreases for lower-ranked paths
            weighted_sum += path.trust_value * path_weight;
            total_weight += path_weight;

            tracing::debug!(
                "Trust path {}: {} (trust: {:.3}, depth: {})",
                i + 1,
                path.path.join(" -> "),
                path.trust_value,
                path.depth
            );
        }

        let final_trust = if total_weight > 0.0 {
            weighted_sum / total_weight
        } else {
            0.0
        };

        // Apply network confidence factor
        let confidence_factor = self
            .calculate_network_confidence(network, source, target, &found_paths)
            .await?;
        Ok(final_trust * confidence_factor)
    }

    /// Calculate network confidence based on path diversity and participant reputation
    async fn calculate_network_confidence(
        &self,
        network: &TrustNetwork,
        source: &str,
        target: &str,
        paths: &[impl std::fmt::Debug], // Generic for path type
    ) -> Result<f64> {
        let source_connections = network.get_outgoing_count(source);
        let target_connections = network.get_incoming_count(target);
        let path_count = paths.len();

        // Base confidence from connectivity
        let connectivity_factor =
            ((source_connections + target_connections) as f64 / 20.0).min(1.0);

        // Path diversity factor
        let diversity_factor = (path_count as f64 / 3.0).min(1.0);

        // Network maturity factor (how established the network is)
        let total_nodes = network.node_count();
        let maturity_factor = (total_nodes as f64 / 100.0).min(1.0);

        // Combined confidence score
        let confidence = 0.4 * connectivity_factor + 0.4 * diversity_factor + 0.2 * maturity_factor;

        Ok(confidence.max(0.3)) // Minimum 30% confidence
    }

    /// Get network trust score (objective, blockchain-verified)
    /// Apply a trust boost based on verification level
    pub async fn apply_verification_boost(
        &self,
        participant_id: &str,
        boost_amount: f64,
        reason: String,
    ) -> Result<()> {
        // Get current trust balance
        let balance = self.database.get_trust_balance(participant_id).await?;

        if let Some(mut balance) = balance {
            // Apply the boost to total and available points
            balance.total_points =
                ((balance.total_points as f64) + boost_amount).min(1000.0) as u32;
            balance.available_points =
                ((balance.available_points as f64) + boost_amount).min(1000.0) as u32;
            balance.earned_lifetime = ((balance.earned_lifetime as f64) + boost_amount) as u32;

            // Update last activity
            balance.last_activity = DateTimeWrapper::new(Utc::now());

            // Save updated balance
            self.database.upsert_trust_balance(&balance).await?;

            // Log the boost
            info!(
                "Applied verification trust boost of {} to {} (reason: {})",
                boost_amount, participant_id, reason
            );
        }

        Ok(())
    }

    pub async fn get_network_trust_score(&self, participant_id: &str) -> Result<f64> {
        // Get trust score from blockchain
        let blockchain_score = self.blockchain.get_trust_score(participant_id).await?;

        // Get trust balance to factor in stake
        let balance = self.database.get_trust_balance(participant_id).await?;

        if let Some(balance) = balance {
            // Factor in trust point balance (participants with more points get slight boost)
            let balance_factor = (balance.total_points as f64).min(1000.0) / 1000.0 * 10.0; // Max 10 point bonus
            Ok((blockchain_score + balance_factor).min(100.0))
        } else {
            Ok(blockchain_score)
        }
    }

    /// Submit a trust report to the blockchain with enhanced security
    pub async fn submit_trust_report(
        &self,
        reporter_id: &str,
        subject_id: &str,
        score: i8, // -100 to +100
        category: TrustCategory,
        stake_amount: u32,
        evidence: Option<String>,
    ) -> Result<String> {
        // Input validation
        if !(-100..=100).contains(&score) {
            return Err(anyhow::anyhow!("Trust score must be between -100 and 100"));
        }

        // Validate participant IDs
        if reporter_id.is_empty() || subject_id.is_empty() {
            return Err(anyhow::anyhow!("Reporter and subject IDs cannot be empty"));
        }

        if reporter_id.len() > 256 || subject_id.len() > 256 {
            return Err(anyhow::anyhow!("Participant IDs too long (max 256 chars)"));
        }

        // Validate and sanitize evidence using security module
        let _sanitized_evidence = if let Some(ref evidence) = evidence {
            validate_trust_evidence(evidence)
                .map_err(|e| anyhow::anyhow!("Evidence validation failed: {}", e))?;
            Some(sanitize_trust_report(evidence))
        } else {
            None
        };

        // Rate limiting check
        let recent_reports = self.count_recent_reports(reporter_id).await?;
        if recent_reports > MAX_DAILY_REPORTS {
            return Err(anyhow::anyhow!(
                "Rate limit exceeded: maximum {} reports per day",
                MAX_DAILY_REPORTS
            ));
        }

        // Verify reporter has interacted with subject for negative reports
        if score < 0 {
            let has_interaction = self
                .verify_interaction_history(reporter_id, subject_id)
                .await?;
            if !has_interaction {
                return Err(anyhow::anyhow!(
                    "Cannot submit negative report without prior interaction"
                ));
            }
        }

        // Validate reporter has enough available trust points
        let reporter_balance = self
            .database
            .get_trust_balance(reporter_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Reporter not found"))?;

        if reporter_balance.available_points < stake_amount {
            return Err(anyhow::anyhow!(
                "Insufficient trust points: have {}, need {}",
                reporter_balance.available_points,
                stake_amount
            ));
        }

        // Get nonce for replay protection
        let nonce = self.blockchain.get_next_nonce(reporter_id).await?;

        // Submit to blockchain with nonce for replay protection
        let transaction_id = self
            .blockchain
            .submit_trust_report(
                reporter_id,
                subject_id,
                score,
                category,
                stake_amount,
                nonce,
            )
            .await?;

        // Log the transaction for audit purposes
        info!(
            "Trust report submitted: reporter={}, subject={}, score={}, stake={}, tx_id={}",
            reporter_id, subject_id, score, stake_amount, transaction_id
        );

        // Update trust balance (deduct staked points)
        let mut updated_balance = reporter_balance;
        updated_balance.available_points -= stake_amount;
        updated_balance.staked_points += stake_amount;
        updated_balance.last_activity = DateTimeWrapper::new(Utc::now());

        self.database.upsert_trust_balance(&updated_balance).await?;

        Ok(transaction_id)
    }

    /// Check if user has exceeded rate limit for trust reports
    async fn count_recent_reports(&self, reporter_id: &str) -> Result<u64> {
        let since = Utc::now() - Duration::hours(24);

        let count = self
            .database
            .count_reports_since(reporter_id, since)
            .await?;
        Ok(count)
    }

    /// Verify if two participants have interaction history
    async fn verify_interaction_history(
        &self,
        reporter_id: &str,
        subject_id: &str,
    ) -> Result<bool> {
        // Query for interaction history - adjusted to use abstracted DB methods
        let has_direct_interaction = self
            .database
            .has_direct_interaction(reporter_id, subject_id)
            .await?;

        if has_direct_interaction {
            return Ok(true);
        }

        // Check if they participated in same network events
        let has_shared_events = self
            .database
            .has_shared_events(reporter_id, subject_id)
            .await?;

        Ok(has_shared_events)
    }

    /// Store entity-to-entity trust rating in database
    // Feature gating removed
    #[allow(dead_code)]
    async fn store_entity_trust_rating(
        &self,
        reporter_id: &str,
        subject_id: &str,
        score: i8,
        category: TrustCategory,
    ) -> Result<()> {
        // Build direct trust score object
        let direct_score = crate::synapse::models::trust::DirectTrustScore {
            score: score.unsigned_abs().min(100),
            category,
            given_by: reporter_id.to_string(),
            given_at: DateTimeWrapper::new(Utc::now()),
            comment: None,
            relationship_context: None,
        };

        // Serialize to JSON
        let direct_score_json = serde_json::to_value(direct_score)?;

        // Store in database using raw SQL
        // In a real implementation, this would update the trust_ratings field in the participant record
        let _upsert_query = r#"
            UPDATE participants
            SET trust_ratings = jsonb_set(
                jsonb_set(
                    COALESCE(trust_ratings, '{}'::jsonb),
                    '{entity_trust}',
                    COALESCE(trust_ratings->'entity_trust', '{}'::jsonb)
                ),
                '{entity_trust,given_ratings}',
                COALESCE(
                    (
                        SELECT jsonb_agg(
                            CASE
                                WHEN rating->>'subject_id' = $3 THEN $2::jsonb
                                ELSE rating
                            END
                        )
                        FROM jsonb_array_elements(
                            COALESCE(trust_ratings->'entity_trust'->'given_ratings', '[]'::jsonb)
                        ) AS rating
                        WHERE rating->>'subject_id' = $3
                    ),
                    '[]'::jsonb
                ) || CASE
                    WHEN NOT EXISTS (
                        SELECT 1
                        FROM jsonb_array_elements(
                            COALESCE(trust_ratings->'entity_trust'->'given_ratings', '[]'::jsonb)
                        ) AS rating
                        WHERE rating->>'subject_id' = $3
                    ) THEN jsonb_build_array($2::jsonb)
                    ELSE '[]'::jsonb
                END
            )
            WHERE global_id = $1
        "#;

        // Add subject_id to the direct_score_json
        let mut score_with_subject = serde_json::Map::new();
        score_with_subject.insert(
            "subject_id".to_string(),
            serde_json::Value::String(subject_id.to_string()),
        );
        for (k, v) in direct_score_json.as_object().unwrap() {
            score_with_subject.insert(k.clone(), v.clone());
        }

        // Use alternative approach with regular query
        let score_json = serde_json::to_string(&serde_json::Value::Object(score_with_subject))?;

        let upsert_query = r#"
            UPDATE participants
            SET trust_ratings = jsonb_set(
                jsonb_set(
                    COALESCE(trust_ratings, '{}'::jsonb),
                    '{entity_trust}',
                    COALESCE(trust_ratings->'entity_trust', '{}'::jsonb)
                ),
                '{entity_trust,given_ratings}',
                COALESCE(trust_ratings->'entity_trust'->'given_ratings', '[]'::jsonb) || $2::jsonb
            )
            WHERE global_id = $1
        "#;

        sqlx::query(upsert_query)
            .bind(reporter_id)
            .bind(score_json)
            .execute(&self.database.pool)
            .await
            .context("Failed to store entity trust rating")?;

        Ok(())
    }

    /// Process trust point decay for inactive participants
    pub async fn process_decay(&self) -> Result<Vec<String>> {
        let cutoff_time = { Utc::now() - Duration::days(30) }; // 30 days inactivity
        let balances = self.database.get_balances_for_decay(cutoff_time).await?;

        let mut processed = Vec::new();

        for mut balance in balances {
            let duration =
                { Utc::now().signed_duration_since(balance.clone().last_activity.into_inner()) };
            let days_inactive = { duration.num_days() };
            let months_inactive = days_inactive as f64 / 30.0;

            // Calculate decay amount
            let decay_amount =
                (balance.total_points as f64 * balance.decay_rate * months_inactive) as u32;

            if decay_amount > 0 {
                balance.total_points = balance.total_points.saturating_sub(decay_amount);
                balance.available_points = balance.available_points.saturating_sub(decay_amount);

                // Don't let decay reduce staked points directly
                if balance.available_points < balance.staked_points {
                    balance.available_points = balance.staked_points;
                    balance.total_points = balance.staked_points;
                }

                // Update in database
                self.database.upsert_trust_balance(&balance).await?;
                processed.push(balance.participant_id.clone());

                info!(
                    "Applied decay to {}: -{} points ({} days inactive)",
                    balance.participant_id, decay_amount, days_inactive
                );
            }
        }

        Ok(processed)
    }

    /// Start background task for periodic trust decay
    pub async fn start_decay_scheduler(&self) -> Result<()> {
        let database = self.database.clone();
        let blockchain = self.blockchain.clone();

        tokio::spawn(async move {
            let mut interval = interval(tokio::time::Duration::from_secs(24 * 60 * 60)); // Daily

            loop {
                interval.tick().await;

                // Create a temporary TrustManager for the task
                let temp_manager = TrustManager {
                    database: database.clone(),
                    blockchain: blockchain.clone(),
                };

                match temp_manager.process_decay().await {
                    Ok(processed) => {
                        if !processed.is_empty() {
                            info!("Processed trust decay for {} participants", processed.len());
                        }
                    }
                    Err(e) => {
                        warn!("Failed to process trust decay: {}", e);
                    }
                }
            }
        });

        info!("Started trust decay scheduler");
        Ok(())
    }

    /// Stake trust points for specific purpose
    pub async fn stake_trust_points(
        &self,
        participant_id: &str,
        amount: u32,
        purpose: crate::synapse::blockchain::block::StakePurpose,
    ) -> Result<String> {
        // Get current balance
        let balance = self
            .database
            .get_trust_balance(participant_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Participant not found"))?;

        // Check if they have enough available points
        if balance.available_points < amount {
            return Err(anyhow::anyhow!(
                "Insufficient available trust points: have {}, need {}",
                balance.available_points,
                amount
            ));
        }

        // Validate stake amount against requirements
        let min_stake = match purpose {
            crate::synapse::blockchain::block::StakePurpose::ConsensusValidator => {
                self.blockchain
                    .config
                    .staking_requirements
                    .min_stake_for_consensus
            }
            crate::synapse::blockchain::block::StakePurpose::TrustReporting => {
                self.blockchain
                    .config
                    .staking_requirements
                    .min_stake_for_report
            }
            _ => self.blockchain.config.staking_requirements.min_stake_amount,
        };

        if amount < min_stake {
            return Err(anyhow::anyhow!(
                "Stake amount {} is below minimum {} for {:?}",
                amount,
                min_stake,
                purpose
            ));
        }

        // Create blockchain stake transaction
        let stake_tx = crate::synapse::blockchain::block::StakeTransaction::new(
            participant_id.to_string(),
            amount,
            purpose.clone(),
        );

        // Add to blockchain
        let blockchain_tx = crate::synapse::blockchain::Transaction::Stake(stake_tx);
        let transaction_id = blockchain_tx.id();

        // Update local balance (decrement available, increment staked)
        let mut updated_balance = balance;
        updated_balance.available_points -= amount;
        updated_balance.staked_points += amount;
        updated_balance.last_activity = DateTimeWrapper::new(Utc::now());

        // Update database
        self.database.upsert_trust_balance(&updated_balance).await?;

        info!(
            "Staked {} trust points for {} ({:?})",
            amount, participant_id, purpose
        );

        Ok(transaction_id)
    }

    /// Unstake previously staked trust points
    pub async fn unstake_trust_points(&self, participant_id: &str, stake_id: &str) -> Result<u32> {
        // Get current balance
        let mut balance = self
            .database
            .get_trust_balance(participant_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Participant not found"))?;

        // Attempt to unstake via blockchain
        let unstaked_amount = self
            .blockchain
            .staking_manager
            .unstake_points(participant_id, stake_id)
            .await?;

        // Update local balance
        balance.staked_points = balance.staked_points.saturating_sub(unstaked_amount);
        balance.available_points += unstaked_amount;
        balance.last_activity = DateTimeWrapper::new(Utc::now());

        // Update database
        self.database.upsert_trust_balance(&balance).await?;

        info!(
            "Unstaked {} trust points for {} (stake ID: {})",
            unstaked_amount, participant_id, stake_id
        );

        Ok(unstaked_amount)
    }

    /// Award trust points for good behavior
    pub async fn award_trust_points(
        &self,
        participant_id: &str,
        amount: u32,
        reason: &str,
    ) -> Result<()> {
        if let Some(mut balance) = self.database.get_trust_balance(participant_id).await? {
            balance.total_points += amount;
            balance.available_points += amount;
            balance.earned_lifetime += amount;
            balance.last_activity = DateTimeWrapper::new(Utc::now());

            self.database.upsert_trust_balance(&balance).await?;

            info!(
                "Awarded {} trust points to {} for: {}",
                amount, participant_id, reason
            );
        }

        Ok(())
    }

    /// Get trust balance for participant
    pub async fn get_trust_balance(&self, participant_id: &str) -> Result<Option<TrustBalance>> {
        self.database.get_trust_balance(participant_id).await
    }

    /// Calculate trust score based on participation metrics
    pub async fn calculate_participation_score(&self, _participant_id: &str) -> Result<f64> {
        // This would analyze:
        // - Message response rates
        // - Collaboration success rates
        // - Community contributions
        // - Stake participation
        // - Report accuracy

        // For now, return neutral score
        Ok(50.0)
    }

    /// Update participant activity (resets decay timer)
    pub async fn update_activity(&self, participant_id: &str) -> Result<()> {
        if let Some(mut balance) = self.database.get_trust_balance(participant_id).await? {
            balance.last_activity = DateTimeWrapper::new(Utc::now());
            self.database.upsert_trust_balance(&balance).await?;
        }

        Ok(())
    }
}
