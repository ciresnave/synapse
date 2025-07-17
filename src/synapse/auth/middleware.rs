// Authentication middleware for Synapse
// Provides token validation and role-based authorization for API endpoints

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::synapse::auth::SynapseAuth;
use crate::synapse::models::participant::ParticipantProfile;
use crate::synapse::services::registry::ParticipantRegistry;

/// Security clearance levels
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SecurityClearance {
    Public,
    Internal,
    Confidential,
    Secret,
    TopSecret,
}

/// Context for authenticated requests
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// ID of the authenticated user
    pub user_id: String,
    
    /// Profile of the authenticated user (if found in registry)
    pub profile: Option<ParticipantProfile>,
    
    /// Whether the user has admin privileges
    pub is_admin: bool,
    
    /// Whether the user is authenticated
    pub is_authenticated: bool,
    
    /// Security clearance level
    pub clearance: SecurityClearance,
    
    /// Participant roles
    pub roles: Vec<String>,
}

impl Default for AuthContext {
    fn default() -> Self {
        Self {
            user_id: String::new(),
            profile: None,
            is_admin: false,
            is_authenticated: false,
            clearance: SecurityClearance::Public,
            roles: vec![],
        }
    }
}

/// Authentication middleware
pub struct AuthMiddleware {
    auth_service: Arc<SynapseAuth>,
    registry: Arc<ParticipantRegistry>,
}

impl AuthMiddleware {
    /// Create a new authentication middleware
    pub fn new(
        auth_service: Arc<SynapseAuth>,
        registry: Arc<ParticipantRegistry>,
    ) -> Self {
        Self {
            auth_service,
            registry,
        }
    }
    
    /// Authenticate a request using a token
    pub async fn authenticate(
        &self, 
        token: Option<&str>
    ) -> AuthContext {
        // Default to unauthenticated
        let mut context = AuthContext::default();
        
        // If no token provided, return unauthenticated context
        let token = match token {
            Some(token) => token,
            None => return context,
        };
        
        // Validate token
        let user_id = match self.auth_service.validate_token(token).await {
            Ok(user_id) => user_id,
            Err(_) => return context,
        };
        
        // User is authenticated at this point
        context.user_id = user_id.clone();
        context.is_authenticated = true;
        
        // Try to get profile from registry
        if let Ok(profile) = self.registry.get_participant(&user_id).await {
            if let Some(unwrapped_profile) = &profile {
                // Check for admin role - using string role since ParticipantRole enum doesn't exist
                context.is_admin = unwrapped_profile.organizational_context
                    .as_ref()
                    .map(|org| org.role == "Admin")
                    .unwrap_or(false);
                
                // Set security clearance - defaults to Public if not found
                context.clearance = SecurityClearance::Public;
                
                // Set roles based on organizational context role
                if let Some(org_context) = &unwrapped_profile.organizational_context {
                    context.roles = vec![org_context.role.clone()];
                }
            }
            
            // Set profile
            context.profile = profile.clone();
        }
        
        context
    }
    
    /// Check if a user has permission to access a resource
    pub fn check_permission(
        &self,
        context: &AuthContext,
        required_roles: &[String],
        required_clearance: SecurityClearance,
    ) -> bool {
        // Admins have access to everything
        if context.is_admin {
            return true;
        }
        
        // Must be authenticated
        if !context.is_authenticated {
            return false;
        }
        
        // Check security clearance
        if context.clearance < required_clearance {
            return false;
        }
        
        // If no specific roles required, authentication and clearance are enough
        if required_roles.is_empty() {
            return true;
        }
        
        // Check if user has any of the required roles
        for role in required_roles {
            if context.roles.contains(role) {
                return true;
            }
        }
        
        false
    }
}
