use crate::synapse::blockchain::serialization::DateTimeWrapper;
use crate::synapse::blockchain::serialization::UuidWrapper;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Block {
    pub number: u64,
    pub timestamp: crate::synapse::blockchain::serialization::DateTimeWrapper,
    pub previous_hash: String,
    pub hash: String,
    pub transactions: Vec<crate::synapse::blockchain::Transaction>,
    pub nonce: u64,
    pub validator: String,
    pub signature: Option<BlockSignature>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockSignature {
    pub data: Vec<u8>,
    pub algorithm: String,
}

impl bincode::Encode for BlockSignature {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode::error::EncodeError> {
        self.data.encode(encoder)?;
        self.algorithm.encode(encoder)?;
        Ok(())
    }
}

impl<Context> bincode::Decode<Context> for BlockSignature {
    fn decode<D: bincode::de::Decoder>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        Ok(BlockSignature {
            data: bincode::Decode::decode(decoder)?,
            algorithm: bincode::Decode::decode(decoder)?,
        })
    }
}

// Remove derive for Encode/Decode, implement manually below
impl bincode::Encode for Block {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode::error::EncodeError> {
        self.number.encode(encoder)?;
        self.timestamp.encode(encoder)?;
        self.previous_hash.encode(encoder)?;
        self.hash.encode(encoder)?;
        self.transactions.encode(encoder)?;
        self.nonce.encode(encoder)?;
        self.validator.encode(encoder)?;
        self.signature.encode(encoder)?;
        Ok(())
    }
}

impl<Context> bincode::Decode<Context> for Block {
    fn decode<D: bincode::de::Decoder>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        Ok(Block {
            number: bincode::Decode::decode(decoder)?,
            timestamp: bincode::Decode::decode(decoder)?,
            previous_hash: bincode::Decode::decode(decoder)?,
            hash: bincode::Decode::decode(decoder)?,
            transactions: bincode::Decode::decode(decoder)?,
            nonce: bincode::Decode::decode(decoder)?,
            validator: bincode::Decode::decode(decoder)?,
            signature: bincode::Decode::decode(decoder)?,
        })
    }
}

impl Block {
    /// Create the genesis block
    pub fn genesis() -> Self {
        let mut block = Self {
            number: 0,
            timestamp: DateTimeWrapper::new(chrono::Utc::now()),
            previous_hash: "0".repeat(64),
            hash: String::new(),
            transactions: vec![],
            nonce: 0,
            validator: "genesis".to_string(),
            signature: None, // Genesis block has no signature
        };

        block.hash = block.calculate_hash();
        block
    }

    /// Create a new block
    pub fn new(
        number: u64,
        previous_hash: String,
        transactions: Vec<Transaction>,
        validator: String,
    ) -> Self {
        let mut block = Self {
            number,
            timestamp: DateTimeWrapper::new(chrono::Utc::now()),
            previous_hash,
            hash: String::new(),
            transactions,
            nonce: 0,
            validator,
            signature: None, // Signature will be added after block creation
        };
        block.hash = block.calculate_hash();
        block
    }

