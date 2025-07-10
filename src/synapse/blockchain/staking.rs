// Synapse Blockchain Staking Manager
// Manages trust point staking for consensus and reporting

use super::block::{Block, Transaction, StakePurpose};
use crate::synapse::models::trust::TrustBalance;
use anyhow::Result;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Manages trust point staking operations
pub struct StakingManager {
    config: super::StakingRequirements,
    chain: Arc<RwLock<Vec<Block>>>,
    active_stakes: DashMap<String, Vec<ActiveStake>>, // participant_id -> stakes
}

impl StakingManager {
    /// Create new staking manager
    pub async fn new(
        config: super::StakingRequirements,
        chain: Arc<RwLock<Vec<Block>>>,
    ) -> Result<Self> {
        Ok(Self {
            config,
            chain,
            active_stakes: DashMap::new(),
        })
    }
    
    /// Check if participant has sufficient stake for operation
    pub async fn has_sufficient_stake(&self, participant_id: &str, required_amount: u32) -> Result<bool> {
        let available_stake = self.get_available_stake(participant_id).await?;
        Ok(available_stake >= required_amount)
    }
    
    /// Get total available (unstaked) trust points for participant
    pub async fn get_available_stake(&self, participant_id: &str) -> Result<u32> {
        let total_points = self.get_total_trust_points(participant_id).await?;
        let staked_points = self.get_staked_points(participant_id).await?;
        Ok(total_points.saturating_sub(staked_points))
    }
    
