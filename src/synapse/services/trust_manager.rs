// Synapse Trust Manager
// Manages dual trust system: entity-to-entity and network trust


use crate::blockchain::serialization::DateTimeWrapper;
use crate::synapse::models::{TrustBalance, TrustCategory};
#[cfg(feature = "database")]
use crate::synapse::storage::Database;
use crate::synapse::blockchain::SynapseBlockchain;
use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use std::sync::Arc;
use tokio::time::{interval, Duration as TokioDuration};
use tracing::{info, warn};
#[cfg(feature = "database")]
use sqlx::Row;

// Security constants
const MAX_DAILY_REPORTS: u64 = 10;
const MAX_EVIDENCE_SIZE: usize = 1024 * 10; // 10KB max evidence size

/// Trust management service for dual trust system
#[cfg(feature = "database")]
pub struct TrustManager {
    database: Arc<Database>,
    blockchain: Arc<SynapseBlockchain>,
}

/// Simplified trust manager when database feature is not available
#[cfg(not(feature = "database"))]
pub struct TrustManager {
    blockchain: Arc<SynapseBlockchain>,
}

#[cfg(feature = "database")]
impl TrustManager {
    /// Create new trust manager
    pub async fn new(
        database: Arc<Database>,
        blockchain: Arc<SynapseBlockchain>,
    ) -> Result<Self> {
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
        
        self.database.upsert_trust_balance(&initial_balance).await
            .context("Failed to initialize trust balance")?;
        
        info!("Initialized trust balance for participant: {}", participant_id);
        Ok(())
    }
    
    /// Get combined trust score (entity-to-entity + network)
    pub async fn get_trust_score(&self, subject_id: &str, requester_id: &str) -> Result<f64> {
        // Get entity-to-entity trust (subjective, personal experience)
        let entity_trust = self.get_entity_trust_score(subject_id, requester_id).await?;
        
        // Get network trust (objective, blockchain-verified)
        let network_trust = self.get_network_trust_score(subject_id).await?;
        
        // Weighted combination: 60% network trust, 40% entity trust
        let combined_score = (network_trust * 0.6) + (entity_trust * 0.4);
        
        Ok(combined_score)
    }
    
    /// Get entity-to-entity trust score (subjective)
    pub async fn get_entity_trust_score(&self, subject_id: &str, requester_id: &str) -> Result<f64> {
        // Check cache first
        let _cache_key = format!("entity_trust:{}:{}", requester_id, subject_id);
        
        // Look up direct trust relationships in database
        let sql_query = r#"
            SELECT trust_data.data->>'score' as score
            FROM participants
            CROSS JOIN LATERAL jsonb_array_elements(trust_ratings->'entity_trust'->'given_ratings') AS trust_data
            WHERE global_id = $1
            AND trust_data.data->>'given_by' = $2
        "#;
        
        let result = self.database.query_raw_string(sql_query, &[&requester_id, &subject_id]).await
            .context("Failed to query direct trust")?;
        
        if let Some(row) = result.first() {
            if let Some(score) = row.try_get::<Option<String>, _>("score").unwrap_or(None) {
                if let Ok(numeric_score) = score.parse::<u8>() {
                    // Convert to 0-100 scale
                    return Ok(numeric_score as f64);
                }
            }
        }
        
        // If no direct relationship, check for trust propagation through network
        let propagated_score = self.get_propagated_trust_score(subject_id, requester_id).await?;
        
        if propagated_score > 0.0 {
            return Ok(propagated_score);
        }
        
        // Default neutral score if no direct or propagated trust
        Ok(50.0) // 0-100 scale, 50 = neutral
    }
    
