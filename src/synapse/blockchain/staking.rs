impl StakingManager {
    /// Synchronous constructor for StakingManager
    pub fn new(config: super::StakingRequirements, chain: Arc<RwLock<Vec<Block>>>) -> Self {
        StakingManager {
            config,
            chain,
            active_stakes: DashMap::new(),
        }
    }
}
use crate::synapse::TrustBalance;
use crate::synapse::blockchain::serialization::{DateTimeWrapper, UuidWrapper};
use crate::synapse::blockchain::{Block, StakePurpose, Transaction};
use anyhow::Result;
use chrono::Utc;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

pub struct StakingManager {
    config: super::StakingRequirements,
    chain: Arc<RwLock<Vec<Block>>>,
    active_stakes: DashMap<String, Vec<ActiveStake>>, // participant_id -> stakes
}

impl StakingManager {
    /// Get total available (unstaked) trust points for participant
    pub async fn get_available_stake(&self, participant_id: &str) -> Result<u32> {
        let total_points = self.get_total_trust_points(participant_id).await?;
        let staked_points = self.get_staked_points(participant_id).await?;
        Ok(total_points.saturating_sub(staked_points))
    }

    /// Check if participant has sufficient available stake
    pub async fn has_sufficient_stake(&self, participant_id: &str, required: u32) -> Result<bool> {
        let available = self.get_available_stake(participant_id).await?;
        Ok(available >= required)
    }

