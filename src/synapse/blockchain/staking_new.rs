// Synapse Blockchain Staking Manager
// Manages trust point staking for consensus and reporting

use super::block::{Block, Transaction, StakePurpose};
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
    pub async fn has_sufficient_stake(
        &self,
        participant_id: &str,
        purpose: &StakePurpose,
    ) -> Result<bool> {
        let required_amount = match purpose {
            StakePurpose::Consensus => self.config.consensus_threshold,
            StakePurpose::Reporting => self.config.reporting_threshold,
        };
        
        let total_staked = self.get_total_staked(participant_id).await?;
        Ok(total_staked >= required_amount)
    }
    
    /// Get total staked amount for participant
    pub async fn get_total_staked(&self, participant_id: &str) -> Result<u32> {
        if let Some(stakes) = self.active_stakes.get(participant_id) {
            Ok(stakes.iter().map(|s| s.amount).sum())
        } else {
            Ok(0)
        }
    }
    
    /// Create new stake
    pub async fn create_stake(
        &self,
        participant_id: String,
        amount: u32,
        purpose: StakePurpose,
    ) -> Result<String> {
        let stake_id = format!("stake_{}", UuidWrapper::new(uuid::UuidWrapper::new(Uuid::new_v4())));
        
        let stake = ActiveStake {
            id: stake_id.clone(),
            participant_id: participant_id.clone(),
            amount,
            purpose,
            staked_at: DateTimeWrapper::new(Utc::now()),
            locked_until: None,
        };
        
        self.active_stakes
            .entry(participant_id)
            .or_insert_with(Vec::new)
            .push(stake);
        
        Ok(stake_id)
    }
    
    /// Slash stake for misbehavior
    pub async fn slash_stake(
        &self,
        participant_id: &str,
        stake_id: &str,
        percentage: f64,
    ) -> Result<u32> {
        let mut slashed_amount = 0;
        
        if let Some(mut stakes) = self.active_stakes.get_mut(participant_id) {
            if let Some(stake) = stakes.iter_mut().find(|s| s.id == stake_id) {
                let slash_amount = (stake.amount as f64 * percentage) as u32;
                stake.amount -= slash_amount;
                slashed_amount = slash_amount;
                
                // Remove stake if fully slashed
                if stake.amount == 0 {
                    stakes.retain(|s| s.id != stake_id);
                }
            }
        }
        
        if slashed_amount > 0 {
            Ok(slashed_amount)
        } else {
            Err(anyhow::anyhow!("Stake not found"))
        }
    }
    
    /// Remove stake (unstake)
    pub async fn remove_stake(
        &self,
        participant_id: &str,
        stake_id: &str,
    ) -> Result<u32> {
        if let Some(mut stakes) = self.active_stakes.get_mut(participant_id) {
            if let Some(index) = stakes.iter().position(|s| s.id == stake_id) {
                let stake = stakes.remove(index);
                return Ok(stake.amount);
            }
        }
        
        Err(anyhow::anyhow!("Stake not found"))
    }
    
    /// Get stakes for participant
    pub async fn get_stakes(&self, participant_id: &str) -> Result<Vec<ActiveStake>> {
        if let Some(stakes) = self.active_stakes.get(participant_id) {
            Ok(stakes.clone())
        } else {
            Ok(Vec::new())
        }
    }
    
    /// Lock stake for period
    pub async fn lock_stake(
        &self,
        participant_id: &str,
        stake_id: &str,
        duration: chrono::Duration,
    ) -> Result<()> {
        if let Some(mut stakes) = self.active_stakes.get_mut(participant_id) {
            if let Some(stake) = stakes.iter_mut().find(|s| s.id == stake_id) {
                stake.locked_until = Some(DateTimeWrapper::new(Utc::now()) + duration);
                return Ok(());
            }
        }
        
        Err(anyhow::anyhow!("Stake not found"))
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
            DateTimeWrapper::new(Utc::now()) < locked_until
        } else {
            false
        }
    }
}