    /// Calculate block hash
    pub fn calculate_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.number.to_be_bytes());
        hasher.update(self.timestamp.0.timestamp().to_be_bytes());
        hasher.update(&self.previous_hash);
        hasher.update(self.nonce.to_be_bytes());
        hasher.update(&self.validator);
        for transaction in &self.transactions {
            hasher.update(transaction.hash());
        }
        // Note: signature is not included in hash calculation
        format!("{:x}", hasher.finalize())
    }

    /// Sign the block with a validator's private key
    pub async fn sign_block(
        &mut self,
        _validator_private_key: &[u8],
        algorithm: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Create data to sign (block without signature)
        let mut block_to_sign = self.clone();
        block_to_sign.signature = None;

        let block_bytes = serde_json::to_vec(&block_to_sign)?;

        // Use the authentication system for signing
        let mut auth_manager = crate::synapse::auth::utils::SynapseKeyManager::new().await?;

        // Create a temporary keypair for signing
        let _temp_keypair = auth_manager
            .generate_keypair(
                "temp_validator_key",
                crate::synapse::auth::utils::KeyAlgorithm::Ed25519,
            )
            .await?;

        let signature_data = auth_manager
            .sign("temp_validator_key", &block_bytes)
            .await?;

        self.signature = Some(BlockSignature {
            data: signature_data,
            algorithm: algorithm.to_string(),
        });

        Ok(())
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

    pub fn to_bytes(&self) -> Result<Vec<u8>, bincode::error::EncodeError> {
        bincode::encode_to_vec(self, bincode::config::standard())
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::error::DecodeError> {
        bincode::decode_from_slice(bytes, bincode::config::standard()).map(|r| r.0)
    }
}

// Remove derive for Encode/Decode, implement manually below
impl bincode::Encode for Transaction {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode::error::EncodeError> {
        match self {
            Transaction::TrustReport(r) => {
                0u8.encode(encoder)?;
                r.encode(encoder)
            }
            Transaction::Stake(s) => {
                1u8.encode(encoder)?;
                s.encode(encoder)
            }
            Transaction::Unstake(u) => {
                2u8.encode(encoder)?;
                u.encode(encoder)
            }
            Transaction::Transfer(t) => {
                3u8.encode(encoder)?;
                t.encode(encoder)
            }
            Transaction::Registration(r) => {
                4u8.encode(encoder)?;
                r.encode(encoder)
            }
        }
    }
}

impl<Context> bincode::Decode<Context> for Transaction {
    fn decode<D: bincode::de::Decoder>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        match u8::decode(decoder)? {
            0 => Ok(Transaction::TrustReport(bincode::Decode::decode(decoder)?)),
            1 => Ok(Transaction::Stake(bincode::Decode::decode(decoder)?)),
            2 => Ok(Transaction::Unstake(bincode::Decode::decode(decoder)?)),
            3 => Ok(Transaction::Transfer(bincode::Decode::decode(decoder)?)),
            4 => Ok(Transaction::Registration(bincode::Decode::decode(decoder)?)),
            _ => Err(bincode::error::DecodeError::OtherString(
                "Invalid Transaction variant".to_string(),
            )),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "serde")]
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

    /// Get hash of this transaction
    pub fn hash(&self) -> Vec<u8> {
        let mut hasher = Sha256::new();
        match self {
            Transaction::TrustReport(report) => {
                hasher.update(&report.id);
                hasher.update(&report.reporter_id);
                hasher.update(&report.subject_id);
                hasher.update(report.timestamp.0.timestamp().to_be_bytes());
            }
            Transaction::Stake(stake) => {
                hasher.update(&stake.id);
                hasher.update(&stake.participant_id);
                hasher.update(stake.amount.to_be_bytes());
                hasher.update(stake.timestamp.0.timestamp().to_be_bytes());
            }
            // Handle other transaction types...
            _ => {}
        }
        hasher.finalize().to_vec()
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

    pub fn to_bytes(&self) -> Result<Vec<u8>, bincode::error::EncodeError> {
        bincode::encode_to_vec(self, bincode::config::standard())
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::error::DecodeError> {
        bincode::decode_from_slice(bytes, bincode::config::standard()).map(|r| r.0)
    }
}

// Remove derive for Encode/Decode, implement manually below
impl bincode::Encode for TrustReport {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode::error::EncodeError> {
        self.id.encode(encoder)?;
        self.reporter_id.encode(encoder)?;
        self.subject_id.encode(encoder)?;
        self.report_type.encode(encoder)?;
        self.score.encode(encoder)?;
        self.category.encode(encoder)?;
        self.evidence_hash.encode(encoder)?;
        self.stake_amount.encode(encoder)?;
        self.timestamp.encode(encoder)?;
        self.signature.encode(encoder)?;
        Ok(())
    }
}

impl<Context> bincode::Decode<Context> for TrustReport {
    fn decode<D: bincode::de::Decoder>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        Ok(TrustReport {
            id: bincode::Decode::decode(decoder)?,
            reporter_id: bincode::Decode::decode(decoder)?,
            subject_id: bincode::Decode::decode(decoder)?,
            report_type: bincode::Decode::decode(decoder)?,
            score: bincode::Decode::decode(decoder)?,
            category: bincode::Decode::decode(decoder)?,
            evidence_hash: bincode::Decode::decode(decoder)?,
            stake_amount: bincode::Decode::decode(decoder)?,
            timestamp: bincode::Decode::decode(decoder)?,
            signature: bincode::Decode::decode(decoder)?,
        })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "serde")]
pub struct TrustReport {
    pub id: String,
    pub reporter_id: String,
    pub subject_id: String,
    pub report_type: TrustReportType,
    pub score: i8, // -100 to +100
    pub category: String,
    pub evidence_hash: Option<String>, // Hash of evidence data
    pub stake_amount: u32,             // Trust points staked on this report
    pub timestamp: DateTimeWrapper,
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
            id: UuidWrapper::new(Uuid::new_v4()).to_string(),
            reporter_id,
            subject_id,
            report_type,
            score,
            category,
            evidence_hash: None,
            stake_amount,
            timestamp: DateTimeWrapper::new(chrono::Utc::now()),
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

        // Verify digital signature
        if !self.signature.is_empty() {
            use ed25519_dalek::{Signature, VerifyingKey};
            // Assume reporter_id is the public key in base64 format
            use base64::Engine;
            use base64::engine::general_purpose::STANDARD;
            let pk_bytes = match STANDARD.decode(&self.reporter_id) {
                Ok(bytes) => bytes,
                Err(_) => return false,
            };
            if pk_bytes.len() != 32 {
                return false;
            }
            let mut pk_array = [0u8; 32];
            pk_array.copy_from_slice(&pk_bytes);
            let pk = match VerifyingKey::from_bytes(&pk_array) {
                Ok(p) => p,
                Err(_) => return false,
            };
            // Signature must be &[u8; 64]
            if self.signature.len() != 64 {
                return false;
            }
            let mut sig_bytes = [0u8; 64];
            sig_bytes.copy_from_slice(&self.signature);
            let sig = Signature::from_bytes(&sig_bytes);
            // Serialize the report data (excluding signature)
            let report_bytes = bincode::encode_to_vec(
                (
                    &self.id,
                    &self.subject_id,
                    &self.report_type,
                    &self.score,
                    &self.category,
                    &self.evidence_hash,
                    &self.stake_amount,
                    &self.timestamp,
                ),
                bincode::config::standard(),
            )
            .unwrap_or_default();
            if pk.verify_strict(&report_bytes, &sig).is_err() {
                return false;
            }
        }

        true
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, bincode::error::EncodeError> {
        bincode::encode_to_vec(self, bincode::config::standard())
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::error::DecodeError> {
        bincode::decode_from_slice(bytes, bincode::config::standard()).map(|r| r.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "serde")]
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

// Manual bincode impls for TrustReportType
impl bincode::Encode for TrustReportType {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode::error::EncodeError> {
        match self {
            TrustReportType::Positive => 0u8.encode(encoder),
            TrustReportType::Negative => 1u8.encode(encoder),
            TrustReportType::IdentityVerification => 2u8.encode(encoder),
            TrustReportType::CollaborationFeedback => 3u8.encode(encoder),
        }
    }
}

impl<Context> bincode::Decode<Context> for TrustReportType {
    fn decode<D: bincode::de::Decoder>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        match u8::decode(decoder)? {
            0 => Ok(TrustReportType::Positive),
            1 => Ok(TrustReportType::Negative),
            2 => Ok(TrustReportType::IdentityVerification),
            3 => Ok(TrustReportType::CollaborationFeedback),
            _ => Err(bincode::error::DecodeError::OtherString(
                "Invalid TrustReportType variant".to_string(),
            )),
        }
    }
}

impl TrustReportType {
    pub fn to_bytes(&self) -> Result<Vec<u8>, bincode::error::EncodeError> {
        bincode::encode_to_vec(self, bincode::config::standard())
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::error::DecodeError> {
        bincode::decode_from_slice(bytes, bincode::config::standard()).map(|r| r.0)
    }
}

/// Stake trust points transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "serde")]
pub struct StakeTransaction {
    pub id: String,
    pub participant_id: String,
    pub amount: u32,
    pub purpose: StakePurpose,
    pub timestamp: DateTimeWrapper,
    pub signature: Vec<u8>,
}

// Manual bincode impls for StakeTransaction
impl bincode::Encode for StakeTransaction {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode::error::EncodeError> {
        self.id.encode(encoder)?;
        self.participant_id.encode(encoder)?;
        self.amount.encode(encoder)?;
        self.purpose.encode(encoder)?;
        self.timestamp.encode(encoder)?;
        self.signature.encode(encoder)?;
        Ok(())
    }
}

impl<Context> bincode::Decode<Context> for StakeTransaction {
    fn decode<D: bincode::de::Decoder>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        Ok(StakeTransaction {
            id: bincode::Decode::decode(decoder)?,
            participant_id: bincode::Decode::decode(decoder)?,
            amount: bincode::Decode::decode(decoder)?,
            purpose: bincode::Decode::decode(decoder)?,
            timestamp: bincode::Decode::decode(decoder)?,
            signature: bincode::Decode::decode(decoder)?,
        })
    }
}

impl StakeTransaction {
    pub fn new(participant_id: String, amount: u32, purpose: StakePurpose) -> Self {
        Self {
            id: UuidWrapper::new(Uuid::new_v4()).to_string(),
            participant_id,
            amount,
            purpose,
            timestamp: DateTimeWrapper::new(chrono::Utc::now()),
            signature: vec![],
        }
    }

    pub fn verify(&self) -> bool {
        self.amount > 0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "serde")]
pub enum StakePurpose {
    /// Stake to become consensus validator
    ConsensusValidator,
    /// Stake to submit trust reports
    TrustReporting,
    /// Stake for identity verification
    IdentityVerification,
}

// Manual bincode impls for StakePurpose
impl bincode::Encode for StakePurpose {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode::error::EncodeError> {
        match self {
            StakePurpose::ConsensusValidator => 0u8.encode(encoder),
            StakePurpose::TrustReporting => 1u8.encode(encoder),
            StakePurpose::IdentityVerification => 2u8.encode(encoder),
        }
    }
}

impl<Context> bincode::Decode<Context> for StakePurpose {
    fn decode<D: bincode::de::Decoder>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        match u8::decode(decoder)? {
            0 => Ok(StakePurpose::ConsensusValidator),
            1 => Ok(StakePurpose::TrustReporting),
            2 => Ok(StakePurpose::IdentityVerification),
            _ => Err(bincode::error::DecodeError::OtherString(
                "Invalid StakePurpose variant".to_string(),
            )),
        }
    }
}

/// Unstake trust points transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "serde")]
pub struct UnstakeTransaction {
    pub id: String,
    pub participant_id: String,
    pub amount: u32,
    pub stake_id: String, // Reference to original stake
    pub timestamp: DateTimeWrapper,
    pub signature: Vec<u8>,
}

// Manual bincode impls for UnstakeTransaction
impl bincode::Encode for UnstakeTransaction {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode::error::EncodeError> {
        self.id.encode(encoder)?;
        self.participant_id.encode(encoder)?;
        self.amount.encode(encoder)?;
        self.stake_id.encode(encoder)?;
        self.timestamp.encode(encoder)?;
        self.signature.encode(encoder)?;
        Ok(())
    }
}

impl<Context> bincode::Decode<Context> for UnstakeTransaction {
    fn decode<D: bincode::de::Decoder>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        Ok(UnstakeTransaction {
            id: bincode::Decode::decode(decoder)?,
            participant_id: bincode::Decode::decode(decoder)?,
            amount: bincode::Decode::decode(decoder)?,
            stake_id: bincode::Decode::decode(decoder)?,
            timestamp: bincode::Decode::decode(decoder)?,
            signature: bincode::Decode::decode(decoder)?,
        })
    }
}

