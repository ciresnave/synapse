//! Configuration management for EMRP

use crate::types::{EmailConfig, SmtpConfig, ImapConfig};
use crate::error::{ConfigError, Result};
use serde::{Deserialize, Serialize};
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

/// Main configuration for EMRP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Entity configuration
    pub entity: EntityConfig,
    /// Email configuration
    pub email: EmailConfig,
    /// Router configuration
    pub router: RouterConfig,
    /// Security configuration
    pub security: SecurityConfig,
    /// Logging configuration
    pub logging: LoggingConfig,
}

/// Entity-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityConfig {
    /// Local entity name
    pub local_name: String,
    /// Entity type
    pub entity_type: String,
    /// Domain for generating global ID
    pub domain: String,
    /// Capabilities this entity provides
    pub capabilities: Vec<String>,
    /// Display name
    pub display_name: Option<String>,
}

/// Router configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterConfig {
    /// Maximum concurrent connections
    pub max_connections: usize,
    /// Message queue size
    pub queue_size: usize,
    /// Connection timeout in seconds
    pub connection_timeout: u64,
    /// Retry attempts for failed messages
    pub max_retries: u32,
    /// Enable real-time optimizations
    pub enable_realtime: bool,
    /// IMAP IDLE timeout in seconds
    pub idle_timeout: u64,
}

/// Security configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Path to private key file
    pub private_key_path: Option<String>,
    /// Path to public key file
    pub public_key_path: Option<String>,
    /// Auto-generate keys if not found
    pub auto_generate_keys: bool,
    /// Default security level
    pub default_security_level: String,
    /// Trusted domains
    pub trusted_domains: Vec<String>,
    /// Require encryption for these entity types
    pub require_encryption_for: Vec<String>,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level (trace, debug, info, warn, error)
    pub level: String,
    /// Log format (compact, pretty, json)
    pub format: String,
    /// Log to file
    pub file: Option<String>,
    /// Log message content (be careful with privacy)
    pub log_message_content: bool,
}

