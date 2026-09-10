// SPDX-License-Identifier: MIT OR Apache-2.0
//! Authorization and authentication for EMRP email server

use crate::email_server::smtp_server::AuthHandler;
use crate::error::Result;
use bcrypt::{DEFAULT_COST, hash, verify};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Default authorization handler for EMRP email server
pub struct SynapseAuthHandler {
    /// User credentials
    users: Arc<Mutex<HashMap<String, UserAccount>>>,
    /// Email routing permissions
    routing_permissions: Arc<Mutex<RoutingPermissions>>,
    /// Server configuration
    config: AuthConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAccount {
    /// Username
    pub username: String,
    /// Password hash (in production, this should be properly hashed)
    pub password_hash: String,
    /// Email address
    pub email: String,
    /// Account permissions
    pub permissions: UserPermissions,
    /// Account status
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPermissions {
    /// Can send emails through this server
    pub can_send: bool,
    /// Can receive emails on this server
    pub can_receive: bool,
    /// Can relay emails for other users
    pub can_relay: bool,
    /// Is server administrator
    pub is_admin: bool,
}

#[derive(Debug, Clone)]
pub struct RoutingPermissions {
    /// Domains this server accepts mail for
    pub local_domains: Vec<String>,
    /// Remote domains allowed to relay through this server
    pub relay_domains: Vec<String>,
    /// IP addresses allowed to relay
    pub relay_ips: Vec<String>,
    /// Maximum message size per user
    pub max_message_size: HashMap<String, usize>,
}

#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Require authentication for sending
    pub require_auth_for_send: bool,
    /// Require authentication for receiving
    pub require_auth_for_receive: bool,
    /// Allow anonymous relay (dangerous!)
    pub allow_anonymous_relay: bool,
    /// Default user permissions
    pub default_permissions: UserPermissions,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            require_auth_for_send: true,
            require_auth_for_receive: false, // Local delivery doesn't require auth
            allow_anonymous_relay: false,
            default_permissions: UserPermissions {
                can_send: true,
                can_receive: true,
                can_relay: false,
                is_admin: false,
            },
        }
    }
}

impl Default for RoutingPermissions {
    fn default() -> Self {
        Self {
            local_domains: vec!["localhost".to_string(), "127.0.0.1".to_string()],
            relay_domains: Vec::new(),
            relay_ips: vec!["127.0.0.1".to_string(), "::1".to_string()],
            max_message_size: HashMap::new(),
        }
    }
}

impl Default for SynapseAuthHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl SynapseAuthHandler {
    /// Create a new auth handler with default configuration
    pub fn new() -> Self {
        let mut users = HashMap::new();

        // Add default admin user with properly hashed password
        let admin_password_hash = hash("admin", DEFAULT_COST).unwrap_or_else(|_| {
            eprintln!("WARNING: Failed to hash admin password, using fallback");
            "fallback_hash".to_string()
        });

        users.insert(
            "admin".to_string(),
            UserAccount {
                username: "admin".to_string(),
                password_hash: admin_password_hash,
                email: "admin@localhost".to_string(),
                permissions: UserPermissions {
                    can_send: true,
                    can_receive: true,
                    can_relay: true,
                    is_admin: true,
                },
                active: true,
            },
        );

        // Add default emrp user with properly hashed password
        let emrp_password_hash = hash("emrp123", DEFAULT_COST).unwrap_or_else(|_| {
            eprintln!("WARNING: Failed to hash emrp password, using fallback");
            "fallback_hash".to_string()
        });

        users.insert(
            "emrp".to_string(),
            UserAccount {
                username: "emrp".to_string(),
                password_hash: emrp_password_hash,
                email: "emrp@localhost".to_string(),
                permissions: UserPermissions {
                    can_send: true,
                    can_receive: true,
                    can_relay: false,
                    is_admin: false,
                },
                active: true,
            },
        );

        Self {
            users: Arc::new(Mutex::new(users)),
            routing_permissions: Arc::new(Mutex::new(RoutingPermissions::default())),
            config: AuthConfig::default(),
        }
    }

    /// Create auth handler with custom configuration
    pub fn with_config(config: AuthConfig) -> Self {
        let mut handler = Self::new();
        handler.config = config;
        handler
    }

    /// Add a new user account
    pub fn add_user(&self, user: UserAccount) -> Result<()> {
        let mut users = self.users.lock().unwrap();
        users.insert(user.username.clone(), user);
        Ok(())
    }

    /// Add a new user account with password hashing
    pub fn add_user_with_password(
        &self,
        username: &str,
        password: &str,
        email: &str,
        permissions: UserPermissions,
    ) -> Result<()> {
        let password_hash = hash(password, DEFAULT_COST).map_err(|e| {
            crate::error::SynapseError::AuthenticationError(format!(
                "Failed to hash password: {}",
                e
            ))
        })?;

        let user = UserAccount {
            username: username.to_string(),
            password_hash,
            email: email.to_string(),
            permissions,
            active: true,
        };

        self.add_user(user)
    }

