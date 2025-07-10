//! Registry Integration Test
//! Tests the full participant registry functionality including:
//! - Participant registration and updates
//! - Organization-based participant queries
//! - Topic-based participant queries
//! - Alias resolution
//! - Advanced search functionality
//! - Privacy and discovery controls

use anyhow::Result;
use synapse::{
    synapse::{
        services::{
            registry::ParticipantRegistry,
            trust_manager::TrustManager,
        },
        storage::{Database, Cache},
        blockchain::SynapseBlockchain,
        models::{
            participant::{ParticipantProfile, DiscoverabilityLevel},
        },
    },
};
use std::sync::Arc;
use tokio;
use chrono::{Duration, Utc};

// Test database URL (using in-memory SQLite for tests)
const TEST_DB_URL: &str = "sqlite::memory:";

// Test helper to create registry with in-memory database
async fn setup_test_environment() -> Result<(Arc<ParticipantRegistry>, Arc<Database>)> {
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
        
        CREATE TABLE aliases (
            alias TEXT NOT NULL,
            participant_id TEXT NOT NULL,
            context TEXT NOT NULL,
            PRIMARY KEY (alias, context),
            FOREIGN KEY (participant_id) REFERENCES participants(global_id)
        );
        
        CREATE TABLE topic_subscriptions (
            topic_id TEXT NOT NULL,
            participant_id TEXT NOT NULL,
            subscription_level INTEGER NOT NULL,
            PRIMARY KEY (topic_id, participant_id),
            FOREIGN KEY (participant_id) REFERENCES participants(global_id)
        );
    "#;
    
    sqlx::query(schema).execute(&db.pool).await?;
    
    // Create cache
    let cache = Arc::new(Cache::new("memory://test").await?);
    
    // Create blockchain for trust manager
    let blockchain_config = Default::default();
    let blockchain = Arc::new(SynapseBlockchain::new(blockchain_config).await?);
    
    // Create trust manager
    let trust_manager = Arc::new(TrustManager::new(db.clone(), blockchain.clone()).await?);
    
    // Create registry
    let registry = Arc::new(ParticipantRegistry::new(
        db.clone(),
        cache.clone(),
        trust_manager.clone(),
    ).await?);
    
    Ok((registry, db))
}

