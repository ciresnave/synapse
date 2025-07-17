// Synapse Blockchain for Trust Verification

pub mod block;
pub mod consensus;
pub mod serialization;  // Add this line
pub mod staking;
pub mod verification;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use dashmap::DashMap;
use tokio::sync::RwLock;
use tracing::info;
 
 pub use block::{Block, Transaction, TrustReport, TrustReportType};
 pub use consensus::ConsensusEngine;
 pub use staking::StakingManager;
 pub use verification::VerificationEngine;

/// Configuration for Synapse blockchain
#[derive(Debug, Clone)]
pub struct BlockchainConfig {
    pub genesis_trust_points: u32,
    pub block_time_seconds: u64,
    pub min_consensus_nodes: usize,
    pub staking_requirements: StakingRequirements,
    pub trust_decay_config: TrustDecayConfig,
}

#[derive(Debug, Clone)]
pub struct StakingRequirements {
    pub min_stake_amount: u32,
    pub max_stake_amount: u32,
    pub min_stake_for_report: u32,
    pub min_stake_for_consensus: u32,
    pub slash_percentage: f64, // Percentage of stake to slash for false reports
}

#[derive(Debug, Clone)]
pub struct TrustDecayConfig {
    pub monthly_decay_rate: f64, // Default 2% per month
    pub min_activity_days: u64,  // Days without activity before decay starts
    pub decay_check_interval_hours: u64, // How often to run decay
}

impl Default for BlockchainConfig {
    fn default() -> Self {
        Self {
            genesis_trust_points: 100,
            block_time_seconds: 30, // 30 second blocks
            min_consensus_nodes: 3,
            staking_requirements: StakingRequirements {
                min_stake_amount: 10,
                max_stake_amount: 10000,
                min_stake_for_report: 10,
                min_stake_for_consensus: 50,
                slash_percentage: 0.1, // 10% slash for false reports
            },
            trust_decay_config: TrustDecayConfig {
                monthly_decay_rate: 0.02, // 2% per month
                min_activity_days: 30,
                decay_check_interval_hours: 24, // Check daily
            },
        }
    }
}

/// Main Synapse blockchain coordinator
pub struct SynapseBlockchain {
    pub config: BlockchainConfig,
    chain: Arc<RwLock<Vec<Block>>>,
    pending_transactions: Arc<RwLock<Vec<Transaction>>>,
    // Track participant nonces for replay protection
    participant_nonces: Arc<DashMap<String, u64>>,
    #[allow(dead_code)]
    consensus_engine: Arc<ConsensusEngine>,
    pub staking_manager: Arc<StakingManager>,
    #[allow(dead_code)]
    verification_engine: Arc<VerificationEngine>,
}

impl SynapseBlockchain {
    /// Create new blockchain instance
    pub async fn new(config: BlockchainConfig) -> Result<Self> {
        let genesis_block = Block::genesis();
        let chain = Arc::new(RwLock::new(vec![genesis_block]));
        
        let consensus_engine = Arc::new(ConsensusEngine::new(
            config.clone(),
        ));
        
        let staking_manager = Arc::new(StakingManager::new(
            config.staking_requirements.clone(),
            chain.clone(),
        ).await?);
        
        let verification_engine = Arc::new(VerificationEngine::new(config.clone()));
        
        Ok(Self {
            config,
            chain,
            pending_transactions: Arc::new(RwLock::new(Vec::new())),
            participant_nonces: Arc::new(DashMap::new()),
            consensus_engine,
            staking_manager,
            verification_engine,
        })
    }
    
    /// Start consensus process
    pub async fn start_consensus(&self) -> Result<()> {
        let chain = self.chain.clone();
        let pending_transactions = self.pending_transactions.clone();
        let config = self.config.clone();
        let staking_manager = self.staking_manager.clone();
        
        // Start consensus in background thread
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(config.block_time_seconds));
            