    /// Remove a user account
    pub fn remove_user(&self, username: &str) -> Result<bool> {
        let mut users = self.users.lock().unwrap();
        Ok(users.remove(username).is_some())
    }

    /// Update routing permissions
    pub fn update_routing_permissions(&self, permissions: RoutingPermissions) -> Result<()> {
        let mut routing = self.routing_permissions.lock().unwrap();
        *routing = permissions;
        Ok(())
    }

    /// Add local domain
    pub fn add_local_domain(&self, domain: &str) -> Result<()> {
        let mut routing = self.routing_permissions.lock().unwrap();
        if !routing.local_domains.contains(&domain.to_string()) {
            routing.local_domains.push(domain.to_string());
        }
        Ok(())
    }

    /// Add relay domain
    pub fn add_relay_domain(&self, domain: &str) -> Result<()> {
        let mut routing = self.routing_permissions.lock().unwrap();
        if !routing.relay_domains.contains(&domain.to_string()) {
            routing.relay_domains.push(domain.to_string());
        }
        Ok(())
    }

    /// Check if email address is for a local domain
    fn is_local_domain(&self, email: &str) -> bool {
        let routing = self.routing_permissions.lock().unwrap();

        if let Some(domain) = email.split('@').nth(1) {
            routing.local_domains.iter().any(|local_domain| {
                domain == local_domain || domain.ends_with(&format!(".{local_domain}"))
            })
        } else {
            false
        }
    }

    /// Check if domain is allowed for relay
    fn is_relay_allowed(&self, email: &str) -> bool {
        let routing = self.routing_permissions.lock().unwrap();

        if let Some(domain) = email.split('@').nth(1) {
            routing.relay_domains.iter().any(|relay_domain| {
                domain == relay_domain || domain.ends_with(&format!(".{relay_domain}"))
            })
        } else {
            false
        }
    }

    /// Get user by username
    fn get_user(&self, username: &str) -> Option<UserAccount> {
        let users = self.users.lock().unwrap();
        users.get(username).cloned()
    }

    /// Get user by email address
    fn get_user_by_email(&self, email: &str) -> Option<UserAccount> {
        let users = self.users.lock().unwrap();
        users.values().find(|user| user.email == email).cloned()
    }
}

impl AuthHandler for SynapseAuthHandler {
    /// Authenticate user credentials
    fn authenticate(&self, username: &str, password: &str) -> Result<bool> {
        if let Some(user) = self.get_user(username)
            && user.active
        {
            // Use bcrypt to verify the password against the stored hash
            match verify(password, &user.password_hash) {
                Ok(is_valid) => return Ok(is_valid),
                Err(_) => {
                    // If bcrypt verification fails, log and deny access
                    eprintln!("Password verification failed for user: {}", username);
                    return Ok(false);
                }
            }
        }
        Ok(false)
    }

    /// Check if sender is authorized
    fn is_authorized_sender(&self, email: &str) -> Result<bool> {
        // If authentication is not required for sending, allow any local user
        if !self.config.require_auth_for_send {
            return Ok(self.is_local_domain(email));
        }

        // Check if user exists and has send permissions
        if let Some(user) = self.get_user_by_email(email) {
            Ok(user.active && user.permissions.can_send)
        } else {
            // Allow sending from local domains even without explicit user account
            Ok(self.is_local_domain(email))
        }
    }

    /// Check if recipient is authorized
    fn is_authorized_recipient(&self, email: &str) -> Result<bool> {
        // Always accept mail for local domains
        if self.is_local_domain(email) {
            return Ok(true);
        }

        // Check if relay is allowed for this domain
        if self.is_relay_allowed(email) {
            return Ok(true);
        }

        // Check if we have specific user permissions for relay
        if let Some(user) = self.get_user_by_email(email) {
            return Ok(user.active && user.permissions.can_relay);
        }

        // Default: reject external recipients unless relay is configured
        Ok(false)
    }
}

/// Create a pre-configured auth handler for testing
pub fn create_test_auth_handler() -> SynapseAuthHandler {
    let handler = SynapseAuthHandler::new();

    // Add test domains
    handler.add_local_domain("synapse.local").unwrap();
    handler.add_local_domain("test.com").unwrap();
    handler.add_relay_domain("example.com").unwrap();

    // Add test user with proper password hashing
    handler
        .add_user_with_password(
            "testuser",
            "testpass",
            "test@synapse.local",
            UserPermissions {
                can_send: true,
                can_receive: true,
                can_relay: true,
                is_admin: false,
            },
        )
        .unwrap();

    handler
}
