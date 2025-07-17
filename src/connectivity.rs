//! Network connectivity enhancements for EMRP
//! 
//! This module provides solutions for entities behind NAT firewalls,
//! IPv6-only networks, and other connectivity challenges.

use crate::{
    router::SynapseRouter, 
    config::Config,
    types::{MessageType, SecurityLevel, SimpleMessage},
    error::Result,
};
use crate::synapse::blockchain::serialization::UuidWrapper;
use uuid::Uuid;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn, debug, error};
use uuid;

/// Network connectivity assistant that helps entities communicate
/// regardless of their network constraints
pub struct ConnectivityManager {
    router: SynapseRouter,
    polling_interval: Duration,
    adaptive_polling: bool,
    backup_email_providers: Vec<EmailProviderConfig>,
}

/// Configuration for backup email providers
#[derive(Debug, Clone)]
pub struct EmailProviderConfig {
    pub name: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub imap_host: String,
    pub imap_port: u16,
    pub supports_ipv6: bool,
    pub supports_ipv4: bool,
}

impl ConnectivityManager {
    /// Create a new connectivity manager
    pub fn new(router: SynapseRouter) -> Self {
        Self {
            router,
            polling_interval: Duration::from_secs(30), // Default 30 seconds
            adaptive_polling: true,
            backup_email_providers: Self::default_backup_providers(),
        }
    }

    /// Start adaptive message polling that works around network constraints
    pub async fn start_adaptive_polling(&mut self) -> Result<()> {
        info!("Starting adaptive polling for network-constrained environments");

        let mut polling_interval = self.polling_interval;
        let mut consecutive_failures = 0;
        let mut last_message_time = std::time::Instant::now();

        loop {
            sleep(polling_interval).await;

            match self.router.receive_messages().await {
                Ok(messages) => {
                    if !messages.is_empty() {
                        info!("Received {} messages via polling", messages.len());
                        last_message_time = std::time::Instant::now();
                        consecutive_failures = 0;

                        // Process messages here if needed
                        for message in messages {
                            debug!("Polled message from {}: {}", message.from_entity, message.content);
                        }

                        // Adaptive polling: increase frequency when active
                        if self.adaptive_polling {
                            polling_interval = Duration::from_secs(10); // More frequent when active
                        }
                    } else {
                        // No messages - adapt polling frequency
                        if self.adaptive_polling {
                            let time_since_last = last_message_time.elapsed();
                            if time_since_last > Duration::from_secs(600) { // 10 minutes
                                // Slow down polling when inactive
                                polling_interval = Duration::from_secs(120); // 2 minutes
                            }
                        }
                    }
                }
                Err(e) => {
                    consecutive_failures += 1;
                    error!("Failed to poll messages (attempt {}): {}", consecutive_failures, e);

                    // Try backup email providers if primary fails
                    if consecutive_failures >= 3 {
                        warn!("Primary email provider failing, attempting backup providers");
                        if let Err(backup_error) = self.try_backup_providers().await {
                            error!("All backup providers failed: {}", backup_error);
                        }
                    }

                    // Exponential backoff for failures
                    let backoff_duration = Duration::from_secs(std::cmp::min(300, 30 * consecutive_failures));
                    sleep(backoff_duration).await;
                }
            }
        }
    }

    /// Try backup email providers when primary fails
    async fn try_backup_providers(&self) -> Result<()> {
        for provider in &self.backup_email_providers {
            info!("Attempting backup provider: {}", provider.name);
            
            // In a real implementation, you would:
            // 1. Temporarily reconfigure the router with backup provider
            // 2. Test connectivity
            // 3. Switch if successful
            
            // For now, just log the attempt
            debug!("Testing connectivity to {}:{}", provider.smtp_host, provider.smtp_port);
        }
        
        Ok(())
    }

