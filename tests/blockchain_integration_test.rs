//! Blockchain Integration Test
//! Tests the full blockchain system including:
//! - Block creation and validation
//! - Transaction processing
//! - Consensus mechanism
//! - Trust point staking and decay
//! - Validator rewards

use anyhow::Result;
use synapse::{
    synapse::{
        blockchain::{
            SynapseBlockchain, 
            BlockchainConfig, 
            block::{Block, Transaction, TransactionType, StakePurpose},
            consensus::ConsensusMethod,
            verification::ValidationResult,
        },
        storage::Database,
        models::{
            trust::TrustBalance,
            participant::ParticipantProfile,
        },
    },
};
use std::sync::Arc;
use tokio;
use chrono::{Duration, Utc};

// Test database URL (using in-memory SQLite for tests)
const TEST_DB_URL: &str = "sqlite::memory:";

// Test helper to create blockchain with in-memory database
async fn setup_test_environment() -> Result<(Arc<SynapseBlockchain>, Arc<Database>)> {
    // Create database
    let db = Arc::new(Database::new(TEST_DB_URL).await?);
    
    // Create schema (would be handled by migrations in real code)
    let schema = r#"
        CREATE TABLE participants (
            global_id TEXT PRIMARY KEY,
            display_name TEXT NOT NULL,
            entity_type TEXT NOT NULL,
            identities TEXT NOT NULL,
            discovery_permissions TEXT NOT NULL,
            availability TEXT NOT NULL,
            contact_preferences TEXT NOT NULL,
            trust_ratings TEXT NOT NULL,
            topic_subscriptions TEXT NOT NULL,
            organizational_context TEXT NOT NULL,
            public_key BLOB NOT NULL,
            supported_protocols TEXT NOT NULL,
            last_seen TIMESTAMP NOT NULL,
            created_at TIMESTAMP NOT NULL,
            updated_at TIMESTAMP NOT NULL
        );
        
        CREATE TABLE trust_balances (
            participant_id TEXT PRIMARY KEY,
            total_points INTEGER NOT NULL,
            available_points INTEGER NOT NULL,
            staked_points INTEGER NOT NULL,
            earned_lifetime INTEGER NOT NULL,
            last_activity TIMESTAMP NOT NULL,
            decay_rate REAL NOT NULL
        );
        
        CREATE TABLE blockchain_blocks (
            hash TEXT PRIMARY KEY,
            prev_hash TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            nonce INTEGER NOT NULL,
            transactions TEXT NOT NULL,
            validator TEXT NOT NULL,
            signature BLOB NOT NULL,
            height INTEGER NOT NULL
        );
    "#;
    
    sqlx::query(schema).execute(&db.pool).await?;
    
    // Create blockchain with custom test config
    let blockchain_config = BlockchainConfig {
        consensus_method: ConsensusMethod::ProofOfStake,
        block_time_seconds: 5, // Fast blocks for testing
        min_validator_stake: 10,
        trust_decay_rate: 0.1, // 10% decay for testing
        rewards_per_block: 5,
        .. BlockchainConfig::default()
    };
    
    let blockchain = Arc::new(SynapseBlockchain::new(blockchain_config).await?);
    
    Ok((blockchain, db))
}