    /// Calculate trust propagated through the network
    async fn get_propagated_trust_score(&self, subject_id: &str, requester_id: &str) -> Result<f64> {
        // This is a simplified implementation of trust propagation
        // In a full implementation, we would calculate this using a graph algorithm
        
        // Get participants that requester trusts
        let trusted_by_requester_sql = r#"
            SELECT trust_data.data->>'subject_id' as trusted_id, 
                   trust_data.data->>'score' as score
            FROM participants
            CROSS JOIN LATERAL jsonb_array_elements(trust_ratings->'entity_trust'->'given_ratings') AS trust_data
            WHERE global_id = $1
            AND (trust_data.data->>'score')::integer > 70
        "#;
        
        let requester_trusts = self.database.query_raw_string(trusted_by_requester_sql, &[&requester_id]).await?;
        
        let mut total_score = 0.0;
        let mut total_weight = 0.0;
        
        // For each trusted participant, check if they have rated the subject
        for row in requester_trusts {
            let trusted_id = row.try_get::<String, _>("trusted_id").unwrap_or_default();
            let trust_score = row.try_get::<String, _>("score").unwrap_or_default().parse::<f64>().unwrap_or(0.0);
            let trust_weight = trust_score / 100.0; // Weight based on how much requester trusts them
            
            // Get their rating of the subject
            let second_hop_sql = r#"
                SELECT trust_data.data->>'score' as score
                FROM participants
                CROSS JOIN LATERAL jsonb_array_elements(trust_ratings->'entity_trust'->'given_ratings') AS trust_data
                WHERE global_id = $1
                AND trust_data.data->>'subject_id' = $2
            "#;
            
            let second_hop = self.database.query_raw_string(second_hop_sql, &[&trusted_id, &subject_id]).await?;
            
            if let Some(row) = second_hop.first() {
                if let Some(score_str) = row.try_get::<Option<String>, _>("score").unwrap_or(None) {
                    if let Ok(score) = score_str.parse::<f64>() {
                        total_score += score * trust_weight;
                        total_weight += trust_weight;
                    }
                }
            }
        }
        
        if total_weight > 0.0 {
            let propagated_score = total_score / total_weight;
            // Apply a dampening factor to propagated trust (less confidence)
            let dampened_score = 50.0 + (propagated_score - 50.0) * 0.7;
            Ok(dampened_score)
        } else {
            Ok(0.0)
        }
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
            balance.total_points = ((balance.total_points as f64) + boost_amount).min(1000.0) as u32;
            balance.available_points = ((balance.available_points as f64) + boost_amount).min(1000.0) as u32;
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
        if score < -100 || score > 100 {
            return Err(anyhow::anyhow!("Trust score must be between -100 and 100"));
        }
        
        // Validate evidence size
        if let Some(ref evidence) = evidence {
            if evidence.len() > MAX_EVIDENCE_SIZE {
                return Err(anyhow::anyhow!("Evidence size exceeds maximum allowed length"));
            }
            
            // Basic evidence sanitization
            // This is a simple example - in production use proper sanitization
            if evidence.contains("<script>") {
                return Err(anyhow::anyhow!("Evidence contains potentially malicious content"));
            }
        }
        
        // Rate limiting check
        let recent_reports = self.count_recent_reports(reporter_id).await?;
        if recent_reports > MAX_DAILY_REPORTS {
            return Err(anyhow::anyhow!("Rate limit exceeded: maximum {} reports per day", MAX_DAILY_REPORTS));
        }
        
        // Verify reporter has interacted with subject for negative reports
        if score < 0 {
            let has_interaction = self.verify_interaction_history(reporter_id, subject_id).await?;
            if !has_interaction {
                return Err(anyhow::anyhow!("Cannot submit negative report without prior interaction"));
            }
        }
        
        // Validate reporter has enough available trust points
        let reporter_balance = self.database.get_trust_balance(reporter_id).await?
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
        let transaction_id = self.blockchain.submit_trust_report(
            reporter_id,
            subject_id,
            score,
            category,
            stake_amount,
            evidence,
            nonce,
        ).await?;
        
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
        
        // Use a query that doesn't depend on the specific DB implementation
        let count = self.database.count_reports_since(reporter_id, since).await?;
        Ok(count)
    }
    
    /// Verify if two participants have interaction history
    async fn verify_interaction_history(&self, reporter_id: &str, subject_id: &str) -> Result<bool> {
        // Query for interaction history - adjusted to use abstracted DB methods
        let has_direct_interaction = self.database.has_direct_interaction(reporter_id, subject_id).await?;
        
        if has_direct_interaction {
            return Ok(true);
        }
        
        // Check if they participated in same network events
        let has_shared_events = self.database.has_shared_events(reporter_id, subject_id).await?;
        
        Ok(has_shared_events)
    }
    
    /// Store entity-to-entity trust rating in database
    #[cfg(feature = "database")]
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
            score: score.unsigned_abs().min(100) as u8,
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
        score_with_subject.insert("subject_id".to_string(), serde_json::Value::String(subject_id.to_string()));
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
        let cutoff_time = Utc::now() - Duration::days(30); // 30 days inactivity
        let balances = self.database.get_balances_for_decay(cutoff_time).await?;
        
        let mut processed = Vec::new();
        