// Helper to create test organization participants
async fn create_org_test_participants(registry: &ParticipantRegistry) -> Result<Vec<String>> {
    let orgs = vec![
        "org1.example.com",
        "org2.example.com",
    ];
    
    let mut participant_ids = Vec::new();
    
    // Create participants with organization contexts
    for (i, org) in orgs.iter().enumerate() {
        for j in 1..=3 {
            let global_id = format!("user{}@{}", j, org);
            let display_name = format!("User {} ({})", j, org);
            
            let profile = ParticipantProfile {
                global_id: global_id.clone(),
                display_name,
                entity_type: serde_json::json!({"type": "User"}),
                identities: vec![],
                discovery_permissions: serde_json::json!({
                    "level": "Listed",
                    "organizational_visibility": "Public",
                }),
                availability: serde_json::json!({
                    "status": "Available",
                }),
                contact_preferences: Default::default(),
                trust_ratings: Default::default(),
                relationships: vec![],
                topic_subscriptions: vec![],
                organizational_context: serde_json::json!({
                    "organization_id": org,
                    "department": format!("Department {}", i + 1),
                    "role": format!("Role {}", j),
                }),
                public_key: vec![0, 1, 2, 3],
                supported_protocols: vec!["http".to_string(), "email".to_string()],
                last_seen: Utc::now(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            
            registry.register_participant(profile).await?;
            participant_ids.push(global_id);
        }
    }
    
    Ok(participant_ids)
}

// Helper to create test topic participants
async fn create_topic_test_participants(registry: &ParticipantRegistry) -> Result<Vec<String>> {
    let topics = vec![
        "ai.research",
        "blockchain.development",
        "machine.learning",
    ];
    
    let mut participant_ids = Vec::new();
    
    // Create participants with topic subscriptions
    for i in 1..=6 {
        let global_id = format!("topic_user_{}@example.com", i);
        let display_name = format!("Topic User {}", i);
        
        // Assign topics (some participants have multiple topics)
        let mut assigned_topics = vec![];
        if i <= 3 {
            assigned_topics.push(topics[0].to_string());
        }
        if i >= 2 && i <= 4 {
            assigned_topics.push(topics[1].to_string());
        }
        if i >= 4 {
            assigned_topics.push(topics[2].to_string());
        }
        
        let profile = ParticipantProfile {
            global_id: global_id.clone(),
            display_name,
            entity_type: serde_json::json!({"type": "User"}),
            identities: vec![],
            discovery_permissions: Default::default(),
            availability: Default::default(),
            contact_preferences: Default::default(),
            trust_ratings: Default::default(),
            relationships: vec![],
            topic_subscriptions: assigned_topics.iter().map(|t| {
                serde_json::json!({
                    "topic_id": t,
                    "subscription_level": 2,
                })
            }).collect(),
            organizational_context: Default::default(),
            public_key: vec![0, 1, 2, 3],
            supported_protocols: vec!["http".to_string()],
            last_seen: Utc::now(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        
        registry.register_participant(profile).await?;
        participant_ids.push(global_id);
        
        // Also register in topic_subscriptions table for DB-level querying
        for topic in assigned_topics {
            let db = registry.get_database();
            sqlx::query(
                "INSERT INTO topic_subscriptions (topic_id, participant_id, subscription_level) VALUES ($1, $2, $3)"
            )
            .bind(topic)
            .bind(&global_id)
            .bind(2)
            .execute(&db.pool)
            .await?;
        }
    }
    
    Ok(participant_ids)
}

// Helper to create test aliases
async fn create_alias_test_participants(registry: &ParticipantRegistry) -> Result<Vec<(String, String)>> {
    let mut alias_mappings = Vec::new();
    
    // Create participants with aliases
    for i in 1..=3 {
        let global_id = format!("alias_user_{}@example.com", i);
        let display_name = format!("Alias User {}", i);
        let alias = format!("alias{}", i);
        
        let profile = ParticipantProfile {
            global_id: global_id.clone(),
            display_name,
            entity_type: serde_json::json!({"type": "User"}),
            identities: vec![
                serde_json::json!({
                    "type": "Alias",
                    "value": alias,
                    "context": "global",
                })
            ],
            discovery_permissions: Default::default(),
            availability: Default::default(),
            contact_preferences: Default::default(),
            trust_ratings: Default::default(),
            relationships: vec![],
            topic_subscriptions: vec![],
            organizational_context: Default::default(),
            public_key: vec![0, 1, 2, 3],
            supported_protocols: vec!["http".to_string()],
            last_seen: Utc::now(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        
        registry.register_participant(profile).await?;
        
        // Also register in aliases table for DB-level querying
        let db = registry.get_database();
        sqlx::query(
            "INSERT INTO aliases (alias, participant_id, context) VALUES ($1, $2, $3)"
        )
        .bind(&alias)
        .bind(&global_id)
        .bind("global")
        .execute(&db.pool)
        .await?;
        
        alias_mappings.push((alias, global_id));
    }
    
    Ok(alias_mappings)
}

#[tokio::test]
async fn test_registry_participant_crud() -> Result<()> {
    let (registry, _) = setup_test_environment().await?;
    
    // Create a test participant
    let global_id = "test_user@example.com";
    let profile = ParticipantProfile {
        global_id: global_id.to_string(),
        display_name: "Test User".to_string(),
        entity_type: serde_json::json!({"type": "User"}),
        identities: vec![],
        discovery_permissions: Default::default(),
        availability: Default::default(),
        contact_preferences: Default::default(),
        trust_ratings: Default::default(),
        relationships: vec![],
        topic_subscriptions: vec![],
        organizational_context: Default::default(),
        public_key: vec![0, 1, 2, 3],
        supported_protocols: vec!["http".to_string()],
        last_seen: Utc::now(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    
    // Register participant
    registry.register_participant(profile.clone()).await?;
    
    // Get the participant
    let retrieved = registry.get_participant(global_id).await?;
    assert!(retrieved.is_some(), "Should retrieve the registered participant");
    assert_eq!(retrieved.unwrap().display_name, "Test User", "Display name should match");
    
    // Update the participant
    let mut updated_profile = profile.clone();
    updated_profile.display_name = "Updated User".to_string();
    registry.update_participant(updated_profile).await?;
    
    // Get the updated participant
    let retrieved = registry.get_participant(global_id).await?;
    assert!(retrieved.is_some(), "Should retrieve the updated participant");
    assert_eq!(retrieved.unwrap().display_name, "Updated User", "Display name should be updated");
    
    Ok(())
}

#[tokio::test]
async fn test_get_participants_by_organization() -> Result<()> {
    let (registry, _) = setup_test_environment().await?;
    let _ = create_org_test_participants(&registry).await?;
    
    // Get participants from first organization
    let org1_participants = registry.get_participants_by_organization("org1.example.com").await?;
    
    // Should find 3 participants from org1
    assert_eq!(org1_participants.len(), 3, "Should find 3 participants in org1");
    
    // Check domain in participant IDs
    for participant in &org1_participants {
        assert!(
            participant.global_id.contains("org1.example.com"),
            "Participant should belong to org1.example.com"
        );
    }
    
    // Test with second organization
    let org2_participants = registry.get_participants_by_organization("org2.example.com").await?;
    assert_eq!(org2_participants.len(), 3, "Should find 3 participants in org2");
    
    // Test with non-existent organization
    let nonexistent = registry.get_participants_by_organization("nonexistent.com").await?;
    assert_eq!(nonexistent.len(), 0, "Should find 0 participants in nonexistent org");
    
    Ok(())
}

#[tokio::test]
async fn test_get_participants_by_topic() -> Result<()> {
    let (registry, _) = setup_test_environment().await?;
    let _ = create_topic_test_participants(&registry).await?;
    
    // Get participants subscribed to AI research
    let ai_participants = registry.get_participants_by_topic("ai.research").await?;
    assert_eq!(ai_participants.len(), 3, "Should find 3 participants in AI research");
    
    // Get participants subscribed to blockchain development
    let blockchain_participants = registry.get_participants_by_topic("blockchain.development").await?;
    assert_eq!(blockchain_participants.len(), 3, "Should find 3 participants in blockchain development");
    
    // Get participants subscribed to machine learning
    let ml_participants = registry.get_participants_by_topic("machine.learning").await?;
    assert_eq!(ml_participants.len(), 3, "Should find 3 participants in machine learning");
    
    // Test with non-existent topic
    let nonexistent = registry.get_participants_by_topic("nonexistent.topic").await?;
    assert_eq!(nonexistent.len(), 0, "Should find 0 participants in nonexistent topic");
    
    Ok(())
}

#[tokio::test]
async fn test_get_participant_by_alias() -> Result<()> {
    let (registry, _) = setup_test_environment().await?;
    let alias_mappings = create_alias_test_participants(&registry).await?;
    
    // Test each alias
    for (alias, global_id) in alias_mappings {
        let participant = registry.get_participant_by_alias(&alias).await?;
        assert!(participant.is_some(), "Should resolve participant from alias");
        assert_eq!(participant.unwrap().global_id, global_id, "Should resolve to correct participant");
    }
    
    // Test non-existent alias
    let nonexistent = registry.get_participant_by_alias("nonexistent_alias").await?;
    assert!(nonexistent.is_none(), "Should not find participant for nonexistent alias");
    
    Ok(())
}

#[tokio::test]
async fn test_participant_discoverability() -> Result<()> {
    let (registry, _) = setup_test_environment().await?;
    
    // Create participants with different discoverability levels
    let public_id = "public@example.com";
    let unlisted_id = "unlisted@example.com";
    let private_id = "private@example.com";
    
    // Public participant
    let public_profile = ParticipantProfile {
        global_id: public_id.to_string(),
        display_name: "Public User".to_string(),
        entity_type: serde_json::json!({"type": "User"}),
        identities: vec![],
        discovery_permissions: serde_json::json!({
            "level": "Listed",
            "organizational_visibility": "Public",
        }),
        availability: Default::default(),
        contact_preferences: Default::default(),
        trust_ratings: Default::default(),
        relationships: vec![],
        topic_subscriptions: vec![],
        organizational_context: serde_json::json!({
            "organization_id": "test.org",
        }),
        public_key: vec![0, 1, 2, 3],
        supported_protocols: vec!["http".to_string()],
        last_seen: Utc::now(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    
    // Unlisted participant
    let mut unlisted_profile = public_profile.clone();
    unlisted_profile.global_id = unlisted_id.to_string();
    unlisted_profile.display_name = "Unlisted User".to_string();
    unlisted_profile.discovery_permissions = serde_json::json!({
        "level": "Unlisted",
        "organizational_visibility": "OrgOnly",
    });
    
    // Private participant
    let mut private_profile = public_profile.clone();
    private_profile.global_id = private_id.to_string();
    private_profile.display_name = "Private User".to_string();
    private_profile.discovery_permissions = serde_json::json!({
        "level": "Private",
        "organizational_visibility": "None",
    });
    
    // Register all participants
    registry.register_participant(public_profile).await?;
    registry.register_participant(unlisted_profile).await?;
    registry.register_participant(private_profile).await?;
    
    // Test public participant discovery
    let discovery_result = registry.discover_participants(None, None, Some("test.org")).await?;
    assert!(discovery_result.contains(&public_id.to_string()), "Public user should be discoverable");
    assert!(!discovery_result.contains(&private_id.to_string()), "Private user should not be discoverable");
    
    // Unlisted participant should be accessible directly but not in discovery results
    let unlisted = registry.get_participant(unlisted_id).await?;
    assert!(unlisted.is_some(), "Unlisted participant should be accessible directly");
    assert!(!discovery_result.contains(&unlisted_id.to_string()), "Unlisted user should not appear in discovery");
    
    Ok(())
}