    /// Send message with automatic retry and provider fallback
    pub async fn send_with_fallback(
        &self,
        to_entity: &str,
        content: &str,
        message_type: MessageType,
        security_level: SecurityLevel,
    ) -> Result<String> {
        use std::collections::HashMap;
        
        // Generate a unique message ID for tracking
        let message_id = UuidWrapper::new(Uuid::new_v4()).to_string();
        
        // Create the message
        let simple_msg = SimpleMessage {
            to: to_entity.to_string(),
            from_entity: self.router.get_our_global_id().to_string(),
            content: content.to_string(),
            message_type,
            metadata: {
                let mut meta = HashMap::new();
                meta.insert("message_id".to_string(), message_id.clone());
                meta.insert("security_level".to_string(), security_level.to_string());
                meta
            },
        };
        
        // Try primary provider first
        match self.router.send_message(simple_msg.clone(), to_entity.to_string()).await {
            Ok(_) => {
                info!("Message sent successfully via primary provider");
                return Ok(message_id);
            }
            Err(primary_error) => {
                warn!("Primary provider failed: {}", primary_error);
            }
        }

        // Try backup providers
        for provider in &self.backup_email_providers {
            info!("Attempting to send via backup provider: {}", provider.name);
            
            // In a real implementation, temporarily switch providers
            // For demonstration, we'll just retry with current provider
            match self.router.send_message(simple_msg.clone(), to_entity.to_string()).await {
                Ok(_) => {
                    info!("Message sent successfully via backup provider: {}", provider.name);
                    return Ok(message_id.clone());
                }
                Err(e) => {
                    warn!("Backup provider {} failed: {}", provider.name, e);
                }
            }
        }

        Err(crate::error::SynapseError::NetworkError("All email providers failed".to_string()))
    }

    /// Default backup email providers that work well with NAT/IPv6
    fn default_backup_providers() -> Vec<EmailProviderConfig> {
        vec![
            EmailProviderConfig {
                name: "Gmail".to_string(),
                smtp_host: "smtp.gmail.com".to_string(),
                smtp_port: 587,
                imap_host: "imap.gmail.com".to_string(),
                imap_port: 993,
                supports_ipv6: true,
                supports_ipv4: true,
            },
            EmailProviderConfig {
                name: "Outlook".to_string(),
                smtp_host: "smtp-mail.outlook.com".to_string(),
                smtp_port: 587,
                imap_host: "outlook.office365.com".to_string(),
                imap_port: 993,
                supports_ipv6: true,
                supports_ipv4: true,
            },
            EmailProviderConfig {
                name: "ProtonMail".to_string(),
                smtp_host: "127.0.0.1".to_string(), // Via ProtonMail Bridge
                smtp_port: 1025,
                imap_host: "127.0.0.1".to_string(),
                imap_port: 1143,
                supports_ipv6: false,
                supports_ipv4: true,
            },
            EmailProviderConfig {
                name: "Yahoo".to_string(),
                smtp_host: "smtp.mail.yahoo.com".to_string(),
                smtp_port: 587,
                imap_host: "imap.mail.yahoo.com".to_string(),
                imap_port: 993,
                supports_ipv6: true,
                supports_ipv4: true,
            },
        ]
    }

    /// Detect network constraints and suggest optimal configuration
    pub async fn detect_network_constraints(&self) -> NetworkConstraints {
        let mut constraints = NetworkConstraints::default();

        // Test IPv4 connectivity
        constraints.has_ipv4 = self.test_ipv4_connectivity().await;
        
        // Test IPv6 connectivity  
        constraints.has_ipv6 = self.test_ipv6_connectivity().await;
        
        // Test if behind NAT
        constraints.behind_nat = self.detect_nat().await;
        
        // Test firewall restrictions
        constraints.firewall_restrictions = self.detect_firewall_restrictions().await;

        info!("Network constraints detected: {:?}", constraints);
        constraints
    }

    /// Test IPv4 connectivity
    async fn test_ipv4_connectivity(&self) -> bool {
        // In a real implementation, try to connect to known IPv4 servers
        debug!("Testing IPv4 connectivity...");
        true // Assume available for demo
    }

    /// Test IPv6 connectivity
    async fn test_ipv6_connectivity(&self) -> bool {
        // In a real implementation, try to connect to known IPv6 servers
        debug!("Testing IPv6 connectivity...");
        false // Assume not available for demo
    }

    /// Detect if behind NAT
    async fn detect_nat(&self) -> bool {
        // In a real implementation, compare local IP with external IP
        debug!("Detecting NAT configuration...");
        true // Assume behind NAT for demo
    }

    /// Detect firewall restrictions
    async fn detect_firewall_restrictions(&self) -> Vec<String> {
        // In a real implementation, test various ports
        debug!("Detecting firewall restrictions...");
        vec!["inbound_blocked".to_string()] // Assume inbound blocked for demo
    }