        for mut balance in balances {
            let duration = Utc::now().signed_duration_since(balance.clone().last_activity.into_inner());
            let days_inactive = duration.num_days();
            let months_inactive = days_inactive as f64 / 30.0;
            
            // Calculate decay amount
            let decay_amount = (balance.total_points as f64 * balance.decay_rate * months_inactive) as u32;
            
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
            let mut interval = interval(TokioDuration::from_secs(24 * 60 * 60)); // Daily
            
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
        let balance = self.database.get_trust_balance(participant_id).await?
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
                self.blockchain.config.staking_requirements.min_stake_for_consensus
            },
            crate::synapse::blockchain::block::StakePurpose::TrustReporting => {
                self.blockchain.config.staking_requirements.min_stake_for_report
            },
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
            amount,
            participant_id,
            purpose
        );
        
        Ok(transaction_id)
    }
    
    /// Unstake previously staked trust points
    pub async fn unstake_trust_points(
        &self,
        participant_id: &str,
        stake_id: &str,
    ) -> Result<u32> {
        // Get current balance
        let mut balance = self.database.get_trust_balance(participant_id).await?
            .ok_or_else(|| anyhow::anyhow!("Participant not found"))?;
            
        // Attempt to unstake via blockchain
        let unstaked_amount = self.blockchain.staking_manager.unstake_points(participant_id, stake_id).await?;
        
        // Update local balance
        balance.staked_points = balance.staked_points.saturating_sub(unstaked_amount);
        balance.available_points += unstaked_amount;
        balance.last_activity = DateTimeWrapper::new(Utc::now());
        
        // Update database
        self.database.upsert_trust_balance(&balance).await?;
        
        info!(
            "Unstaked {} trust points for {} (stake ID: {})",
            unstaked_amount,
            participant_id,
            stake_id
        );
        
        Ok(unstaked_amount)
    }
    
    /// Award trust points for good behavior
    pub async fn award_trust_points(&self, participant_id: &str, amount: u32, reason: &str) -> Result<()> {
        if let Some(mut balance) = self.database.get_trust_balance(participant_id).await? {
            balance.total_points += amount;
            balance.available_points += amount;
            balance.earned_lifetime += amount;
            balance.last_activity = DateTimeWrapper::new(Utc::now());
            
            self.database.upsert_trust_balance(&balance).await?;
            
            info!("Awarded {} trust points to {} for: {}", amount, participant_id, reason);
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

#[cfg(not(feature = "database"))]
impl TrustManager {
    /// Create new simplified trust manager without database
    pub async fn new(
        blockchain: Arc<SynapseBlockchain>,
    ) -> Result<Self> {
        Ok(Self {
            blockchain,
        })
    }

    /// Initialize trust balance for new participant (simplified)
    pub async fn initialize_participant(&self, _participant_id: &str) -> Result<()> {
        // Without database, just acknowledge the request
        Ok(())
    }

    /// Calculate trust between two participants (simplified)
    pub async fn calculate_trust(&self, _from_id: &str, _to_id: &str) -> Result<f64> {
        // Return default trust score when database is not available
        Ok(0.5)
    }

    /// Report trust violation (simplified)
    pub async fn report_violation(
        &self,
        _reporter_id: &str,
        _reported_id: &str,
        _violation_type: &str,
        _evidence: &str,
        _stake_amount: u32,
    ) -> Result<String> {
        // Return a dummy report ID when database is not available
        Ok(UuidWrapper::new(uuid::UuidWrapper::new(Uuid::new_v4())).to_string())
    }

    /// Award trust points (simplified)
    pub async fn award_points(
        &self,
        _participant_id: &str,
        _points: u32,
        _category: &str,
        _reason: &str,
    ) -> Result<()> {
        Ok(())
    }

    /// Get trust balance (simplified)
    pub async fn get_trust_balance(&self, _participant_id: &str) -> Result<Option<TrustBalance>> {
        // Return None when database is not available
        Ok(None)
    }

    /// Stake trust points (simplified)
    pub async fn stake_points(
        &self,
        _participant_id: &str,
        _amount: u32,
        _reason: &str,
    ) -> Result<()> {
        Ok(())
    }

    /// Unstake trust points (simplified)
    pub async fn unstake_points(
        &self,
        _participant_id: &str,
        _amount: u32,
    ) -> Result<u32> {
        Ok(0)
    }

    /// Process trust point decay (simplified)
    pub async fn process_decay(&self) -> Result<Vec<String>> {
        Ok(vec![])
    }

    /// Start decay scheduler (simplified version)
    pub async fn start_decay_scheduler(&self) -> Result<()> {
        // Without database, nothing to schedule
        info!("Trust decay scheduler started (simplified mode)");
        Ok(())
    }

    /// Get trust score (simplified version)
    pub async fn get_trust_score(&self, _subject_id: &str, _requester_id: &str) -> Result<f64> {
        // Return default neutral score
        Ok(50.0)
    }

    /// Get network trust score (simplified version)
    pub async fn get_network_trust_score(&self, participant_id: &str) -> Result<f64> {
        // Get score from blockchain only
        self.blockchain.get_trust_score(participant_id).await
    }

    /// Submit trust report (simplified version)
    pub async fn submit_trust_report(
        &self,
        _reporter_id: &str,
        _subject_id: &str,
        _score: i8,
        _category: crate::synapse::models::TrustCategory,
        _stake_amount: u32,
        _evidence: Option<String>,
    ) -> Result<String> {
        // Return a dummy report ID
        Ok(UuidWrapper::new(uuid::UuidWrapper::new(Uuid::new_v4())).to_string())
    }
}

