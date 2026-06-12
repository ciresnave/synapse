//! Example: Enterprise Federated Authentication with Synapse
//!
//! This example demonstrates how to use Synapse's enterprise authentication
//! system for secure, federated authentication with OAuth and SAML providers.

use synapse::auth_enterprise::{
    EnterpriseAuthConfig, OAuthProviderConfig, SynapseEnterpriseAuthManager,
    SynapseEnterpriseAuthResult,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    #[cfg(not(target_arch = "wasm32"))]
    synapse::init_logging();

    run_enterprise_auth_demo().await
}

async fn run_enterprise_auth_demo() -> Result<(), Box<dyn std::error::Error>> {
    println!("🏢 Synapse Enterprise Federated Authentication Demo");
    println!("==================================================");

    // Configure enterprise authentication
    let auth_config = EnterpriseAuthConfig {
        oauth_providers: vec![OAuthProviderConfig {
            name: "GitHub Enterprise".to_string(),
            client_id: "your-github-enterprise-client-id".to_string(),
            client_secret: "your-github-enterprise-client-secret".to_string(),
            authorization_url: "https://github.com/login/oauth/authorize".to_string(),
            token_url: "https://github.com/login/oauth/access_token".to_string(),
            redirect_uri: "https://synapse.enterprise/auth/github/callback".to_string(),
            scopes: vec!["user:email".to_string(), "read:org".to_string()],
            enabled: true,
        }],
        ..EnterpriseAuthConfig::default()
    };

    // Initialize enterprise authentication manager
    let auth_manager = SynapseEnterpriseAuthManager::new(auth_config).await?;

    println!("✅ Enterprise authentication system initialized");
    println!("   🔗 OAuth providers: 1 configured");
    println!("   📋 Compliance: GDPR, HIPAA, SOX, ISO27001 enabled");
    println!();

    // Demonstrate enterprise SAML authentication
    match auth_manager
        .authenticate_saml_enterprise(
            "Corporate Active Directory",
            "mock_saml_assertion",
            "192.168.10.100",
        )
        .await?
    {
        SynapseEnterpriseAuthResult::Success {
            user_profile,
            session_token,
            expires_at,
        } => {
            println!("✅ Enterprise user authenticated via SAML SSO");
            println!(
                "   👤 User: {} ({})",
                user_profile.display_name, user_profile.global_id
            );
            println!(
                "   🔑 Session expires: {}",
                expires_at.format("%Y-%m-%d %H:%M UTC")
            );

            // Validate session
            if auth_manager
                .validate_session(&session_token)
                .await?
                .is_some()
            {
                println!("   ✅ Session validation successful");
            }
        }
        other => {
            println!("❌ Authentication failed: {:?}", other);
        }
    }

    // Show enterprise metrics
    let metrics = auth_manager.get_enterprise_metrics().await;
    println!("\n📊 Enterprise Metrics:");
    println!("   👥 Total Users: {}", metrics.total_users);
    println!("   🤖 AI Agents: {}", metrics.ai_agents);
    println!("   👤 Human Users: {}", metrics.human_users);
    println!("   🔗 Active Sessions: {}", metrics.active_sessions);

    println!("\n🎉 Enterprise Federated Authentication Demo Complete!");
    Ok(())
}