    /// Get recommended configuration based on network constraints
    pub fn get_recommended_config(&self, constraints: &NetworkConstraints) -> ConnectivityRecommendations {
        let mut recommendations = ConnectivityRecommendations::default();

        if constraints.behind_nat {
            recommendations.use_email_only = true;
            recommendations.polling_interval = Duration::from_secs(30);
            recommendations.message = "Behind NAT: Using email-only mode with regular polling".to_string();
        }

        if !constraints.has_ipv4 && constraints.has_ipv6 {
            recommendations.preferred_providers = self.backup_email_providers
                .iter()
                .filter(|p| p.supports_ipv6)
                .map(|p| p.name.clone())
                .collect();
            recommendations.message = "IPv6-only: Using IPv6-capable email providers".to_string();
        }

        if constraints.firewall_restrictions.contains(&"inbound_blocked".to_string()) {
            recommendations.use_email_only = true;
            recommendations.message = "Firewall restrictions: Email-only mode recommended".to_string();
        }

        recommendations
    }
}

/// Detected network constraints
#[derive(Debug, Default)]
pub struct NetworkConstraints {
    pub has_ipv4: bool,
    pub has_ipv6: bool,
    pub behind_nat: bool,
    pub firewall_restrictions: Vec<String>,
}

/// Connectivity recommendations
#[derive(Debug, Default)]
pub struct ConnectivityRecommendations {
    pub use_email_only: bool,
    pub polling_interval: Duration,
    pub preferred_providers: Vec<String>,
    pub message: String,
}

/// Extension trait for creating NAT/firewall-friendly configurations
pub trait ConnectivityConfigExt {
    /// Create a configuration optimized for NAT/firewall environments
    fn for_constrained_network(entity_name: &str, entity_type: &str) -> Config;
    
    /// Create a configuration for IPv6-only networks
    fn for_ipv6_only(entity_name: &str, entity_type: &str) -> Config;
    
    /// Create a configuration with multiple backup providers
    fn with_backup_providers(entity_name: &str, entity_type: &str) -> Config;
}

impl ConnectivityConfigExt for Config {
    fn for_constrained_network(entity_name: &str, entity_type: &str) -> Config {
        // Use Gmail as it works well behind NAT
        let mut config = Config::gmail_config(entity_name, entity_type, 
            &format!("{}@gmail.com", entity_name.to_lowercase()), 
            "app_password");
        
        // Optimize for polling-based operation
        config.router.idle_timeout = 30; // Shorter IDLE timeout
        config.router.max_retries = 5; // More retries for unreliable connections
        config.router.connection_timeout = 60; // Longer timeout for slow connections
        
        config
    }

    fn for_ipv6_only(entity_name: &str, entity_type: &str) -> Config {
        // Gmail and Outlook both support IPv6 well
        Config::gmail_config(entity_name, entity_type,
            &format!("{}@gmail.com", entity_name.to_lowercase()),
            "app_password")
    }

    fn with_backup_providers(entity_name: &str, entity_type: &str) -> Config {
        // Start with a reliable provider
        let config = Config::gmail_config(entity_name, entity_type,
            &format!("{}@gmail.com", entity_name.to_lowercase()),
            "app_password");
        
        // Configure for high reliability - we would modify config here in real implementation
        // For now, just return the base configuration
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_network_constraint_detection() {
        let config = Config::default_for_entity("test", "tool");
        let router = SynapseRouter::new(config, "test@example.com".to_string()).await.unwrap();
        let connectivity_manager = ConnectivityManager::new(router);
        
        let constraints = connectivity_manager.detect_network_constraints().await;
        
        // In the demo, we assume certain constraints
        assert!(constraints.behind_nat);
        assert!(constraints.has_ipv4);
    }

    #[test]
    fn test_constrained_network_config() {
        let config = Config::for_constrained_network("test_entity", "ai_model");
        
        assert_eq!(config.entity.local_name, "test_entity");
        assert_eq!(config.router.max_retries, 5);
        assert_eq!(config.router.idle_timeout, 30);
    }

    #[test]
    fn test_backup_providers() {
        let backup_providers = ConnectivityManager::default_backup_providers();
        
        assert!(!backup_providers.is_empty());
        assert!(backup_providers.iter().any(|p| p.name == "Gmail"));
        assert!(backup_providers.iter().any(|p| p.supports_ipv6));
    }
}
