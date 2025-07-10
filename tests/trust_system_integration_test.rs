//! Trust System Integration Test
//! Tests the full trust system including:
//! - Trust point staking and unstaking
//! - Trust report submission and verification
//! - Trust calculation (entity-to-entity and network)
//! - Trust decay processing

use anyhow::Result;
use synapse::{
    synapse::{
        services::{trust_manager::TrustManager, registry::ParticipantRegistry},
        storage::{Database, Cache},
        blockchain::{SynapseBlockchain, BlockchainConfig},
        models::{trust::{TrustCategory, TrustBalance}, participant::ParticipantProfile},
    },
};
use std::sync::Arc;
use tokio;
use chrono::{Duration, Utc};

// Test database URL (using in-memory SQLite for tests)
const TEST_DB_URL: &str = "sqlite::memory:";

// Test helper to create trust manager with in-memory database
async fn setup_test_environment() -> Result<(Arc<TrustManager>, Arc<Database>, Arc<SynapseBlockchain>)> {
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
    "#;
    
    sqlx::query(schema).execute(&db.pool).await?;
    
    // Create blockchain
    let blockchain_config = BlockchainConfig::default();
    let blockchain = Arc::new(SynapseBlockchain::new(blockchain_config).await?);
    
    // Create trust manager
    let trust_manager = Arc::new(TrustManager::new(db.clone(), blockchain.clone()).await?);
    
    Ok((trust_manager, db, blockchain))
}

async fn create_test_participants(db: &Database) -> Result<Vec<String>> {
    // Create test participants with minimal data
    let participant_ids = vec![
        "test_participant_1".to_string(),
        "test_participant_2".to_string(),
        "test_participant_3".to_string(),
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
    }
    
    Ok(participant_ids)
}

#[tokio::test]
async fn test_trust_initialization_and_staking() -> Result<()> {
    let (trust_manager, db, blockchain) = setup_test_environment().await?;
    let participants = create_test_participants(&db).await?;
    
    // Initialize participants with trust balances
    for participant in &participants {
        trust_manager.initialize_participant(participant).await?;
    }
    
    // Verify trust balances were initialized
    for participant in &participants {
        let balance = trust_manager.get_trust_balance(participant).await?;
        assert!(balance.is_some(), "Trust balance should exist for {}", participant);
        
        let balance = balance.unwrap();
        assert_eq!(balance.total_points, 100, "Should have 100 initial trust points");
        assert_eq!(balance.available_points, 100, "All points should be available initially");
        assert_eq!(balance.staked_points, 0, "No points should be staked initially");
    }
    
    // Test staking trust points
    let stake_amount = 30;
    let stake_id = trust_manager
        .stake_trust_points(
            &participants[0],
            stake_amount,
            crate::synapse::blockchain::block::StakePurpose::TrustReporting,
        )
        .await?;
    
    // Verify balance after staking
    let balance = trust_manager.get_trust_balance(&participants[0]).await?.unwrap();
    assert_eq!(balance.total_points, 100, "Total points should remain the same");
    assert_eq!(balance.available_points, 70, "Available points should decrease by stake amount");
    assert_eq!(balance.staked_points, 30, "Staked points should increase by stake amount");
    
    // Test unstaking trust points
    let unstaked = trust_manager
        .unstake_trust_points(&participants[0], &stake_id)
        .await?;
    
    assert_eq!(unstaked, stake_amount, "Should unstake the full amount");
    
    // Verify balance after unstaking
    let balance = trust_manager.get_trust_balance(&participants[0]).await?.unwrap();
    assert_eq!(balance.total_points, 100, "Total points should remain the same");
    assert_eq!(balance.available_points, 100, "Available points should be restored");
    assert_eq!(balance.staked_points, 0, "Staked points should be zero again");
    
    Ok(())
}

#[tokio::test]
async fn test_trust_report_submission() -> Result<()> {
    let (trust_manager, db, blockchain) = setup_test_environment().await?;
    let participants = create_test_participants(&db).await?;
    
    // Initialize participants with trust balances
    for participant in &participants {
        trust_manager.initialize_participant(participant).await?;
    }
    
    // Submit a trust report
    let report_id = trust_manager
        .submit_trust_report(
            &participants[0],  // reporter
            &participants[1],  // subject
            75,                // score (positive)
            TrustCategory::Collaboration,
            10,                // stake amount
            Some("Test evidence data".to_string()),
        )
        .await?;
    
    // Verify balance after report submission
    let reporter_balance = trust_manager.get_trust_balance(&participants[0]).await?.unwrap();
    assert_eq!(reporter_balance.available_points, 90, "Reporter should have 10 fewer available points");
    assert_eq!(reporter_balance.staked_points, 10, "Reporter should have 10 points staked");
    
    // Get entity trust score (direct relationship)
    let trust_score = trust_manager
        .get_entity_trust_score(&participants[1], &participants[0])
        .await?;
    
    // Score should be positive
    assert!(trust_score > 50.0, "Trust score should be positive");
    
    // Negative report
    let negative_report_id = trust_manager
        .submit_trust_report(
            &participants[2],  // reporter
            &participants[1],  // subject
            -25,               // score (negative)
            TrustCategory::Communication,
            5,                 // stake amount
            None,
        )
        .await?;
    
    // Get network trust score (should account for both positive and negative reports)
    let network_score = trust_manager
        .get_network_trust_score(&participants[1])
        .await?;
    
    // Score should be influenced by both positive and negative reports
    assert!(network_score >= 0.0 && network_score <= 100.0, "Network score should be in valid range");
    
    // Calculate combined trust score
    let combined_score = trust_manager
        .get_trust_score(&participants[1], &participants[0])
        .await?;
    
    // Should be a weighted combination of entity and network trust
    assert!(combined_score >= 0.0 && combined_score <= 100.0, "Combined score should be in valid range");
    
    Ok(())
}

#[tokio::test]
async fn test_trust_decay() -> Result<()> {
    let (trust_manager, db, blockchain) = setup_test_environment().await?;
    let participants = create_test_participants(&db).await?;
    
    // Initialize participants
    for participant in &participants {
        trust_manager.initialize_participant(participant).await?;
    }
    
    // Manually set last activity to 60 days ago for one participant
    let old_date = Utc::now() - Duration::days(60);
    let mut balance = trust_manager.get_trust_balance(&participants[0]).await?.unwrap();
    balance.last_activity = old_date;
    db.upsert_trust_balance(&balance).await?;
    
    // Process decay
    let decayed_ids = trust_manager.process_decay().await?;
    
    // Verify decay was applied
    assert!(decayed_ids.contains(&participants[0]), "Participant should have decay applied");
    
    // Verify updated balance
    let updated_balance = trust_manager.get_trust_balance(&participants[0]).await?.unwrap();
    assert!(updated_balance.total_points < 100, "Points should have decayed from initial 100");
    
    // Other participants should not have decay
    let unchanged_balance = trust_manager.get_trust_balance(&participants[1]).await?.unwrap();
    assert_eq!(unchanged_balance.total_points, 100, "Recent activity should not have decay");
    
    Ok(())
}
