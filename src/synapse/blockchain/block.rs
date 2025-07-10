// Synapse Blockchain Block Structure

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// A block in the Synapse blockchain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub number: u64,
    pub timestamp: DateTime<Utc>,
    pub previous_hash: String,
    pub hash: String,
    pub transactions: Vec<Transaction>,
    pub nonce: u64,
    pub validator: String, // Node that validated this block
}

impl Block {
    /// Create the genesis block
    pub fn genesis() -> Self {
        let mut block = Self {
            number: 0,
            timestamp: Utc::now(),
            previous_hash: "0".repeat(64),
            hash: String::new(),
            transactions: vec![],
            nonce: 0,
            validator: "genesis".to_string(),
        };
        
        block.hash = block.calculate_hash();
        block
    }
    
    /// Create a new block
    pub fn new(number: u64, previous_hash: String, transactions: Vec<Transaction>, validator: String) -> Self {
        let mut block = Self {
            number,
            timestamp: Utc::now(),
            previous_hash,
            hash: String::new(),
            transactions,
            nonce: 0,
            validator,
        };
        
        block.hash = block.calculate_hash();
        block
    }
    
    /// Calculate block hash
    pub fn calculate_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.number.to_be_bytes());
        hasher.update(self.timestamp.timestamp().to_be_bytes());
        hasher.update(&self.previous_hash);
        hasher.update(self.nonce.to_be_bytes());
        hasher.update(&self.validator);
        
        for transaction in &self.transactions {
            hasher.update(transaction.hash());
        }
        
        format!("{:x}", hasher.finalize())
    }
    
    /// Verify block integrity
    pub fn verify(&self, previous_block: Option<&Block>) -> bool {
        // Check hash is correct
        if self.hash != self.calculate_hash() {
            return false;
        }
        
        // Check previous hash links correctly
        if let Some(prev) = previous_block {
            if self.previous_hash != prev.hash {
                return false;
            }
            if self.number != prev.number + 1 {
                return false;
            }
        }
        
        // Verify all transactions
        for transaction in &self.transactions {
            if !transaction.verify() {
                return false;
            }
        }
        
        true
    }
}

/// Transactions that can be stored in blocks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Transaction {
    /// Trust report about a participant
    TrustReport(TrustReport),
    /// Stake trust points
    Stake(StakeTransaction),
    /// Unstake trust points
    Unstake(UnstakeTransaction),
    /// Transfer trust points
    Transfer(TransferTransaction),
    /// Participant registration
    Registration(RegistrationTransaction),
}

impl Transaction {
    /// Get transaction ID
    pub fn id(&self) -> String {
        format!("{:x}", Sha256::digest(self.hash()))
    }
    
    /// Calculate transaction hash
    pub fn hash(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }
    
    /// Verify transaction is valid
    pub fn verify(&self) -> bool {
        match self {
            Transaction::TrustReport(report) => report.verify(),
            Transaction::Stake(stake) => stake.verify(),
            Transaction::Unstake(unstake) => unstake.verify(),
            Transaction::Transfer(transfer) => transfer.verify(),
            Transaction::Registration(reg) => reg.verify(),
        }
    }
}

/// A trust report submitted to the blockchain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustReport {
    pub id: String,
    pub reporter_id: String,
    pub subject_id: String,
    pub report_type: TrustReportType,
    pub score: i8, // -100 to +100
    pub category: String,
    pub evidence_hash: Option<String>, // Hash of evidence data
    pub stake_amount: u32, // Trust points staked on this report
    pub timestamp: DateTime<Utc>,
    pub signature: Vec<u8>, // Digital signature from reporter
}

impl TrustReport {
    /// Create new trust report
    pub fn new(
        reporter_id: String,
        subject_id: String,
        report_type: TrustReportType,
        score: i8,
        category: String,
        stake_amount: u32,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            reporter_id,
            subject_id,
            report_type,
            score,
            category,
            evidence_hash: None,
            stake_amount,
            timestamp: Utc::now(),
            signature: vec![], // Would be populated with actual signature
        }
    }
    
    /// Verify report is valid
    pub fn verify(&self) -> bool {
        // Check score is in valid range
        if self.score < -100 || self.score > 100 {
            return false;
        }
        
        // Check stake amount is positive
        if self.stake_amount == 0 {
            return false;
        }
        
        // Check reporter and subject are different
        if self.reporter_id == self.subject_id {
            return false;
        }
        
        // TODO: Verify digital signature
        
        true
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TrustReportType {
    /// Report good behavior
    Positive,
    /// Report bad behavior
    Negative,
    /// Verify identity claims
    IdentityVerification,
    /// Report on collaboration quality
    CollaborationFeedback,
}

/// Stake trust points transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakeTransaction {
    pub id: String,
    pub participant_id: String,
    pub amount: u32,
    pub purpose: StakePurpose,
    pub timestamp: DateTime<Utc>,
    pub signature: Vec<u8>,
}

impl StakeTransaction {
    pub fn new(participant_id: String, amount: u32, purpose: StakePurpose) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            participant_id,
            amount,
            purpose,
            timestamp: Utc::now(),
            signature: vec![],
        }
    }
    
    pub fn verify(&self) -> bool {
        self.amount > 0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StakePurpose {
    /// Stake to become consensus validator
    ConsensusValidator,
    /// Stake to submit trust reports
    TrustReporting,
    /// Stake for identity verification
    IdentityVerification,
}

/// Unstake trust points transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnstakeTransaction {
    pub id: String,
    pub participant_id: String,
    pub amount: u32,
    pub stake_id: String, // Reference to original stake
    pub timestamp: DateTime<Utc>,
    pub signature: Vec<u8>,
}

impl UnstakeTransaction {
    pub fn verify(&self) -> bool {
        self.amount > 0
    }
}

/// Transfer trust points between participants
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferTransaction {
    pub id: String,
    pub from_participant: String,
    pub to_participant: String,
    pub amount: u32,
    pub reason: String,
    pub timestamp: DateTime<Utc>,
    pub signature: Vec<u8>,
}

impl TransferTransaction {
    pub fn verify(&self) -> bool {
        self.amount > 0 && self.from_participant != self.to_participant
    }
}

/// Participant registration transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationTransaction {
    pub id: String,
    pub participant_id: String,
    pub public_key: Vec<u8>,
    pub initial_trust_points: u32,
    pub entity_type: String,
    pub timestamp: DateTime<Utc>,
    pub signature: Vec<u8>,
}

impl RegistrationTransaction {
    pub fn verify(&self) -> bool {
        !self.participant_id.is_empty() && !self.public_key.is_empty()
    }
}
