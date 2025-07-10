//! Concurrent Registry Access Test
//! This test validates that the registry service can handle concurrent access
//! from multiple participants without race conditions or data corruption.

use std::sync::Arc;
use anyhow::Result;
use tokio::sync::Barrier;
use synapse::{
    Config,
    identity::{Identity, KeyPair},
    services::registry::RegistryService,
    storage::database::Database,
};
use futures::future::join_all;

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_concurrent_registry_access() -> Result<()> {
    let config = Config::default();
    
    // Create a shared registry service
    let admin_keypair = KeyPair::generate();
    let admin_identity = Identity::from_keypair(&admin_keypair)?;
    let db = Database::new_in_memory()?;
    let registry_service = Arc::new(RegistryService::new(db.clone(), admin_identity.clone()));
    
    // Number of concurrent operations to run
    let concurrent_ops = 20;
    let barrier = Arc::new(Barrier::new(concurrent_ops));
    
    // Generate test identities
    let mut identities = Vec::new();
    for i in 0..concurrent_ops {
        let keypair = KeyPair::generate();
        let identity = Identity::from_keypair(&keypair)?;
        
        // Add some metadata to make each identity unique
        let mut identity_with_meta = identity.clone();
        identity_with_meta.set_metadata("name", &format!("Test Identity {}", i))?;
        identity_with_meta.set_metadata("test_id", &i.to_string())?;
        
        identities.push(identity_with_meta);
    }
    
    // Create a vector to hold task join handles
    let mut registration_tasks = Vec::new();
    
    // Spawn tasks to register identities concurrently
    for identity in &identities {
        let registry = registry_service.clone();
        let identity = identity.clone();
        let barrier = barrier.clone();
        
        let task = tokio::spawn(async move {
            // Wait for all tasks to be ready
            barrier.wait().await;
            
            // Register the identity
            match registry.register_participant(&identity) {
                Ok(()) => Ok(identity),
                Err(err) => Err(err),
            }
        });
        
        registration_tasks.push(task);
    }
    
    // Wait for all registration tasks to complete
    let registration_results = join_all(registration_tasks).await;
    
    // Verify all registrations were successful
    for result in registration_results {
        let identity_result = result?;
        assert!(identity_result.is_ok(), "Registration failed: {:?}", identity_result.err());
    }
    
    // Now test concurrent lookups
    let barrier = Arc::new(Barrier::new(concurrent_ops));
    let mut lookup_tasks = Vec::new();
    
    for identity in &identities {
        let registry = registry_service.clone();
        let id = identity.get_id().clone();
        let barrier = barrier.clone();
        
        let task = tokio::spawn(async move {
            // Wait for all tasks to be ready
            barrier.wait().await;
            
            // Lookup by ID
            registry.get_participant(&id)
        });
        
        lookup_tasks.push(task);
    }
    
    // Wait for all lookup tasks to complete
    let lookup_results = join_all(lookup_tasks).await;
    
    // Verify all lookups were successful
    for result in lookup_results {
        let lookup_result = result?;
        assert!(lookup_result.is_ok(), "Lookup failed: {:?}", lookup_result.err());
    }
    
    // Test concurrent updates to the same identities
    let barrier = Arc::new(Barrier::new(concurrent_ops));
    let mut update_tasks = Vec::new();
    
    for (i, identity) in identities.iter().enumerate() {
        let registry = registry_service.clone();
        let mut identity = identity.clone();
        let barrier = barrier.clone();
        
        // Make a unique update for each identity
        identity.set_metadata("updated", &format!("update-{}", i))?;
        
        let task = tokio::spawn(async move {
            // Wait for all tasks to be ready
            barrier.wait().await;
            
            // Update the identity
            registry.update_participant(&identity)
        });
        
        update_tasks.push(task);
    }
    
    // Wait for all update tasks to complete
    let update_results = join_all(update_tasks).await;
    
    // Verify all updates were successful
    for result in update_results {
        let update_result = result?;
        assert!(update_result.is_ok(), "Update failed: {:?}", update_result.err());
    }
    
    // Test concurrent search operations - these are read-heavy operations
    let barrier = Arc::new(Barrier::new(concurrent_ops));
    let mut search_tasks = Vec::new();
    
    for i in 0..concurrent_ops {
        let registry = registry_service.clone();
        let barrier = barrier.clone();
        
        // Different search queries to simulate real-world usage
        let search_type = i % 4;
        
        let task = tokio::spawn(async move {
            // Wait for all tasks to be ready
            barrier.wait().await;
            
            match search_type {
                0 => registry.search_by_metadata("name", "Test"), // Partial match
                1 => registry.list_all_participants(),
                2 => registry.search_by_metadata("updated", "update"), // Partial match
                3 => registry.get_participants_by_capability("unknown"), // Should return empty but valid
                _ => unreachable!(),
            }
        });
        
        search_tasks.push(task);
    }
    
    // Wait for all search tasks to complete
    let search_results = join_all(search_tasks).await;
    
    // Verify all searches completed without errors
    for result in search_results {
        let search_result = result?;
        assert!(search_result.is_ok(), "Search failed: {:?}", search_result.err());
    }
    
    // Final test: mixed operations (read/write) on the registry
    let barrier = Arc::new(Barrier::new(concurrent_ops * 2)); // Double the operations
    let mut mixed_tasks = Vec::new();
    
    // Generate more test identities for mixed operations
    let mut more_identities = Vec::new();
    for i in 0..concurrent_ops {
        let keypair = KeyPair::generate();
        let identity = Identity::from_keypair(&keypair)?;
        
        // Add some metadata to make each identity unique
        let mut identity_with_meta = identity.clone();
        identity_with_meta.set_metadata("name", &format!("Mixed Test Identity {}", i))?;
        identity_with_meta.set_metadata("test_id", &format!("mixed-{}", i))?;
        
        more_identities.push(identity_with_meta);
    }
    
    // Create tasks with a mix of operations
    for i in 0..concurrent_ops * 2 {
        let registry = registry_service.clone();
        let barrier = barrier.clone();
        
        let op_type = i % 5;
        
        // For update and lookup operations
        let existing_id = if !identities.is_empty() {
            identities[i % identities.len()].get_id().clone()
        } else {
            String::new() // Fallback
        };
        
        // For registration
        let new_identity = if i < more_identities.len() {
            more_identities[i].clone()
        } else {
            // Generate a new one if we've used all prepared identities
            let keypair = KeyPair::generate();
            let identity = Identity::from_keypair(&keypair)?;
            
            let mut identity_with_meta = identity;
            identity_with_meta.set_metadata("name", &format!("Dynamic Identity {}", i))?;
            identity_with_meta
        };
        
        let task = tokio::spawn(async move {
            // Wait for all tasks to be ready
            barrier.wait().await;
            
            match op_type {
                0 => registry.register_participant(&new_identity),
                1 => {
                    match registry.get_participant(&existing_id) {
                        Ok(Some(mut participant)) => {
                            // Make a modification
                            participant.set_metadata("modified_timestamp", 
                                                  &chrono::Utc::now().to_rfc3339())?;
                            registry.update_participant(&participant)
                        }
                        Ok(None) => Ok(()),
                        Err(e) => Err(e),
                    }
                }
                2 => registry.list_all_participants().map(|_| ()),
                3 => registry.search_by_metadata("name", "Test").map(|_| ()),
                4 => registry.get_participants_by_capability("test").map(|_| ()),
                _ => unreachable!(),
            }
        });
        
        mixed_tasks.push(task);
    }
    
    // Wait for all mixed operation tasks to complete
    let mixed_results = join_all(mixed_tasks).await;
    
    // Verify all mixed operations were successful
    for result in mixed_results {
        let op_result = result?;
        assert!(op_result.is_ok(), "Mixed operation failed: {:?}", op_result.err());
    }
    
    // Verify final registry state
    let all_participants = registry_service.list_all_participants()?;
    assert!(all_participants.len() > 0, "Registry should contain participants");
    
    Ok(())
}