    /// Get total trust points for participant
    pub async fn get_total_trust_points(&self, participant_id: &str) -> Result<u32> {
        let chain = self.chain.read().await;
        let mut total_points = 0u32;
        
        // Scan blockchain for all transactions affecting this participant
        for block in chain.iter() {
            for transaction in &block.transactions {
                match transaction {
                    // Registration gives initial points
                    Transaction::Registration(reg) if reg.participant_id == participant_id => {
                        total_points += reg.initial_trust_points;
                    }
                    // Transfers to this participant
                    Transaction::Transfer(transfer) if transfer.to_participant == participant_id => {
                        total_points += transfer.amount;
                    }
                    // Transfers from this participant
                    Transaction::Transfer(transfer) if transfer.from_participant == participant_id => {
                        total_points = total_points.saturating_sub(transfer.amount);
                    }
                    // Trust reports can award points if verified
                    Transaction::TrustReport(report) if report.subject_id == participant_id => {
                        if report.score > 0 {
                            // Positive reports award points (simplified - would need consensus verification)
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
        let stakes = self.active_stakes.get(participant_id)
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
        // Validate stake amount
        if amount < self.config.min_stake_amount {
            return Err(anyhow::anyhow!("Stake amount {} is below minimum {}", amount, self.config.min_stake_amount));
        }
        
        if amount > self.config.max_stake_amount {
            return Err(anyhow::anyhow!("Stake amount {} exceeds maximum {}", amount, self.config.max_stake_amount));
        }
        
        // Check if participant has enough available points
        if !self.has_sufficient_stake(participant_id, amount).await? {
            return Err(anyhow::anyhow!("Insufficient available trust points"));
        }
        
        // Create stake record
        let stake = ActiveStake {
            id: uuid::Uuid::new_v4().to_string(),
            participant_id: participant_id.to_string(),
            amount,
            purpose,
            staked_at: Utc::now(),
            locked_until: None, // Can be set based on purpose
        };
        
        // Add to active stakes
        self.active_stakes.entry(participant_id.to_string())
            .or_insert_with(Vec::new)
            .push(stake.clone());
        
        Ok(stake.id)
    }
    
    /// Unstake trust points
    pub async fn unstake_points(&self, participant_id: &str, stake_id: &str) -> Result<u32> {
        let mut found_stake: Option<ActiveStake> = None;
        let mut stake_index: Option<usize> = None;
        
        // Find the stake to remove
        if let Some(stakes) = self.active_stakes.get_mut(participant_id) {
            if let Some(index) = stakes.iter().position(|s| s.id == stake_id) {
                stake_index = Some(index);
                found_stake = Some(stakes[index].clone());
            }
        }
        
        let stake = found_stake.ok_or_else(|| anyhow::anyhow!("Stake not found"))?;
        let index = stake_index.unwrap();
        
        // Check if stake is still locked
        if let Some(locked_until) = stake.locked_until {
            if Utc::now() < locked_until {
                return Err(anyhow::anyhow!("Stake is still locked"));
            }
        }
        
        // Remove the stake
        if let Some(mut stakes) = self.active_stakes.get_mut(participant_id) {
            stakes.remove(index);
        }
        
        Ok(stake.amount)
    }
    
    /// Slash stake for false reports or bad behavior
    pub async fn slash_stake(&self, participant_id: &str, stake_id: &str, reason: &str) -> Result<u32> {
        let mut found_stake: Option<ActiveStake> = None;
        let mut stake_index: Option<usize> = None;
        
        // Find the stake to slash
        if let Some(stakes) = self.active_stakes.get_mut(participant_id) {
            if let Some(index) = stakes.iter().position(|s| s.id == stake_id) {
                stake_index = Some(index);
                found_stake = Some(stakes[index].clone());
            }
        }
        
        let stake = found_stake.ok_or_else(|| anyhow::anyhow!("Stake not found"))?;
        let index = stake_index.unwrap();
        let slashed_amount = (stake.amount as f64 * self.config.slash_percentage) as u32;
        let remaining_amount = stake.amount - slashed_amount;
        
        // Remove the original stake and add remaining if any
        if let Some(mut stakes) = self.active_stakes.get_mut(participant_id) {
            stakes.remove(index);
            
            // If there's remaining amount, create new stake
            if remaining_amount > 0 {
                let remaining_stake = ActiveStake {
                    id: uuid::Uuid::new_v4().to_string(),
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
        Ok(self.active_stakes.get(participant_id)
            .map(|entry| entry.value().clone())
            .unwrap_or_default())
    }
    
    /// Get participants eligible for consensus (have minimum stake)
    pub async fn get_consensus_validators(&self) -> Result<Vec<String>> {
        let mut validators = Vec::new();
        
        for entry in self.active_stakes.iter() {
            let participant_id = entry.key();
            let participant_stakes = entry.value();
            let total_consensus_stake = participant_stakes.iter()
                .filter(|s| matches!(s.purpose, StakePurpose::ConsensusValidator))
                .map(|s| s.amount)
                .sum::<u32>();
            
            if total_consensus_stake >= self.config.min_stake_for_consensus {
                validators.push(participant_id.clone());
            }
        }
        
        Ok(validators)
    }
    
    /// Lock stake for a specific period (e.g., during consensus participation)
    pub async fn lock_stake(&self, participant_id: &str, stake_id: &str, lock_duration: chrono::Duration) -> Result<()> {
        if let Some(mut stakes) = self.active_stakes.get_mut(participant_id) {
            if let Some(stake) = stakes.iter_mut().find(|s| s.id == stake_id) {
                stake.locked_until = Some(Utc::now() + lock_duration);
                return Ok(());
            }
        }
        
        Err(anyhow::anyhow!("Stake not found"))
    }
    
    /// Get all participant IDs in the system
    pub async fn get_all_participants(&self) -> Result<Vec<String>> {
        // For now, return participants who have stakes
        let mut participants = Vec::new();
        
        for entry in self.active_stakes.iter() {
            participants.push(entry.key().clone());
        }
        
        Ok(participants)
    }
    
    /// Get all balances for a participant
    pub async fn get_participant_balances(&self, participant_id: &str) -> Result<Vec<TrustBalance>> {
        // In a real implementation, this would query the database for all balances
        // For now, create a mock balance from stakes
        let stakes = self.get_participant_stakes(participant_id).await?;
        
        if stakes.is_empty() {
            return Ok(vec![]);
        }
        
        // Calculate total staked amount
        let staked_amount: u32 = stakes.iter().map(|s| s.amount).sum();
        
        // Get total trust points from blockchain
        let total_points = self.get_total_trust_points(participant_id).await?;
        
        // Create balance record
        let balance = TrustBalance {
            participant_id: participant_id.to_string(),
            total_points,
            available_points: total_points.saturating_sub(staked_amount),
            staked_points: staked_amount,
            earned_lifetime: total_points,
            last_activity: stakes.iter()
                .map(|s| s.staked_at)
                .max()
                .unwrap_or_else(chrono::Utc::now),
            decay_rate: 0.02, // Default 2% per month
        };
        
        Ok(vec![balance])
    }
    
    /// Update a participant's trust balance
    pub async fn update_balance(&self, participant_id: &str, balance: &TrustBalance) -> Result<()> {
        // In a real implementation, this would update the database
        // For now, just log the update
        tracing::info!(
            "Updated balance for {}: {} total, {} available, {} staked",
            participant_id,
            balance.total_points,
            balance.available_points,
            balance.staked_points
        );
        
        // We can't actually update the balance in this mock implementation
        // since we don't have a reference to the database
        
        Ok(())
    }
}

/// An active stake record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveStake {
    pub id: String,
    pub participant_id: String,
    pub amount: u32,
    pub purpose: StakePurpose,
    pub staked_at: DateTime<Utc>,
    pub locked_until: Option<DateTime<Utc>>,
}

impl ActiveStake {
    /// Check if stake is currently locked
    pub fn is_locked(&self) -> bool {
        if let Some(locked_until) = self.locked_until {
            Utc::now() < locked_until
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::StakingRequirements;
    use crate::synapse::blockchain::block;
    use tokio;
    use chrono::Duration;

    fn create_test_config() -> StakingRequirements {
        StakingRequirements {
            min_stake_amount: 100,
            max_stake_amount: 10000,
            min_stake_for_report: 10,
            min_stake_for_consensus: 500,
            slash_percentage: 0.2, // 20% slash
        }
    }

    // Helper function for creating test stakes directly (kept for future test scenarios)
    #[allow(dead_code)]
    fn create_test_stake(id: &str, participant_id: &str, amount: u32, purpose: StakePurpose) -> ActiveStake {
        ActiveStake {
            id: id.to_string(),
            participant_id: participant_id.to_string(),
            amount,
            purpose,
            staked_at: Utc::now(),
            locked_until: None,
        }
    }

    async fn create_test_blockchain_with_registration(participant_id: &str, trust_points: u32) -> Arc<RwLock<Vec<Block>>> {
        let registration = block::RegistrationTransaction {
            id: uuid::Uuid::new_v4().to_string(),
            participant_id: participant_id.to_string(),
            public_key: vec![1, 2, 3, 4], // dummy key
            initial_trust_points: trust_points,
            entity_type: "test".to_string(),
            timestamp: Utc::now(),
            signature: vec![1, 2, 3, 4], // dummy signature
        };
        
        let mut block = Block {
            number: 0,
            timestamp: Utc::now(),
            previous_hash: "0".repeat(64),
            hash: String::new(),
            transactions: vec![Transaction::Registration(registration)],
            nonce: 0,
            validator: "test".to_string(),
        };
        
        // Calculate block hash
        block.hash = block.calculate_hash();
        
        Arc::new(RwLock::new(vec![block]))
    }

    #[tokio::test]
    async fn test_stake_points() {
        let config = create_test_config();
        let chain = create_test_blockchain_with_registration("alice", 2000).await;
        let manager = StakingManager::new(config, chain).await.unwrap();
        
        // Test staking
        let result = manager.stake_points("alice", 1000, StakePurpose::TrustReporting).await;
        if let Err(e) = &result {
            eprintln!("Staking failed with error: {}", e);
        }
        assert!(result.is_ok());
        let _stake_id = result.unwrap();
        
        // Verify stake was added
        let stakes = manager.get_participant_stakes("alice").await.unwrap();
        assert_eq!(stakes.len(), 1);
        assert_eq!(stakes[0].amount, 1000);
        assert_eq!(stakes[0].participant_id, "alice");
    }

    #[tokio::test]
    async fn test_unstake_points_success() {
        let config = create_test_config();
        let chain = create_test_blockchain_with_registration("alice", 2000).await;
        let manager = StakingManager::new(config, chain).await.unwrap();
        
        // First stake some points
        let stake_id = manager.stake_points("alice", 1000, StakePurpose::TrustReporting).await.unwrap();
        
        // Then unstake them
        let result = manager.unstake_points("alice", &stake_id).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1000);
        
        // Verify stake was removed
        let stakes = manager.get_participant_stakes("alice").await.unwrap();
        assert_eq!(stakes.len(), 0);
    }

    #[tokio::test]
    async fn test_unstake_points_not_found() {
        let config = create_test_config();
        let chain = create_test_blockchain_with_registration("alice", 2000).await;
        let manager = StakingManager::new(config, chain).await.unwrap();
        
        // Try to unstake non-existent stake
        let result = manager.unstake_points("alice", "non_existent_id").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Stake not found"));
    }

    #[tokio::test]
    async fn test_unstake_points_locked_stake() {
        let config = create_test_config();
        let chain = create_test_blockchain_with_registration("alice", 2000).await;
        let manager = StakingManager::new(config, chain).await.unwrap();
        
        // Stake some points
        let stake_id = manager.stake_points("alice", 1000, StakePurpose::TrustReporting).await.unwrap();
        
        // Lock the stake
        let lock_duration = Duration::hours(1);
        manager.lock_stake("alice", &stake_id, lock_duration).await.unwrap();
        
        // Try to unstake locked stake
        let result = manager.unstake_points("alice", &stake_id).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Stake is still locked"));
    }

    #[tokio::test]
    async fn test_slash_stake_full() {
        let config = create_test_config();
        let chain = create_test_blockchain_with_registration("alice", 2000).await;
        let manager = StakingManager::new(config, chain).await.unwrap();
        
        // Stake some points
        let stake_id = manager.stake_points("alice", 1000, StakePurpose::ConsensusValidator).await.unwrap();
        
        // Slash the stake
        let result = manager.slash_stake("alice", &stake_id, "misbehavior").await;
        assert!(result.is_ok());
        
        // Should slash 20% = 200 points
        let slashed_amount = result.unwrap();
        assert_eq!(slashed_amount, 200);
        
        // Check remaining stake
        let stakes = manager.get_participant_stakes("alice").await.unwrap();
        assert_eq!(stakes.len(), 1);
        assert_eq!(stakes[0].amount, 800); // 1000 - 200
    }

    #[tokio::test]
    async fn test_slash_stake_complete_slash() {
        let config = StakingRequirements {
            min_stake_amount: 100,
            max_stake_amount: 10000,
            min_stake_for_report: 10,
            min_stake_for_consensus: 500,
            slash_percentage: 1.0, // 100% slash
        };
        let chain = create_test_blockchain_with_registration("alice", 2000).await;
        let manager = StakingManager::new(config, chain).await.unwrap();
        
        // Stake some points
        let stake_id = manager.stake_points("alice", 1000, StakePurpose::ConsensusValidator).await.unwrap();
        
        // Slash the stake completely
        let result = manager.slash_stake("alice", &stake_id, "severe_misbehavior").await;
        assert!(result.is_ok());
        
        // Should slash 100% = 1000 points
        let slashed_amount = result.unwrap();
        assert_eq!(slashed_amount, 1000);
        
        // No remaining stake
        let stakes = manager.get_participant_stakes("alice").await.unwrap();
        assert_eq!(stakes.len(), 0);
    }

    #[tokio::test]
    async fn test_slash_stake_not_found() {
        let config = create_test_config();
        let chain = create_test_blockchain_with_registration("alice", 2000).await;
        let manager = StakingManager::new(config, chain).await.unwrap();
        
        // Try to slash non-existent stake
        let result = manager.slash_stake("alice", "non_existent_id", "reason").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Stake not found"));
    }

    #[tokio::test]
    async fn test_multiple_stakes_management() {
        let config = create_test_config();
        let chain = create_test_blockchain_with_registration("alice", 4000).await;
        let manager = StakingManager::new(config, chain).await.unwrap();
        
        // Create multiple stakes for the same participant
        let _stake_id1 = manager.stake_points("alice", 1000, StakePurpose::TrustReporting).await.unwrap();
        let stake_id2 = manager.stake_points("alice", 500, StakePurpose::ConsensusValidator).await.unwrap();
        let _stake_id3 = manager.stake_points("alice", 2000, StakePurpose::TrustReporting).await.unwrap();
        
        // Verify all stakes exist
        let stakes = manager.get_participant_stakes("alice").await.unwrap();
        assert_eq!(stakes.len(), 3);
        
        // Unstake the middle one
        let result = manager.unstake_points("alice", &stake_id2).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 500);
        
        // Verify correct stake was removed
        let stakes = manager.get_participant_stakes("alice").await.unwrap();
        assert_eq!(stakes.len(), 2);
        
        // Check remaining stakes are correct
        let amounts: Vec<u32> = stakes.iter().map(|s| s.amount).collect();
        assert!(amounts.contains(&1000));
        assert!(amounts.contains(&2000));
        assert!(!amounts.contains(&500));
    }

    #[tokio::test]
    async fn test_consensus_validators() {
        let config = create_test_config();
        let mut blocks = Vec::new();
        
        // Register each participant with sufficient trust points
        blocks.push(create_test_blockchain_with_registration("alice", 1000).await.read().await[0].clone());
        blocks.push(create_test_blockchain_with_registration("bob", 1000).await.read().await[0].clone());
        blocks.push(create_test_blockchain_with_registration("charlie", 1000).await.read().await[0].clone());
        blocks.push(create_test_blockchain_with_registration("dave", 1000).await.read().await[0].clone());
        
        let chain = Arc::new(RwLock::new(blocks));
        let manager = StakingManager::new(config, chain).await.unwrap();
        
        // Alice has enough consensus stake
        manager.stake_points("alice", 600, StakePurpose::ConsensusValidator).await.unwrap();
        
        // Bob doesn't have enough
        manager.stake_points("bob", 300, StakePurpose::ConsensusValidator).await.unwrap();
        
        // Charlie has enough through multiple stakes
        manager.stake_points("charlie", 300, StakePurpose::ConsensusValidator).await.unwrap();
        manager.stake_points("charlie", 300, StakePurpose::ConsensusValidator).await.unwrap();
        
        // Dave has enough total but not for consensus (wrong purpose)
        manager.stake_points("dave", 1000, StakePurpose::TrustReporting).await.unwrap();
        
        let validators = manager.get_consensus_validators().await.unwrap();
        assert_eq!(validators.len(), 2);
        assert!(validators.contains(&"alice".to_string()));
        assert!(validators.contains(&"charlie".to_string()));
        assert!(!validators.contains(&"bob".to_string()));
        assert!(!validators.contains(&"dave".to_string()));
    }

    #[tokio::test]
    async fn test_stake_locking() {
        let config = create_test_config();
        let chain = create_test_blockchain_with_registration("alice", 2000).await;
        let manager = StakingManager::new(config, chain).await.unwrap();
        
        // Stake some points
        let stake_id = manager.stake_points("alice", 1000, StakePurpose::TrustReporting).await.unwrap();
        
        // Lock the stake
        let lock_duration = Duration::seconds(1); // Short duration for testing
        manager.lock_stake("alice", &stake_id, lock_duration).await.unwrap();
        
        // Verify stake is locked
        let stakes = manager.get_participant_stakes("alice").await.unwrap();
        assert!(stakes[0].is_locked());
        
        // Wait for lock to expire
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        
        // Verify stake is no longer locked
        let stakes = manager.get_participant_stakes("alice").await.unwrap();
        assert!(!stakes[0].is_locked());
    }

    #[tokio::test]
    async fn test_stake_validation() {
        let config = create_test_config();
        // Create a blockchain and register participants with 1000 trust points
        let chain = create_test_blockchain_with_registration("alice", 1000).await;
        let manager = StakingManager::new(config, chain).await.unwrap();
        
        // Test minimum stake validation
        let result = manager.stake_points("alice", 50, StakePurpose::TrustReporting).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("below minimum"));
        
        // Test maximum stake validation  
        let result = manager.stake_points("alice", 20000, StakePurpose::TrustReporting).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exceeds maximum"));
        
        // Test valid stake
        let result = manager.stake_points("alice", 500, StakePurpose::TrustReporting).await;
        assert!(result.is_ok());
    }
}
