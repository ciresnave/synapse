#[tokio::test]
async fn test_trust_initialization_and_staking() -> anyhow::Result<()> {
    // Test trust system initialization and staking functionality
    use synapse::trust::{TrustManager, TrustConfig};
    use synapse::blockchain::{BlockchainManager, BlockchainConfig};
    use std::collections::HashMap;
    
    println!("Testing trust system initialization and staking...");
    
    // Initialize blockchain for trust system
    let blockchain_config = BlockchainConfig::default();
    let blockchain_manager = BlockchainManager::new(blockchain_config).await?;
    
    // Initialize trust manager
    let trust_config = TrustConfig::default();
    let mut trust_manager = TrustManager::new(trust_config, blockchain_manager).await?;
    
    // Test entity registration with staking
    let entity_id = "test_entity_1";
    let stake_amount = 100.0;
    
    let registration_result = trust_manager.register_entity_with_stake(
        entity_id,
        stake_amount,
        HashMap::new(),
    ).await;
    assert!(registration_result.is_ok(), "Entity registration with stake should succeed");
    
    // Test stake verification
    let stake_info = trust_manager.get_stake_info(entity_id).await?;
    assert_eq!(stake_info.amount, stake_amount, "Stake amount should match");
    assert!(stake_info.is_active, "Stake should be active");
    
    // Test trust score calculation with staking
    let trust_score = trust_manager.calculate_trust_score(entity_id).await?;
    assert!(trust_score > 0.0, "Trust score should be positive with stake");
    assert!(trust_score <= 1.0, "Trust score should not exceed 1.0");
    
    println!("✓ Trust initialization and staking test completed successfully");
    Ok(())
}

#[tokio::test]
async fn test_trust_report_submission() -> anyhow::Result<()> {
    // Test trust report submission and processing
    use synapse::trust::{TrustManager, TrustConfig, TrustReport, TrustInteraction};
    use synapse::blockchain::{BlockchainManager, BlockchainConfig};
    use std::collections::HashMap;
    
    println!("Testing trust report submission...");
    
    // Initialize trust system
    let blockchain_config = BlockchainConfig::default();
    let blockchain_manager = BlockchainManager::new(blockchain_config).await?;
    
    let trust_config = TrustConfig::default();
    let mut trust_manager = TrustManager::new(trust_config, blockchain_manager).await?;
    
    // Register test entities
    let reporter_id = "reporter_entity";
    let target_id = "target_entity";
    
    trust_manager.register_entity_with_stake(reporter_id, 50.0, HashMap::new()).await?;
    trust_manager.register_entity_with_stake(target_id, 50.0, HashMap::new()).await?;
    
    // Create trust report
    let trust_report = TrustReport {
        reporter_id: reporter_id.to_string(),
        target_id: target_id.to_string(),
        interaction_type: TrustInteraction::MessageDelivery,
        success: true,
        timestamp: chrono::Utc::now(),
        context: "successful message delivery".to_string(),
        evidence: Some("message_id_12345".to_string()),
    };
    
    // Submit trust report
    let submission_result = trust_manager.submit_trust_report(trust_report).await;
    assert!(submission_result.is_ok(), "Trust report submission should succeed");
    
    // Verify trust score update
    let updated_trust_score = trust_manager.calculate_trust_score(target_id).await?;
    assert!(updated_trust_score > 0.0, "Trust score should be positive after successful report");
    
    // Test negative trust report
    let negative_report = TrustReport {
        reporter_id: reporter_id.to_string(),
        target_id: target_id.to_string(),
        interaction_type: TrustInteraction::MessageDelivery,
        success: false,
        timestamp: chrono::Utc::now(),
        context: "failed message delivery".to_string(),
        evidence: Some("timeout_evidence".to_string()),
    };
    
    let negative_submission = trust_manager.submit_trust_report(negative_report).await;
    assert!(negative_submission.is_ok(), "Negative trust report should be accepted");
    
    // Verify trust score impact
    let post_negative_score = trust_manager.calculate_trust_score(target_id).await?;
    assert!(post_negative_score < updated_trust_score, "Trust score should decrease after negative report");
    
    println!("✓ Trust report submission test completed successfully");
    Ok(())
}

#[tokio::test]
async fn test_trust_decay() -> anyhow::Result<()> {
    // Test trust decay functionality
    use synapse::trust::{TrustManager, TrustConfig};
    use synapse::blockchain::{BlockchainManager, BlockchainConfig};
    use std::collections::HashMap;
    use tokio::time::{sleep, Duration};
    
    println!("Testing trust decay...");
    
    // Initialize trust system with fast decay for testing
    let blockchain_config = BlockchainConfig::default();
    let blockchain_manager = BlockchainManager::new(blockchain_config).await?;
    
    let mut trust_config = TrustConfig::default();
    trust_config.decay_rate = 0.1; // 10% decay rate for testing
    trust_config.decay_interval = Duration::from_secs(1); // 1 second decay interval
    
    let mut trust_manager = TrustManager::new(trust_config, blockchain_manager).await?;
    
    // Register test entity
    let entity_id = "decay_test_entity";
    trust_manager.register_entity_with_stake(entity_id, 100.0, HashMap::new()).await?;
    
    // Build up trust score with positive interactions
    let trust_report = synapse::trust::TrustReport {
        reporter_id: "reporter".to_string(),
        target_id: entity_id.to_string(),
        interaction_type: synapse::trust::TrustInteraction::MessageDelivery,
        success: true,
        timestamp: chrono::Utc::now(),
        context: "building trust".to_string(),
        evidence: None,
    };
    
    // Submit multiple positive reports
    for _ in 0..5 {
        trust_manager.submit_trust_report(trust_report.clone()).await?;
    }
    
    let initial_trust_score = trust_manager.calculate_trust_score(entity_id).await?;
    println!("Initial trust score: {}", initial_trust_score);
    
    // Start decay process
    trust_manager.start_decay_process().await?;
    
    // Wait for decay to occur
    sleep(Duration::from_secs(3)).await;
    
    // Check trust score after decay
    let decayed_trust_score = trust_manager.calculate_trust_score(entity_id).await?;
    println!("Decayed trust score: {}", decayed_trust_score);
    
    assert!(decayed_trust_score < initial_trust_score, 
            "Trust score should decrease due to decay");
    assert!(decayed_trust_score > 0.0, 
            "Trust score should not go below zero with sufficient stake");
    
    // Test decay limits with stake
    let stake_info = trust_manager.get_stake_info(entity_id).await?;
    assert!(stake_info.is_active, "Stake should still be active after decay");
    
    // Stop decay process
    trust_manager.stop_decay_process().await?;
    
    println!("✓ Trust decay test completed successfully");
    Ok(())
}
