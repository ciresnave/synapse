//! Example: Federated Authentication with Synapse
//! 
//! This example demonstrates how to use Synapse's integrated auth-framework
//! for secure, federated authentication with OAuth providers.

use std::time::Duration;

#[cfg(feature = "auth")]
use synapse::auth_integration::{
    SynapseAuthManager, SynapseAuthConfig, OAuthProviderConfig, SynapseAuthResult
};

use synapse::router_enhanced::EnhancedSynapseRouter;
use synapse::config::Config;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    #[cfg(not(target_arch = "wasm32"))]
    synapse::init_logging();
    
    #[cfg(not(feature = "auth"))]
    {
        println!("❌ This example requires the 'auth' feature to be enabled");
        println!("Run with: cargo run --features auth --example federated_auth_demo");
        return Ok(());
    }
    
    #[cfg(feature = "auth")]
    run_auth_demo().await
}

#[cfg(feature = "auth")]
async fn run_auth_demo() -> Result<(), Box<dyn std::error::Error>> {
    
    println!("🔐 Synapse Federated Authentication Demo");
    println!("========================================");
    
    // Configure authentication with multiple OAuth providers
    let auth_config = SynapseAuthConfig {
        oauth_providers: vec![
            OAuthProviderConfig {
                provider: "github".to_string(),
                client_id: std::env::var("GITHUB_CLIENT_ID")
                    .unwrap_or_else(|_| "your-github-client-id".to_string()),
                client_secret: std::env::var("GITHUB_CLIENT_SECRET")
                    .unwrap_or_else(|_| "your-github-client-secret".to_string()),
                redirect_uri: "http://localhost:8080/auth/callback".to_string(),
                scopes: vec!["user:email".to_string()],
                enabled: true,
            },
            OAuthProviderConfig {
                provider: "google".to_string(),
                client_id: std::env::var("GOOGLE_CLIENT_ID")
                    .unwrap_or_else(|_| "your-google-client-id".to_string()),
                client_secret: std::env::var("GOOGLE_CLIENT_SECRET")
                    .unwrap_or_else(|_| "your-google-client-secret".to_string()),
                redirect_uri: "http://localhost:8080/auth/callback".to_string(),
                scopes: vec!["profile".to_string(), "email".to_string()],
                enabled: true,
            },
        ],
        require_mfa: false,
        token_lifetime: Duration::from_secs(3600), // 1 hour
        refresh_token_lifetime: Duration::from_secs(86400 * 7), // 7 days
        enterprise_mode: false,
        rate_limit_requests: 100,
        rate_limit_window: Duration::from_secs(60),
    };
    
    // Create the authentication manager
    let auth_manager = SynapseAuthManager::new(auth_config).await?;
    println!("✅ Authentication manager initialized");
    
    // Demo 1: OAuth Authentication Flow
    println!("\n📱 Demo 1: OAuth Authentication Flow");
    println!("-----------------------------------");
    
    // In a real application, you would:
    // 1. Generate authorization URL
    // 2. Redirect user to OAuth provider
    // 3. Handle callback with authorization code
    // 4. Exchange code for token
    
    // Simulate OAuth callback with authorization code
    let mock_auth_code = "mock_authorization_code_from_github";
    
    match auth_manager.authenticate_oauth("github", mock_auth_code).await {
        Ok(SynapseAuthResult::Success(profile)) => {
            println!("✅ GitHub authentication successful!");
            println!("   User: {}", profile.display_name);
            println!("   Global ID: {}", profile.global_id);
            println!("   Email: {}", profile.email);
            println!("   Trust Level: {:?}", profile.trust_level);
            println!("   MFA Verified: {}", profile.mfa_verified);
            
            // Demo 2: Permission Checking
            println!("\n🔒 Demo 2: Permission Checking");
            println!("-----------------------------");
            
            let can_read = auth_manager
                .check_permission(&profile.global_id, "read", "messages")
                .await?;
            let can_write = auth_manager
                .check_permission(&profile.global_id, "write", "messages")
                .await?;
            let can_admin = auth_manager
                .check_permission(&profile.global_id, "admin", "system")
                .await?;
            
            println!("   Can read messages: {}", can_read);
            println!("   Can write messages: {}", can_write);
            println!("   Can admin system: {}", can_admin);
            
            // Demo 3: Secure Message Creation
            println!("\n📧 Demo 3: Secure Message Creation");
            println!("----------------------------------");
            
            let recipient_id = "bob@ai-lab.example.com";
            let message_content = "Hello Bob! This is a secure message from Alice.";
            
            match auth_manager
                .create_authenticated_message(&profile.global_id, recipient_id, message_content)
                .await
            {
                Ok(secure_message) => {
                    println!("✅ Secure message created:");
                    println!("   Message ID: {}", secure_message.id);
                    println!("   From: {}", secure_message.from);
                    println!("   To: {}", secure_message.to);
                    println!("   Security Level: {:?}", secure_message.security_level);
                    println!("   Encryption: {}", secure_message.encryption_method);
                    
                    // Demo 4: Message Sender Verification
                    println!("\n🔍 Demo 4: Message Sender Verification");
                    println!("-------------------------------------");
                    
                    let is_verified = auth_manager
                        .verify_message_sender(&secure_message)
                        .await?;
                    
                    println!("   Message sender verified: {}", is_verified);
                    
                    if is_verified {
                        println!("   ✅ Message can be trusted");
                    } else {
                        println!("   ⚠️  Message sender not verified");
                    }
                }
                Err(e) => {
                    println!("❌ Failed to create secure message: {}", e);
                }
            }
        }
        Ok(SynapseAuthResult::MfaRequired { challenge_id, challenge_type, .. }) => {
            println!("🔐 MFA Required:");
            println!("   Challenge ID: {}", challenge_id);
            println!("   Challenge Type: {}", challenge_type);
            println!("   (In a real app, you would prompt for MFA code)");
        }
        Ok(SynapseAuthResult::DeviceFlowRequired { device_code, user_code, verification_uri, .. }) => {
            println!("📱 Device Flow Required:");
            println!("   Device Code: {}", device_code);
            println!("   User Code: {}", user_code);
            println!("   Verification URI: {}", verification_uri);
            println!("   (User should visit the URI and enter the code)");
        }
        Ok(SynapseAuthResult::Failed { reason, can_retry }) => {
            println!("❌ Authentication failed: {}", reason);
            println!("   Can retry: {}", can_retry);
        }
        Err(e) => {
            println!("❌ Authentication error: {}", e);
        }
    }
    
    // Demo 5: Device Flow for CLI/IoT
    println!("\n🖥️  Demo 5: Device Flow for CLI/IoT");
    println!("----------------------------------");
    
    match auth_manager.start_device_flow("github").await {
        Ok(SynapseAuthResult::DeviceFlowRequired { user_code, verification_uri, .. }) => {
            println!("📱 Device Flow Started:");
            println!("   1. Visit: {}", verification_uri);
            println!("   2. Enter code: {}", user_code);
            println!("   3. Authorize the application");
            println!("   4. The app will poll for completion");
            println!("   ");
            println!("   This is perfect for:");
            println!("   ✅ CLI applications");
            println!("   ✅ IoT devices");
            println!("   ✅ Headless services");
            println!("   ✅ TV/streaming apps");
        }
        Ok(other_result) => {
            println!("Unexpected result: {:?}", other_result);
        }
        Err(e) => {
            println!("❌ Device flow error: {}", e);
        }
    }
    
    // Demo 6: Integration with Synapse Router
    println!("\n🌐 Demo 6: Integration with Synapse Router");
    println!("------------------------------------------");
    
    let config = Config::default();
    let router = EnhancedSynapseRouter::new(config, "auth-demo@synapse.local".to_string()).await?;
    
    println!("✅ Enhanced router created with authentication support");
    println!("   ");
    println!("   The router can now:");
    println!("   ✅ Authenticate users via OAuth");
    println!("   ✅ Verify message senders");
    println!("   ✅ Enforce permission-based access");
    println!("   ✅ Handle device flow for CLI apps");
    println!("   ✅ Support MFA for high-security scenarios");
    println!("   ✅ Integrate with enterprise identity systems");
    
    println!("\n🎉 Demo Complete!");
    println!("================");
    println!("Your auth-framework provides exactly what Synapse needs:");
    println!("✅ OAuth 2.0 & OpenID Connect");
    println!("✅ Device flow for CLI/IoT");
    println!("✅ Multi-factor authentication");
    println!("✅ Role-based access control");
    println!("✅ Enterprise integration");
    println!("✅ Secure token management");
    println!("✅ Comprehensive audit logging");
    
    println!("\nNext steps:");
    println!("1. Configure your OAuth providers");
    println!("2. Set up MFA for high-security users");
    println!("3. Define roles and permissions");
    println!("4. Deploy with Redis/PostgreSQL storage");
    println!("5. Enable enterprise features as needed");
    
    print_configuration_example();
    
    Ok(())
}

/// Helper function to demonstrate configuration
#[cfg(feature = "auth")]
fn print_configuration_example() {
    println!("\n📋 Configuration Example");
    println!("------------------------");
    println!("Set these environment variables:");
    println!("export GITHUB_CLIENT_ID=\"your-github-client-id\"");
    println!("export GITHUB_CLIENT_SECRET=\"your-github-client-secret\"");
    println!("export GOOGLE_CLIENT_ID=\"your-google-client-id\"");
    println!("export GOOGLE_CLIENT_SECRET=\"your-google-client-secret\"");
    println!("");
    println!("For production, also configure:");
    println!("export REDIS_URL=\"redis://localhost:6379\"");
    println!("export DATABASE_URL=\"postgresql://user:pass@localhost/synapse\"");
    println!("export JWT_SECRET=\"your-secure-jwt-secret\"");
}
