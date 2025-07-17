// Example implementation of auth-framework integration with Synapse

use std::sync::Arc;

use anyhow::Result;
// use tokio::sync::RwLock;

use crate::{
    Config,
    storage::Database,
    synapse::models::{
        participant::{ParticipantProfile, EntityType},
        trust::VerificationLevel,
    },
    synapse::services::{
        registry::ParticipantRegistry,
        trust_manager::TrustManager,
    },
};

#[cfg(feature = "enhanced-auth")]
use crate::synapse::auth::{
    SynapseAuth,
    SynapseAuthConfig,
    AuthMethod,
    trust_bridge::AuthTrustBridge,
    api::AuthApi,
    middleware::AuthMiddleware,
};

/// Example of setting up and using auth-framework with Synapse
#[cfg(feature = "enhanced-auth")]
pub async fn setup_enhanced_auth_example() -> Result<()> {
    // 1. Initialize Synapse components
    let config = Config::default();
    
    // Create database (simplified for example)
    let database = std::sync::Arc::new(Database::new("postgres://user:password@localhost/synapse_db").await?);
    
    // Create blockchain for trust management
    let blockchain = Arc::new(crate::synapse::blockchain::SynapseBlockchain::new().await?);
    
    // Create trust manager
    let trust_manager = Arc::new(TrustManager::new(
        database.clone(),
        blockchain.clone(),
    ).await?);
    
    // For this example, create a registry using the new() method
    let registry = Arc::new(ParticipantRegistry::new(
        database,
        trust_manager.clone(),
    ).await?);
    
    // 2. Create auth configuration
    let mut auth_config = SynapseAuthConfig::default();
    
    // Add OAuth providers
    auth_config.oauth_providers.push(crate::synapse::auth::OAuthProviderConfig {
        provider_name: "google".to_string(),
        client_id: "your-client-id".to_string(),
        client_secret: "your-client-secret".to_string(),
        redirect_uri: "https://your-app.com/oauth/callback".to_string(),
        scopes: vec!["email".to_string(), "profile".to_string()],
        verification_level: VerificationLevel::Enhanced,
    });
    
    // 3. Initialize auth module
    let auth = Arc::new(SynapseAuth::new(
        auth_config, 
        Arc::clone(&registry)
    ).await?);
    
    // 4. Set up auth-trust bridge
    let trust_bridge = AuthTrustBridge::new(
        Arc::clone(&registry),
        Arc::clone(&trust_manager)
    );
    
    // 5. Create auth API handler
    let auth_api = Arc::new(AuthApi::new(Arc::clone(&auth)));
    
    // 6. Create auth middleware
    let auth_middleware = Arc::new(AuthMiddleware::new(
        Arc::clone(&auth),
        Arc::clone(&registry)
    ));
    
    // 7. Example usage: Register a new participant
    let profile = ParticipantProfile {
        global_id: "new_user@example.com".to_string(),
        display_name: "New Test User".to_string(),
        identities: Vec::new(),
        entity_type: EntityType::Human,
        discovery_permissions: crate::synapse::models::participant::DiscoveryPermissions::default(),
        availability: crate::synapse::models::participant::AvailabilityStatus::default(),
        contact_preferences: crate::synapse::models::participant::ContactPreferences::default(),
        trust_ratings: crate::synapse::models::trust::TrustRatings::default(),
        relationships: Vec::new(),
        topic_subscriptions: Vec::new(),
        organizational_context: None,
        public_key: None,
        supported_protocols: Vec::new(),
        last_seen: chrono::Utc::now(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    
    let (registered_profile, token) = auth.register_participant(
        profile,
        Some("secure-password-123".to_string()),
        AuthMethod::Password
    ).await?;
    
    println!("Registered user: {}", registered_profile.global_id);
    println!("Token: {}", token.access_token);
    
    // 8. Example usage: Login
    let login_token = auth.login_with_password(
        "alice@example.com",
        "secure-password-123"
    ).await?;
    
    println!("Login successful, token: {}", login_token.access_token);
    
    // 9. Example usage: Protect an API endpoint
    fn example_protected_api(
        auth_middleware: &AuthMiddleware,
        token: &str,
        resource_id: &str,
    ) -> Result<String> {
        let auth_context = futures::executor::block_on(
            auth_middleware.authenticate(Some(token))
        );
        
        // Check permissions
        if !auth_middleware.check_permission(
            &auth_context,
            &["User".to_string()],
            crate::synapse::auth::middleware::SecurityClearance::Public
        ) {
            return Err(anyhow::anyhow!("Unauthorized"));
        }
        
        // Process request
        Ok(format!("Access granted to resource {}", resource_id))
    }
    
    let result = example_protected_api(
        &auth_middleware,
        &login_token.access_token,
        "resource-123"
    );
    
    println!("API result: {:?}", result);
    
    Ok(())
}

// Stub for non-auth-framework builds
#[cfg(not(feature = "enhanced-auth"))]
pub async fn setup_enhanced_auth_example() -> Result<()> {
    println!("Enhanced authentication not available without 'enhanced-auth' feature");
    Ok(())
}