async fn create_test_participants(db: &Database) -> Result<Vec<String>> {
    // Create test participants with minimal data
    let participant_ids = vec![
        "validator_1".to_string(),
        "validator_2".to_string(),
        "participant_1".to_string(),
        "participant_2".to_string(),
    ];
    
    for id in &participant_ids {
        let profile = ParticipantProfile {
            global_id: id.clone(),
            display_name: format!("Test User {}", id),
            entity_type: serde_json::json!({"type": "User"}),
            identities: vec![],
            discovery_permissions: Default::default(),
            availability: Default::default(),
            contact_preferences: Default::default(),
            trust_ratings: Default::default(),
            relationships: vec![],
            topic_subscriptions: vec![],
            organizational_context: serde_json::json!({}),
            public_key: vec![0, 1, 2, 3],
            supported_protocols: vec![],
            last_seen: Utc::now(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        
        db.upsert_participant(&profile).await?;
        
        // Initialize trust balance
        let balance = TrustBalance {
            participant_id: id.clone(),
            total_points: 100,
            available_points: 100,
            staked_points: 0,
            earned_lifetime: 0,
            last_activity: Utc::now(),
            decay_rate: 0.01,
        };
        
        db.upsert_trust_balance(&balance).await?;
    }
    
    Ok(participant_ids)
}

#[tokio::test]
async fn test_blockchain_initialization() -> Result<()> {
    let (blockchain, _) = setup_test_environment().await?;
    
    // Verify genesis block was created
    let genesis_block = blockchain.get_latest_block().await?;
    assert_eq!(genesis_block.height, 0, "Genesis block should have height 0");
    assert_eq!(genesis_block.prev_hash, "0".repeat(64), "Genesis block prev_hash should be all zeros");
    
    Ok(())
}

#[tokio::test]
async fn test_transaction_processing() -> Result<()> {
    let (blockchain, db) = setup_test_environment().await?;
    let participants = create_test_participants(&db).await?;
    
    // Create a stake transaction
    let tx = Transaction {
        id: "test_tx_1".to_string(),
        transaction_type: TransactionType::Stake {
            participant_id: participants[0].clone(),
            amount: 20,
            purpose: StakePurpose::Validation,
        },
        timestamp: Utc::now(),
        signature: vec![1, 2, 3, 4], // Mock signature
    };
    
    // Process the transaction
    let tx_result = blockchain.process_transaction(tx.clone()).await?;
    assert!(tx_result.success, "Transaction should be processed successfully");
    
    // Check participant's staked balance
    let balance = db.get_trust_balance(&participants[0]).await?.unwrap();
    assert_eq!(balance.available_points, 80, "Should have 20 points staked");
    assert_eq!(balance.staked_points, 20, "Should have 20 points staked");
    
    Ok(())
}

#[tokio::test]
async fn test_block_creation_and_validation() -> Result<()> {
    let (blockchain, db) = setup_test_environment().await?;
    let participants = create_test_participants(&db).await?;
    
    // Make the first participant a validator by staking
    let stake_tx = Transaction {
        id: "stake_tx_1".to_string(),
        transaction_type: TransactionType::Stake {
            participant_id: participants[0].clone(),
            amount: 50,
            purpose: StakePurpose::Validation,
        },
        timestamp: Utc::now(),
        signature: vec![1, 2, 3, 4], // Mock signature
    };
    
    blockchain.process_transaction(stake_tx).await?;
    
    // Create some test transactions
    let transactions = vec![
        Transaction {
            id: "test_tx_2".to_string(),
            transaction_type: TransactionType::TrustReport {
                reporter_id: participants[2].clone(),
                subject_id: participants[3].clone(),
                score: 75,
                category: "Collaboration".to_string(),
                evidence_hash: "abcdef123456".to_string(),
            },
            timestamp: Utc::now(),
            signature: vec![1, 2, 3, 4], // Mock signature
        },
        Transaction {
            id: "test_tx_3".to_string(),
            transaction_type: TransactionType::Stake {
                participant_id: participants[1].clone(),
                amount: 30,
                purpose: StakePurpose::TrustReporting,
            },
            timestamp: Utc::now(),
            signature: vec![5, 6, 7, 8], // Mock signature
        },
    ];
    
    // Process transactions
    for tx in &transactions {
        blockchain.process_transaction(tx.clone()).await?;
    }
    
    // Create a new block
    let new_block = blockchain.create_block(&participants[0]).await?;
    
    // Validate the block
    let validation_result = blockchain.validate_block(&new_block).await?;
    assert_eq!(validation_result, ValidationResult::Valid, "Block should be valid");
    
    // Submit the block to the chain
    let result = blockchain.submit_block(new_block).await?;
    assert!(result.success, "Block submission should succeed");
    
    // Check chain height
    let latest = blockchain.get_latest_block().await?;
    assert_eq!(latest.height, 1, "Chain height should be 1");
    
    Ok(())
}

#[tokio::test]
async fn test_trust_decay() -> Result<()> {
    let (blockchain, db) = setup_test_environment().await?;
    let participants = create_test_participants(&db).await?;
    
    // Initial balance
    let initial_balance = db.get_trust_balance(&participants[0]).await?.unwrap();
    assert_eq!(initial_balance.total_points, 100);
    
    // Process trust decay
    blockchain.process_trust_decay().await?;
    
    // Get updated balance
    let updated_balance = db.get_trust_balance(&participants[0]).await?.unwrap();
    
    // Should have decayed by config.trust_decay_rate (10%)
    assert!(updated_balance.total_points < 100, "Points should decay");
    assert!(updated_balance.total_points >= 90, "Should decay by approximately 10%");
    
    Ok(())
}

#[tokio::test]
async fn test_consensus_with_multiple_validators() -> Result<()> {
    let (blockchain, db) = setup_test_environment().await?;
    let participants = create_test_participants(&db).await?;
    
    // Set up two validators
    for i in 0..2 {
        let stake_tx = Transaction {
            id: format!("stake_tx_{}", i),
            transaction_type: TransactionType::Stake {
                participant_id: participants[i].clone(),
                amount: 50,
                purpose: StakePurpose::Validation,
            },
            timestamp: Utc::now(),
            signature: vec![1, 2, 3, 4], // Mock signature
        };
        
        blockchain.process_transaction(stake_tx).await?;
    }
    
    // Get validator set
    let validators = blockchain.get_active_validators().await?;
    assert_eq!(validators.len(), 2, "Should have 2 active validators");
    
    // Create blocks from both validators
    let block1 = blockchain.create_block(&participants[0]).await?;
    let block2 = blockchain.create_block(&participants[1]).await?;
    
    // Submit blocks - should handle the conflict based on consensus rules
    let result1 = blockchain.submit_block(block1.clone()).await?;
    assert!(result1.success, "First block submission should succeed");
    
    // Second block with same height should be rejected
    let result2 = blockchain.submit_block(block2.clone()).await?;
    assert!(!result2.success, "Second block with same height should be rejected");
    
    // Check that the chain has advanced
    let latest = blockchain.get_latest_block().await?;
    assert_eq!(latest.height, 1, "Chain height should be 1");
    assert_eq!(latest.validator, participants[0], "First validator's block should be accepted");
    
    Ok(())
}