    /// Get total trust points for participant
    pub async fn get_total_trust_points(&self, participant_id: &str) -> Result<u32> {
        let chain = self.chain.read().await;
        let mut total_points = 0u32;
        for block in chain.iter() {
            for transaction in &block.transactions {
                match transaction {
                    Transaction::Registration(reg) if reg.participant_id == participant_id => {
                        total_points += reg.initial_trust_points;
                    }
                    Transaction::Transfer(transfer)
                        if transfer.to_participant == participant_id =>
                    {
                        total_points += transfer.amount;
                    }
                    Transaction::Transfer(transfer)
                        if transfer.from_participant == participant_id =>
                    {
                        total_points = total_points.saturating_sub(transfer.amount);
                    }
                    Transaction::TrustReport(report) if report.subject_id == participant_id => {
                        if report.score > 0 {
                            let awarded = (report.score as u32 * report.stake_amount) / 100;
                            total_points += awarded;
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(total_points)
    }

    /// Get currently staked points for participant
    pub async fn get_staked_points(&self, participant_id: &str) -> Result<u32> {
        let stakes = self
            .active_stakes
            .get(participant_id)
            .map(|entry| entry.value().clone())
            .unwrap_or_default();
        Ok(stakes.iter().map(|s| s.amount).sum())
    }

    /// Stake trust points for a specific purpose
    pub async fn stake_points(
        &self,
        participant_id: &str,
        amount: u32,
        purpose: StakePurpose,
    ) -> Result<String> {
        if amount < self.config.min_stake_amount {
            return Err(anyhow::anyhow!(
                "Stake amount {} is below minimum {}",
                amount,
                self.config.min_stake_amount
            ));
        }
        if amount > self.config.max_stake_amount {
            return Err(anyhow::anyhow!(
                "Stake amount {} exceeds maximum {}",
                amount,
                self.config.max_stake_amount
            ));
        }
        if self.get_available_stake(participant_id).await? < amount {
            return Err(anyhow::anyhow!("Insufficient available trust points"));
        }
        let stake = ActiveStake {
            id: UuidWrapper::new(Uuid::new_v4()).to_string(),
            participant_id: participant_id.to_string(),
            amount,
            purpose,
            staked_at: DateTimeWrapper::new(Utc::now()),
            locked_until: None,
        };
        self.active_stakes
            .entry(participant_id.to_string())
            .or_default()
            .push(stake.clone());
        Ok(stake.id)
    }

    /// Unstake trust points
    pub async fn unstake_points(&self, participant_id: &str, stake_id: &str) -> Result<u32> {
        let mut found_stake: Option<ActiveStake> = None;
        let mut stake_index: Option<usize> = None;
        let mut stakes_vec = self.active_stakes.get_mut(participant_id);
        if let Some(stakes) = &mut stakes_vec
            && let Some(index) = stakes.iter().position(|s| s.id == stake_id)
        {
            stake_index = Some(index);
            found_stake = Some(stakes[index].clone());
        }
        let stake = found_stake.ok_or_else(|| anyhow::anyhow!("Stake not found"))?;
        let index = stake_index.unwrap();
        if let Some(_locked_until) = stake.locked_until
            && Utc::now() < _locked_until.into_inner()
        {
            return Err(anyhow::anyhow!("Stake is still locked"));
        }
        if let Some(stakes) = &mut stakes_vec {
            stakes.remove(index);
        }
        Ok(stake.amount)
    }

    /// Slash stake for false reports or bad behavior
    pub async fn slash_stake(
        &self,
        participant_id: &str,
        stake_id: &str,
        reason: &str,
    ) -> Result<u32> {
        let mut found_stake: Option<ActiveStake> = None;
        let mut stake_index: Option<usize> = None;
        let mut stakes_vec = self.active_stakes.get_mut(participant_id);
        if let Some(stakes) = &mut stakes_vec
            && let Some(index) = stakes.iter().position(|s| s.id == stake_id)
        {
            stake_index = Some(index);
            found_stake = Some(stakes[index].clone());
        }
        let stake = found_stake.ok_or_else(|| anyhow::anyhow!("Stake not found"))?;
        let index = stake_index.unwrap();
        let slashed_amount = (stake.amount as f64 * self.config.slash_percentage / 100.0) as u32;
        let remaining_amount = stake.amount.saturating_sub(slashed_amount);
        if let Some(stakes) = &mut stakes_vec {
            stakes.remove(index);
            if remaining_amount > 0 {
                let remaining_stake = ActiveStake {
                    id: UuidWrapper::new(Uuid::new_v4()).to_string(),
                    participant_id: stake.participant_id,
                    amount: remaining_amount,
                    purpose: stake.purpose,
                    staked_at: stake.staked_at,
                    locked_until: stake.locked_until,
                };
                stakes.push(remaining_stake);
            }
        }
        tracing::warn!(
            "Slashed {} trust points from {} for: {}",
            slashed_amount,
            participant_id,
            reason
        );
        Ok(slashed_amount)
    }

    /// Get all stakes for a participant
    pub async fn get_participant_stakes(&self, participant_id: &str) -> Result<Vec<ActiveStake>> {
        Ok(self
            .active_stakes
            .get(participant_id)
            .map(|entry| entry.value().clone())
            .unwrap_or_default())
    }

    /// Get participants eligible for consensus (have minimum stake)
    /// Returns validators sorted by total stake amount in descending order
    pub async fn get_consensus_validators(&self) -> Result<Vec<String>> {
        let mut validators_with_stake = Vec::new();
        for entry in self.active_stakes.iter() {
            let participant_id = entry.key();
            let participant_stakes = entry.value();
            let total_consensus_stake = participant_stakes
                .iter()
                .filter(|s| matches!(s.purpose, StakePurpose::ConsensusValidator))
                .map(|s| s.amount)
                .sum::<u32>();
            if total_consensus_stake >= self.config.min_stake_for_consensus {
                validators_with_stake.push((participant_id.clone(), total_consensus_stake));
            }
        }

        // Sort validators by stake amount in descending order (highest stake first)
        validators_with_stake.sort_by(|a, b| b.1.cmp(&a.1));

        // Extract just the validator IDs
        let validators = validators_with_stake
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        Ok(validators)
    }

    /// Lock stake for a specific period (e.g., during consensus participation)
    pub async fn lock_stake(
        &self,
        participant_id: &str,
        stake_id: &str,
        lock_duration: chrono::Duration,
    ) -> Result<()> {
        let mut stakes_vec = self.active_stakes.get_mut(participant_id);
        if let Some(stakes) = &mut stakes_vec
            && let Some(stake) = stakes.iter_mut().find(|s| s.id == stake_id)
        {
            stake.locked_until = Some(DateTimeWrapper::new(Utc::now() + lock_duration));
            return Ok(());
        }
        Err(anyhow::anyhow!("Stake not found"))
    }

    /// Get all participant IDs in the system
    pub async fn get_all_participants(&self) -> Result<Vec<String>> {
        let mut participants = Vec::new();
        for entry in self.active_stakes.iter() {
            participants.push(entry.key().clone());
        }
        Ok(participants)
    }

    /// Get all balances for a participant
    pub async fn get_participant_balances(
        &self,
        participant_id: &str,
    ) -> Result<Vec<TrustBalance>> {
        let stakes = self
            .get_participant_stakes(participant_id)
            .await
            .unwrap_or_default();
        if stakes.is_empty() {
            return Ok(vec![]);
        }
        let staked_amount: u32 = stakes.iter().map(|s| s.amount).sum();
        let total_points = self
            .get_total_trust_points(participant_id)
            .await
            .unwrap_or(0);
        let balance = TrustBalance {
            participant_id: participant_id.to_string(),
            total_points,
            available_points: total_points.saturating_sub(staked_amount),
            staked_points: staked_amount,
            earned_lifetime: total_points,
            last_activity: DateTimeWrapper::new(
                stakes
                    .iter()
                    .map(|s| s.staked_at.clone().into_inner())
                    .max()
                    .unwrap_or_else(Utc::now),
            ),
            decay_rate: 0.02,
        };
        Ok(vec![balance])
    }

    /// Update a participant's trust balance
    pub async fn update_balance(&self, participant_id: &str, balance: &TrustBalance) -> Result<()> {
        tracing::info!(
            "Updated balance for {}: {} total, {} available, {} staked",
            participant_id,
            balance.total_points,
            balance.available_points,
            balance.staked_points
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "serde")]
pub struct ActiveStake {
    pub id: String,
    pub participant_id: String,
    pub amount: u32,
    pub purpose: StakePurpose,
    pub staked_at: DateTimeWrapper,
    pub locked_until: Option<DateTimeWrapper>,
}

impl ActiveStake {
    pub fn is_locked(&self) -> bool {
        if let Some(locked_until) = &self.locked_until {
            chrono::Utc::now() < locked_until.0
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synapse::blockchain::StakingRequirements;
    use crate::synapse::blockchain::Transaction;
    use crate::synapse::blockchain::block::{Block, RegistrationTransaction, StakePurpose};
    use chrono::Utc;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn create_test_config() -> StakingRequirements {
        StakingRequirements {
            min_stake_amount: 50,
            max_stake_amount: 1000,
            min_stake_for_report: 20,
            min_stake_for_consensus: 100,
            slash_percentage: 10.0,
        }
    }

    fn create_test_chain_with_participant(
        participant_id: &str,
        trust_points: u32,
    ) -> Arc<RwLock<Vec<Block>>> {
        let mut blocks = Vec::new();
        let registration = RegistrationTransaction {
            id: format!("reg_{}", participant_id),
            participant_id: participant_id.to_string(),
            public_key: b"test_key".to_vec(),
            initial_trust_points: trust_points,
            entity_type: "validator".to_string(),
            timestamp: DateTimeWrapper::new(Utc::now()),
            signature: b"test_signature".to_vec(),
        };
        let transaction = Transaction::Registration(registration);
        let block = Block {
            number: 0,
            timestamp: DateTimeWrapper::new(Utc::now()),
            previous_hash: "genesis".to_string(),
            transactions: vec![transaction],
            hash: "block_hash".to_string(),
            nonce: 0,
            validator: "genesis".to_string(),
            signature: None,
        };
        blocks.push(block);
        Arc::new(RwLock::new(blocks))
    }

    #[tokio::test]
    async fn test_staking_manager_initialization() {
        let config = create_test_config();
        let chain = create_test_chain_with_participant("validator1", 500);

        let manager = StakingManager::new(config.clone(), chain);

        assert_eq!(manager.config.min_stake_amount, 50);
        assert_eq!(manager.config.max_stake_amount, 1000);
        assert_eq!(manager.config.min_stake_for_consensus, 100);
    }

    #[tokio::test]
    async fn test_stake_points_valid() {
        let config = create_test_config();
        let chain = create_test_chain_with_participant("validator1", 500);
        let manager = StakingManager::new(config, chain);

        // Test valid stake
        let result = manager
            .stake_points("validator1", 100, StakePurpose::ConsensusValidator)
            .await;
        assert!(
            result.is_ok(),
            "Valid stake should succeed: {:?}",
            result.err()
        );

        // Verify stake was recorded
        let staked_points = manager.get_staked_points("validator1").await.unwrap();
        assert_eq!(staked_points, 100, "Staked points should be recorded");

        // Verify available points reduced
        let available = manager.get_available_stake("validator1").await.unwrap();
        assert_eq!(available, 400, "Available stake should be reduced");
    }

    #[tokio::test]
    async fn test_stake_points_insufficient_minimum() {
        let config = create_test_config();
        let chain = create_test_chain_with_participant("validator1", 500);
        let manager = StakingManager::new(config, chain);

        // Test below minimum stake
        let result = manager
            .stake_points("validator1", 25, StakePurpose::ConsensusValidator)
            .await;
        assert!(result.is_err(), "Below minimum stake should fail");
        assert!(result.unwrap_err().to_string().contains("below minimum"));
    }

    #[tokio::test]
    async fn test_stake_points_exceeds_maximum() {
        let config = create_test_config();
        let chain = create_test_chain_with_participant("validator1", 2000);
        let manager = StakingManager::new(config, chain);

        // Test above maximum stake
        let result = manager
            .stake_points("validator1", 1500, StakePurpose::ConsensusValidator)
            .await;
        assert!(result.is_err(), "Above maximum stake should fail");
        assert!(result.unwrap_err().to_string().contains("exceeds maximum"));
    }

    #[tokio::test]
    async fn test_stake_points_insufficient_balance() {
        let config = create_test_config();
        let chain = create_test_chain_with_participant("validator1", 75);
        let manager = StakingManager::new(config, chain);

        // Test insufficient balance
        let result = manager
            .stake_points("validator1", 100, StakePurpose::ConsensusValidator)
            .await;
        assert!(result.is_err(), "Insufficient balance should fail");
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Insufficient available")
        );
    }

    #[tokio::test]
    async fn test_unstake_points_valid() {
        let config = create_test_config();
        let chain = create_test_chain_with_participant("validator1", 500);
        let manager = StakingManager::new(config, chain);

        // First stake some points
        let stake_id = manager
            .stake_points("validator1", 100, StakePurpose::ConsensusValidator)
            .await
            .unwrap();

        // Then unstake them
        let result = manager.unstake_points("validator1", &stake_id).await;
        assert!(
            result.is_ok(),
            "Valid unstake should succeed: {:?}",
            result.err()
        );
        assert_eq!(result.unwrap(), 100, "Unstaked amount should match");

        // Verify stake was removed
        let staked_points = manager.get_staked_points("validator1").await.unwrap();
        assert_eq!(
            staked_points, 0,
            "Staked points should be zero after unstaking"
        );
    }

    #[tokio::test]
    async fn test_unstake_points_nonexistent_stake() {
        let config = create_test_config();
        let chain = create_test_chain_with_participant("validator1", 500);
        let manager = StakingManager::new(config, chain);

        // Try to unstake non-existent stake
        let result = manager.unstake_points("validator1", "nonexistent").await;
        assert!(result.is_err(), "Non-existent stake should fail to unstake");
        assert!(result.unwrap_err().to_string().contains("Stake not found"));
    }

    #[tokio::test]
    async fn test_slash_stake_security_validation() {
        let config = create_test_config();
        let chain = create_test_chain_with_participant("validator1", 1000); // Increased trust points
        let manager = StakingManager::new(config, chain);

        // First stake some points
        let stake_id = manager
            .stake_points("validator1", 200, StakePurpose::ConsensusValidator)
            .await
            .unwrap();

        // Test slashing for malicious behavior
        let result = manager
            .slash_stake("validator1", &stake_id, "False consensus vote")
            .await;
        assert!(
            result.is_ok(),
            "Valid slash should succeed: {:?}",
            result.err()
        );

        // Verify stake was reduced by slash percentage (10% of 200 = 20)
        let staked_points = manager.get_staked_points("validator1").await.unwrap();
        assert_eq!(
            staked_points, 180,
            "Stake should be reduced by slash amount"
        );
    }

    #[tokio::test]
    async fn test_validator_selection_by_stake_weight() {
        let config = create_test_config();
        let mut blocks: Vec<Block> = Vec::new();

        // Create multiple validators with different stake amounts
        let validators = vec![
            ("validator1", 1000),
            ("validator2", 500),
            ("validator3", 200),
        ];

        for (id, trust_points) in validators {
            let registration = RegistrationTransaction {
                id: format!("reg_{}", id),
                participant_id: id.to_string(),
                public_key: format!("{}_key", id).as_bytes().to_vec(),
                initial_trust_points: trust_points,
                entity_type: "validator".to_string(),
                timestamp: DateTimeWrapper::new(Utc::now()),
                signature: b"test_signature".to_vec(),
            };
            let transaction = Transaction::Registration(registration);
            let block = Block {
                number: blocks.len() as u64,
                timestamp: DateTimeWrapper::new(Utc::now()),
                previous_hash: if blocks.is_empty() {
                    "genesis".to_string()
                } else {
                    blocks.last().unwrap().hash.clone()
                },
                transactions: vec![transaction],
                hash: format!("block_hash_{}", blocks.len()),
                nonce: 0,
                validator: "genesis".to_string(),
                signature: None,
            };
            blocks.push(block);
        }

        let chain = Arc::new(RwLock::new(blocks));
        let manager = StakingManager::new(config, chain);

        // Stake different amounts
        manager
            .stake_points("validator1", 300, StakePurpose::ConsensusValidator)
            .await
            .unwrap();
        manager
            .stake_points("validator2", 150, StakePurpose::ConsensusValidator)
            .await
            .unwrap();
        manager
            .stake_points("validator3", 100, StakePurpose::ConsensusValidator)
            .await
            .unwrap();

        // Test validator selection considers stake weight
        let validators = manager.get_consensus_validators().await.unwrap();
        assert_eq!(
            validators.len(),
            3,
            "Should return requested number of validators"
        );

        // Validator with highest stake should be first
        assert_eq!(
            validators[0], "validator1",
            "Highest stake validator should be selected first"
        );
    }
}