impl UnstakeTransaction {
    pub fn verify(&self) -> bool {
        self.amount > 0
    }
}

/// Transfer trust points between participants
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "serde")]
pub struct TransferTransaction {
    pub id: String,
    pub from_participant: String,
    pub to_participant: String,
    pub amount: u32,
    pub reason: String,
    pub timestamp: DateTimeWrapper,
    pub signature: Vec<u8>,
}

// Manual bincode impls for TransferTransaction
impl bincode::Encode for TransferTransaction {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode::error::EncodeError> {
        self.id.encode(encoder)?;
        self.from_participant.encode(encoder)?;
        self.to_participant.encode(encoder)?;
        self.amount.encode(encoder)?;
        self.reason.encode(encoder)?;
        self.timestamp.encode(encoder)?;
        self.signature.encode(encoder)?;
        Ok(())
    }
}

impl<Context> bincode::Decode<Context> for TransferTransaction {
    fn decode<D: bincode::de::Decoder>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        Ok(TransferTransaction {
            id: bincode::Decode::decode(decoder)?,
            from_participant: bincode::Decode::decode(decoder)?,
            to_participant: bincode::Decode::decode(decoder)?,
            amount: bincode::Decode::decode(decoder)?,
            reason: bincode::Decode::decode(decoder)?,
            timestamp: bincode::Decode::decode(decoder)?,
            signature: bincode::Decode::decode(decoder)?,
        })
    }
}