impl Config {
    /// Load configuration from a TOML file (not available on WASM)
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| ConfigError::FileNotFound(e.to_string()))?;

        toml::from_str(&content)
            .map_err(|e| ConfigError::InvalidFormat(e.to_string()).into())
    }

    /// Save configuration to a TOML file (not available on WASM)
    #[cfg(not(target_arch = "wasm32"))]
    pub fn to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| ConfigError::InvalidFormat(e.to_string()))?;

        std::fs::write(path.as_ref(), content)
            .map_err(|e| ConfigError::FileNotFound(e.to_string()).into())
    }

    /// Create a default configuration
    pub fn default_for_entity(local_name: impl Into<String>, entity_type: impl Into<String>) -> Self {
        let local_name = local_name.into();
        let entity_type = entity_type.into();
        
        Self {
            entity: EntityConfig {
                local_name: local_name.clone(),
                entity_type: entity_type.clone(),
                domain: "emrp.local".to_string(),
                capabilities: Self::default_capabilities_for_type(&entity_type),
                display_name: Some(format!("{} ({})", local_name, entity_type)),
            },
            email: EmailConfig {
                smtp: SmtpConfig {
                    host: "localhost".to_string(),
                    port: 587,
                    username: format!("{}@emrp.local", local_name.to_lowercase()),
                    password: "changeme".to_string(),
                    use_tls: true,
                    use_ssl: false,
                },
                imap: ImapConfig {
                    host: "localhost".to_string(),
                    port: 993,
                    username: format!("{}@emrp.local", local_name.to_lowercase()),
                    password: "changeme".to_string(),
                    use_ssl: true,
                },
            },
            router: RouterConfig {
                max_connections: 100,
                queue_size: 1000,
                connection_timeout: 30,
                max_retries: 3,
                enable_realtime: true,
                idle_timeout: 300,
            },
            security: SecurityConfig {
                private_key_path: Some(format!("{}_private.pem", local_name.to_lowercase())),
                public_key_path: Some(format!("{}_public.pem", local_name.to_lowercase())),
                auto_generate_keys: true,
                default_security_level: "private".to_string(),
                trusted_domains: vec!["emrp.local".to_string()],
                require_encryption_for: vec!["human".to_string()],
            },
            logging: LoggingConfig {
                level: "info".to_string(),
                format: "compact".to_string(),
                file: None,
                log_message_content: false,
            },
        }
    }

    /// Get default capabilities for an entity type
    fn default_capabilities_for_type(entity_type: &str) -> Vec<String> {
        match entity_type {
            "ai_model" => vec![
                "conversation".to_string(),
                "analysis".to_string(),
                "reasoning".to_string(),
            ],
            "human" => vec![
                "conversation".to_string(),
                "decision_making".to_string(),
                "creativity".to_string(),
            ],
            "tool" => vec![
                "task_execution".to_string(),
                "data_processing".to_string(),
            ],
            "service" => vec![
                "api_access".to_string(),
                "data_storage".to_string(),
            ],
            "router" => vec![
                "message_routing".to_string(),
                "identity_resolution".to_string(),
            ],
            _ => vec!["communication".to_string()],
        }
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        // Check entity configuration
        if self.entity.local_name.trim().is_empty() {
            return Err(ConfigError::ValidationFailed("Local name cannot be empty".to_string()).into());
        }

        if self.entity.domain.trim().is_empty() {
            return Err(ConfigError::ValidationFailed("Domain cannot be empty".to_string()).into());
        }

        // Check email configuration
        if self.email.smtp.host.trim().is_empty() {
            return Err(ConfigError::ValidationFailed("SMTP host cannot be empty".to_string()).into());
        }

        if self.email.imap.host.trim().is_empty() {
            return Err(ConfigError::ValidationFailed("IMAP host cannot be empty".to_string()).into());
        }

        // Check ports are valid
        if self.email.smtp.port == 0 {
            return Err(ConfigError::ValidationFailed("Invalid SMTP port".to_string()).into());
        }

        if self.email.imap.port == 0 {
            return Err(ConfigError::ValidationFailed("Invalid IMAP port".to_string()).into());
        }

        // Check security configuration
        if !["public", "private", "authenticated", "secure"].contains(&self.security.default_security_level.as_str()) {
            return Err(ConfigError::ValidationFailed("Invalid default security level".to_string()).into());
        }

        // Check logging configuration
        if !["trace", "debug", "info", "warn", "error"].contains(&self.logging.level.as_str()) {
            return Err(ConfigError::ValidationFailed("Invalid log level".to_string()).into());
        }

        if !["compact", "pretty", "json"].contains(&self.logging.format.as_str()) {
            return Err(ConfigError::ValidationFailed("Invalid log format".to_string()).into());
        }

        Ok(())
    }

    /// Create Gmail configuration
    pub fn gmail_config(local_name: impl Into<String>, entity_type: impl Into<String>, email: impl Into<String>, password: impl Into<String>) -> Self {
        let local_name = local_name.into();
        let entity_type = entity_type.into();
        let email = email.into();
        let password = password.into();

        let mut config = Self::default_for_entity(&local_name, &entity_type);
        
        config.email.smtp = SmtpConfig {
            host: "smtp.gmail.com".to_string(),
            port: 587,
            username: email.clone(),
            password: password.clone(),
            use_tls: true,
            use_ssl: false,
        };

        config.email.imap = ImapConfig {
            host: "imap.gmail.com".to_string(),
            port: 993,
            username: email,
            password,
            use_ssl: true,
        };

        config
    }

    /// Create Outlook configuration
    pub fn outlook_config(local_name: impl Into<String>, entity_type: impl Into<String>, email: impl Into<String>, password: impl Into<String>) -> Self {
        let local_name = local_name.into();
        let entity_type = entity_type.into();
        let email = email.into();
        let password = password.into();

        let mut config = Self::default_for_entity(&local_name, &entity_type);
        
        config.email.smtp = SmtpConfig {
            host: "smtp-mail.outlook.com".to_string(),
            port: 587,
            username: email.clone(),
            password: password.clone(),
            use_tls: true,
            use_ssl: false,
        };

        config.email.imap = ImapConfig {
            host: "outlook.office365.com".to_string(),
            port: 993,
            username: email,
            password,
            use_ssl: true,
        };

        config
    }

    /// Create a configuration suitable for testing
    pub fn for_testing() -> Self {
        Self::default_for_entity("test_entity", "test")
    }
}

