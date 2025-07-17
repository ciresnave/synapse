use crate::synapse::blockchain::{Block, BlockchainConfig, Transaction, serialization::DateTimeWrapper};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{info, debug, warn};

/// Consensus mechanism for Synapse blockchain
pub struct ConsensusEngine {
    config: BlockchainConfig,
    validators: HashMap<String, ValidatorInfo>,
    current_round: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorInfo {
    pub participant_id: String,
    pub stake_amount: u64,
    pub trust_score: f64,
    pub last_activity: DateTime<Utc>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusRound {
    pub round_number: u64,
    pub block_height: u64,
    pub proposed_block: Option<Block>,
    pub votes: HashMap<String, Vote>,
    pub started_at: DateTime<Utc>,
    pub finalized_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub validator_id: String,
    pub block_hash: String,
    pub vote_type: VoteType,
    pub timestamp: DateTime<Utc>,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VoteType {
    Prevote,    // Initial vote for a block
    Precommit,  // Commitment to finalize a block
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusState {
    pub height: u64,
    pub round: u64,
    pub step: ConsensusStep,
    pub locked_block: Option<Block>,
    pub valid_block: Option<Block>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsensusStep {
    Propose,
    Prevote,
    Precommit,
    Commit,
}

impl ConsensusEngine {
    pub fn new(config: BlockchainConfig) -> Self {
        Self {
            config,
            validators: HashMap::new(),
            current_round: 0,
        }
    }

    /// Add a validator to the consensus
    pub async fn add_validator(&mut self, validator: ValidatorInfo) -> Result<()> {
        debug!("Adding validator: {}", validator.participant_id);
        
        // Validate minimum stake requirement
        if validator.stake_amount < self.config.staking_requirements.min_stake_for_consensus as u64 {
            return Err(anyhow::anyhow!("Insufficient stake amount"));
        }

        // Validate minimum trust score
        // For now, we'll use a simple minimum trust threshold
        if validator.trust_score < 50.0 { // Minimum 50% trust score
            return Err(anyhow::anyhow!("Insufficient trust score"));
        }

        self.validators.insert(validator.participant_id.clone(), validator);
        info!("Validator added successfully");
        Ok(())
    }

    /// Remove a validator from consensus
    pub async fn remove_validator(&mut self, participant_id: &str) -> Result<()> {
        debug!("Removing validator: {}", participant_id);
        
        if let Some(_validator) = self.validators.remove(participant_id) {
            info!("Validator {} removed successfully", participant_id);
            Ok(())
        } else {
            Err(anyhow::anyhow!("Validator not found"))
        }
    }

    /// Start consensus round for a new block
    pub async fn start_consensus_round(
        &mut self,
        block_height: u64,
        transactions: Vec<Transaction>,
    ) -> Result<ConsensusRound> {
        self.current_round += 1;
        
        debug!("Starting consensus round {} for block height {}", 
               self.current_round, block_height);

        // Select proposer for this round (round-robin based on stake)
        let proposer = self.select_proposer()?;
        
        // Create proposed block
        let proposed_block = self.create_proposed_block(block_height, transactions, &proposer).await?;

        let consensus_round = ConsensusRound {
            round_number: self.current_round,
            block_height,
            proposed_block: Some(proposed_block),
            votes: HashMap::new(),
            started_at: Utc::now(),
            finalized_at: None,
        };

        info!("Consensus round {} started with proposer: {}", 
              self.current_round, proposer);

        Ok(consensus_round)
    }

    /// Process a vote from a validator
    pub async fn process_vote(
        &mut self,
        consensus_round: &mut ConsensusRound,
        vote: Vote,
    ) -> Result<bool> {
        debug!("Processing vote from validator: {}", vote.validator_id);

        // Validate voter is an active validator
        let validator = self.validators.get(&vote.validator_id)
            .ok_or_else(|| anyhow::anyhow!("Unknown validator"))?;

        if !validator.is_active {
            return Err(anyhow::anyhow!("Inactive validator"));
        }

        // Validate vote signature (simplified for now)
        self.validate_vote_signature(&vote)?;

        // Store the vote
        consensus_round.votes.insert(vote.validator_id.clone(), vote);

        // Check if we have enough votes to finalize
        let finalized = self.check_finalization(consensus_round).await?;

        if finalized {
            consensus_round.finalized_at = Some(Utc::now());
            info!("Consensus round {} finalized", consensus_round.round_number);
        }

        Ok(finalized)
    }

    /// Check if consensus has been reached
    async fn check_finalization(&self, consensus_round: &ConsensusRound) -> Result<bool> {
        let total_stake = self.get_total_stake();
        let required_stake = (total_stake * 2) / 3; // 2/3 majority by stake

        let mut _vote_stake = 0u64;
        let mut precommit_stake = 0u64;

        for (validator_id, vote) in &consensus_round.votes {
            if let Some(validator) = self.validators.get(validator_id) {
                _vote_stake += validator.stake_amount;
                
                if matches!(vote.vote_type, VoteType::Precommit) {
                    precommit_stake += validator.stake_amount;
                }
            }
        }

        // Check if we have 2/3 stake in precommits for the same block
        Ok(precommit_stake >= required_stake)
    }

    fn select_proposer(&self) -> Result<String> {
        // Simple round-robin selection weighted by stake
        // In practice, this would be more sophisticated
        let active_validators: Vec<&ValidatorInfo> = self.validators
            .values()
            .filter(|v| v.is_active)
            .collect();

        if active_validators.is_empty() {
            return Err(anyhow::anyhow!("No active validators"));
        }

        // For simplicity, select the validator with highest stake
        let proposer = active_validators
            .iter()
            .max_by_key(|v| v.stake_amount)
            .unwrap();

        Ok(proposer.participant_id.clone())
    }

    async fn create_proposed_block(
        &self,
        height: u64,
        transactions: Vec<Transaction>,
        proposer: &str,
    ) -> Result<Block> {
        // This would create a proper block with the given transactions
        // For now, create a placeholder
        let block = Block {
            number: height,
            timestamp: DateTimeWrapper(Utc::now()),
            previous_hash: "".to_string(), // Would be actual previous hash
            hash: "".to_string(), // Would be calculated
            transactions,
            nonce: 0, // Would be set during consensus
            validator: proposer.to_string(),
        };

        Ok(block)
    }

    fn validate_vote_signature(&self, _vote: &Vote) -> Result<()> {
        // Placeholder for signature validation
        // In practice, this would verify the vote signature against the validator's public key
        Ok(())
    }

    fn get_total_stake(&self) -> u64 {
        self.validators
            .values()
            .filter(|v| v.is_active)
            .map(|v| v.stake_amount)
            .sum()
    }

    /// Get current consensus state
    pub fn get_consensus_state(&self) -> ConsensusState {
        ConsensusState {
            height: 0, // Would track actual height
            round: self.current_round,
            step: ConsensusStep::Propose, // Would track actual step
            locked_block: None,
            valid_block: None,
        }
    }

    /// Update validator trust scores
    pub async fn update_validator_trust_scores(
        &mut self,
        trust_updates: HashMap<String, f64>,
    ) -> Result<()> {
        for (validator_id, new_trust_score) in trust_updates {
            if let Some(validator) = self.validators.get_mut(&validator_id) {
                validator.trust_score = new_trust_score;
                
                // Deactivate validator if trust falls below threshold
                if new_trust_score < 50.0 { // Minimum trust threshold
                    validator.is_active = false;
                    warn!("Validator {} deactivated due to low trust score: {}", 
                          validator_id, new_trust_score);
                }
            }
        }

        Ok(())
    }

    /// Get active validator count
    pub fn get_active_validator_count(&self) -> usize {
        self.validators.values().filter(|v| v.is_active).count()
    }

    /// Check if minimum validators are available for consensus
    pub fn has_minimum_validators(&self) -> bool {
        self.get_active_validator_count() >= self.config.min_consensus_nodes
    }
}