impl TransferTransaction {
    pub fn verify(&self) -> bool {
        self.amount > 0 && self.from_participant != self.to_participant
    }
}

/// Participant registration transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "serde")]
pub struct RegistrationTransaction {
    pub id: String,
    pub participant_id: String,
    pub public_key: Vec<u8>,
    pub initial_trust_points: u32,
    pub entity_type: String,
    pub timestamp: DateTimeWrapper,
    pub signature: Vec<u8>,
}

// Manual bincode impls for RegistrationTransaction
impl bincode::Encode for RegistrationTransaction {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode::error::EncodeError> {
        self.id.encode(encoder)?;
        self.participant_id.encode(encoder)?;
        self.public_key.encode(encoder)?;
        self.initial_trust_points.encode(encoder)?;
        self.entity_type.encode(encoder)?;
        self.timestamp.encode(encoder)?;
        self.signature.encode(encoder)?;
        Ok(())
    }
}

impl<Context> bincode::Decode<Context> for RegistrationTransaction {
    fn decode<D: bincode::de::Decoder>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        Ok(RegistrationTransaction {
            id: bincode::Decode::decode(decoder)?,
            participant_id: bincode::Decode::decode(decoder)?,
            public_key: bincode::Decode::decode(decoder)?,
            initial_trust_points: bincode::Decode::decode(decoder)?,
            entity_type: bincode::Decode::decode(decoder)?,
            timestamp: bincode::Decode::decode(decoder)?,
            signature: bincode::Decode::decode(decoder)?,
        })
    }
}

impl RegistrationTransaction {
    pub fn verify(&self) -> bool {
        !self.participant_id.is_empty() && !self.public_key.is_empty()
    }
}