/// Predefined configurations for common providers
pub struct ConfigTemplates;

impl ConfigTemplates {
    /// Get all available provider templates
    pub fn available_providers() -> Vec<&'static str> {
        vec!["gmail", "outlook", "yahoo", "custom"]
    }

    /// Create config for a specific provider
    pub fn for_provider(
        provider: &str,
        local_name: impl Into<String>,
        entity_type: impl Into<String>,
        email: impl Into<String>,
        password: impl Into<String>,
    ) -> Result<Config> {
        match provider.to_lowercase().as_str() {
            "gmail" => Ok(Config::gmail_config(local_name, entity_type, email, password)),
            "outlook" => Ok(Config::outlook_config(local_name, entity_type, email, password)),
            "yahoo" => {
                let mut config = Config::outlook_config(local_name, entity_type, email, password);
                config.email.smtp.host = "smtp.mail.yahoo.com".to_string();
                config.email.imap.host = "imap.mail.yahoo.com".to_string();
                Ok(config)
            }
            "custom" => Ok(Config::default_for_entity(local_name, entity_type)),
            _ => Err(ConfigError::ValidationFailed(format!("Unknown provider: {}", provider)).into()),
        }
    }

    /// Get provider-specific help text
    pub fn provider_help(provider: &str) -> Option<&'static str> {
        match provider.to_lowercase().as_str() {
            "gmail" => Some(
                "Gmail Configuration:\n\
                - Enable 2-factor authentication\n\
                - Generate an App Password\n\
                - Use App Password instead of regular password\n\
                - Enable IMAP in Gmail settings"
            ),
            "outlook" => Some(
                "Outlook Configuration:\n\
                - Works with Microsoft 365 accounts\n\
                - Use your regular email and password\n\
                - May require app-specific password for some accounts"
            ),
            "yahoo" => Some(
                "Yahoo Configuration:\n\
                - Generate an App Password in Yahoo Account Security\n\
                - Use App Password instead of regular password\n\
                - Enable IMAP access in Yahoo Mail settings"
            ),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_default_config_creation() {
        let config = Config::default_for_entity("Claude", "ai_model");
        assert_eq!(config.entity.local_name, "Claude");
        assert_eq!(config.entity.entity_type, "ai_model");
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_gmail_config() {
        let config = Config::gmail_config("Eric", "human", "eric@gmail.com", "app_password");
        assert_eq!(config.email.smtp.host, "smtp.gmail.com");
        assert_eq!(config.email.imap.host, "imap.gmail.com");
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_file_operations() {
        let config = Config::default_for_entity("Test", "tool");
        
        let temp_file = NamedTempFile::new().unwrap();
        
        // Save config
        assert!(config.to_file(temp_file.path()).is_ok());
        
        // Load config
        let loaded_config = Config::from_file(temp_file.path()).unwrap();
        assert_eq!(config.entity.local_name, loaded_config.entity.local_name);
    }

    #[test]
    fn test_config_validation() {
        let mut config = Config::default_for_entity("Test", "ai_model");
        
        // Valid config should pass
        assert!(config.validate().is_ok());
        
        // Invalid entity name should fail
        config.entity.local_name = "".to_string();
        assert!(config.validate().is_err());
        
        // Fix entity name
        config.entity.local_name = "Test".to_string();
        assert!(config.validate().is_ok());
        
        // Test config with invalid values should fail
        let mut invalid_config = config.clone();
        invalid_config.email.smtp.host = "".to_string(); // Empty host
        assert!(invalid_config.validate().is_err());
    }

    #[test]
    fn test_provider_templates() {
        let providers = ConfigTemplates::available_providers();
        assert!(providers.contains(&"gmail"));
        assert!(providers.contains(&"outlook"));

        let gmail_config = ConfigTemplates::for_provider("gmail", "Test", "ai_model", "test@gmail.com", "password");
        assert!(gmail_config.is_ok());
        
        let unknown_config = ConfigTemplates::for_provider("unknown", "Test", "ai_model", "test@example.com", "password");
        assert!(unknown_config.is_err());
    }
}