            loop {
                interval.tick().await;
                
                // Process pending transactions into a new block
                if let Err(e) = Self::process_next_block(
                    &chain,
                    &pending_transactions,
                    &staking_manager,
                    &config
                ).await {
                    tracing::error!("Consensus error: {}", e);
                }
            }
        });
        
        info!("Consensus engine started with {} second block time", self.config.block_time_seconds);
        Ok(())
    }
    
    /// Process pending transactions into a new block
    async fn process_next_block(
        chain: &Arc<RwLock<Vec<Block>>>,
        pending_transactions: &Arc<RwLock<Vec<Transaction>>>,
        staking_manager: &Arc<StakingManager>,
        config: &BlockchainConfig,
    ) -> Result<()> {
        // Get pending transactions
        let transactions = {
            let mut pending = pending_transactions.write().await;
            if pending.is_empty() {
                return Ok(());  // No transactions to process
            }
            
            // Take up to 100 transactions per block
            let count = pending.len().min(100);
            let processed: Vec<_> = pending.drain(0..count).collect();
            processed
        };
        
        // Create new block
        let previous_block = {
            let chain_read = chain.read().await;
            chain_read.last().cloned().ok_or_else(|| anyhow::anyhow!("Chain is empty"))?
        };
        
        // Select a validator (in production this would use consensus algorithm)
        let validators = staking_manager.get_consensus_validators().await?;
        
        if validators.is_empty() || validators.len() < config.min_consensus_nodes {
            // Not enough validators for consensus
            tracing::warn!(
                "Not enough validators for consensus: have {}, need {}",
                validators.len(),
                config.min_consensus_nodes
            );
            return Ok(());
        }
        
        // Simple round-robin validator selection based on block number
        let validator_idx = previous_block.number as usize % validators.len();
        let validator = validators[validator_idx].clone();
        
        // Create and add the block
        let new_block = Block::new(
            previous_block.number + 1,
            previous_block.hash.clone(),
            transactions,
            validator,
        );
        
        // Verify block before adding
        if !new_block.verify(Some(&previous_block)) {
            return Err(anyhow::anyhow!("Block verification failed"));
        }
        
        // Add to chain
        {
            let mut chain_write = chain.write().await;
            chain_write.push(new_block.clone());
        }
        
        tracing::info!(
            "Block {} added with {} transactions, validated by {}",
            new_block.number,
            new_block.transactions.len(),
            new_block.validator
        );
        
        Ok(())
    }
    
    /// Get next nonce for a participant (for transaction replay protection)
    pub async fn get_next_nonce(&self, participant_id: &str) -> Result<u64> {
        let mut entry = self.participant_nonces.entry(participant_id.to_string()).or_insert(0);
        let next_nonce = *entry.value() + 1;
        *entry.value_mut() = next_nonce;
        Ok(next_nonce)
    }
    
    /// Verify nonce is valid for participant
    pub async fn verify_nonce(&self, participant_id: &str, nonce: u64) -> Result<bool> {
        let current = self.participant_nonces.get(participant_id).map(|entry| *entry.value()).unwrap_or(0);
        
        // Nonce must be exactly one more than current
        if nonce != current + 1 {
            return Ok(false);
        }
        
        Ok(true)
    }
    
    /// Submit a trust report to the blockchain with nonce for replay protection
    pub async fn submit_trust_report(
        &self,
        reporter_id: &str, 
        subject_id: &str,
        score: i8,
        category: crate::synapse::models::TrustCategory,
        stake_amount: u32,
        _evidence: Option<String>,
        nonce: u64,
    ) -> Result<String> {
        // Verify nonce is valid to prevent replay attacks
        if !self.verify_nonce(reporter_id, nonce).await? {
            return Err(anyhow::anyhow!("Invalid nonce for transaction"));
        }
        
        // Verify the reporter has enough stake
        if !self.staking_manager.has_sufficient_stake(reporter_id, self.config.staking_requirements.min_stake_for_report).await? {
            return Err(anyhow::anyhow!("Insufficient stake to submit report"));
        }
        
        // Convert category to string for blockchain storage
        let category_str = match category {
            crate::synapse::models::TrustCategory::Communication => "communication",
            crate::synapse::models::TrustCategory::Technical => "technical",
            crate::synapse::models::TrustCategory::Collaboration => "collaboration",
            crate::synapse::models::TrustCategory::Reliability => "reliability",
            crate::synapse::models::TrustCategory::Privacy => "privacy",
            crate::synapse::models::TrustCategory::Overall => "overall",
        }.to_string();
        
        // Determine report type based on score
        let report_type = if score >= 0 {
            TrustReportType::Positive
        } else {
            TrustReportType::Negative
        };
        
        // Create trust report
        let report = TrustReport::new(
            reporter_id.to_string(),
            subject_id.to_string(),
            report_type,
            score,
            category_str,
            stake_amount,
        );
        
        // Create transaction
        let transaction = Transaction::TrustReport(report);
        let transaction_id = transaction.id();
        
        // Update participant nonce
        self.participant_nonces.insert(reporter_id.to_string(), nonce);
        
        // Add to pending transactions
        {
            let mut pending = self.pending_transactions.write().await;
            pending.push(transaction);
        }
        
        info!(
            "Trust report submitted: reporter={}, subject={}, score={}, stake={}, tx_id={}, nonce={}", 
            reporter_id, subject_id, score, stake_amount, transaction_id, nonce
        );
        
        Ok(transaction_id)
    }
    
    /// Get trust score from blockchain
    pub async fn get_trust_score(&self, participant_id: &str) -> Result<f64> {
        let chain = self.chain.read().await;
        let mut total_score = 0.0;
        let mut report_count = 0;
        
        // Scan all blocks for trust reports about this participant
        for block in chain.iter() {
            for transaction in &block.transactions {
                if let Transaction::TrustReport(report) = transaction {
                    if report.subject_id == participant_id {
                        // Weight recent reports more heavily
                        let age_days = (Utc::now() - report.timestamp.0).num_days();
                        let weight = if age_days < 30 { 1.0 } else { 0.5 };
                        
                        total_score += report.score as f64 * weight;
                        report_count += 1;
                    }
                }
            }
        }
        
        if report_count == 0 {
            Ok(0.0) // No reports = neutral score
        } else {
            Ok(total_score / report_count as f64)
        }
    }
    
    /// Process trust point decay
    pub async fn process_trust_decay(&self) -> Result<Vec<String>> {
        let decay_rate = self.config.trust_decay_config.monthly_decay_rate;
        let min_activity_days = self.config.trust_decay_config.min_activity_days;
        
        // Get cutoff time for decay processing
        let cutoff_time = Utc::now() - chrono::Duration::days(min_activity_days as i64);
        
        // Fetch participants with balances that need decay processing
        let participants = self.staking_manager.get_all_participants().await?;
        let mut processed = Vec::new();
        
        for participant_id in participants {
            let balances = self.staking_manager.get_participant_balances(&participant_id).await?;
            
            for mut balance in balances {
                // Check if decay should be applied
                if balance.last_activity.clone().into_inner() < cutoff_time {
                    // Calculate days since last activity
                    let duration = Utc::now() - balance.last_activity.clone().into_inner();
                    let days_inactive = duration.num_days() as f64;
                    let months_inactive = days_inactive / 30.0;
                    
                    // Calculate decay amount
                    let decay_amount = (balance.total_points as f64 * decay_rate * months_inactive) as u32;
                    
                    if decay_amount > 0 {
                        // Apply decay
                        balance.total_points = balance.total_points.saturating_sub(decay_amount);
                        balance.available_points = balance.available_points.saturating_sub(decay_amount);
                        
                        // Don't decay below staked amount
                        if balance.available_points < balance.staked_points {
                            balance.available_points = balance.staked_points;
                            balance.total_points = balance.staked_points;
                        }
                        
                        // Update balance
                        self.staking_manager.update_balance(&participant_id, &balance).await?;
                        
                        // Add to processed list
                        processed.push(participant_id.clone());
                        
                        info!(
                            "Applied decay to {}: -{} points ({} days inactive)",
                            participant_id,
                            decay_amount,
                            days_inactive
                        );
                    }
                }
            }
        }
        
        Ok(processed)
    }
    
    /// Get blockchain statistics
    pub async fn get_stats(&self) -> Result<BlockchainStats> {
        let chain = self.chain.read().await;
        let pending = self.pending_transactions.read().await;
        
        let mut total_reports = 0;
        let mut total_stakes = 0;
        
        for block in chain.iter() {
            for transaction in &block.transactions {
                match transaction {
                    Transaction::TrustReport(_) => total_reports += 1,
                    Transaction::Stake(_) => total_stakes += 1,
                    _ => {}
                }
            }
        }
        
        Ok(BlockchainStats {
            block_count: chain.len(),
            pending_transactions: pending.len(),
            total_trust_reports: total_reports,
            total_stakes: total_stakes,
            last_block_time: chain.last().map(|b| b.timestamp.0),
        })
    }
}

/// Blockchain statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainStats {
    pub block_count: usize,
    pub pending_transactions: usize,
    pub total_trust_reports: u64,
    pub total_stakes: u64,
    pub last_block_time: Option<DateTime<Utc>>,
}
