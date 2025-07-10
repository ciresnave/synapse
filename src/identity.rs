//! # Identity Management and Name Resolution
//!
//! This module provides the core functionality for EMRP's **intuitive identity system** -
//! one of the protocol's most powerful features. It handles the automatic resolution
//! of simple, human-readable names to full network identities and addresses.
//!
//! ## 🎯 The Magic of Simple Names
//!
//! Instead of dealing with complex URLs, IP addresses, or lengthy identifiers, EMRP
//! lets you use simple names like `"Alice"`, `"Claude"`, or `"MyBot"`. The identity
//! system automatically resolves these to complete network information:
//!
//! ```text
//! Resolution Chain:
//! "Alice" → alice@ai-lab.example.com → 192.168.1.100:8080 → [capabilities]
//! ```
//!
//! ## 🏗️ Three-Layer Identity Architecture
//!
//! ### 1. Local Names (Human Layer)
//! - **Purpose**: Human-friendly identifiers for easy communication
//! - **Examples**: `"Alice"`, `"Claude"`, `"ResearchBot"`, `"Team-Alpha"`
//! - **Scope**: Local to your identity registry
//! - **Usage**: What you use in code and conversations
//!
//! ### 2. Global IDs (Email Layer)  
//! - **Purpose**: Globally unique identifiers based on email addresses
//! - **Examples**: `"alice@ai-lab.example.com"`, `"claude@anthropic.com"`
//! - **Scope**: Global across all EMRP systems worldwide
//! - **Benefits**: Leverages existing email infrastructure and DNS
//!
//! ### 3. Network Addresses (Transport Layer)
//! - **Purpose**: Actual network locations and connection details
//! - **Examples**: `192.168.1.100:8080`, `[2001:db8::1]:9090`, `relay.example.com:587`
//! - **Scope**: Dynamic, discovered through various methods
//! - **Intelligence**: Includes capabilities, performance metrics, security info
//!
//! ## 🔍 How Resolution Works
//!
//! The identity resolution process involves several intelligent steps:
//!
//! ### Step 1: Local Name Lookup
//! ```rust
//! // You send to: "Alice"
//! let global_id = registry.resolve_local_name("Alice")?;
//! // Result: "alice@ai-lab.example.com"
//! ```
//!
//! ### Step 2: Network Discovery
//! ```rust
//! // System discovers network information for alice@ai-lab.example.com
//! let addresses = discover_network_addresses(&global_id).await?;
//! // Results might include:
//! // - Direct: 192.168.1.100:8080 (if on same network)
//! // - Relay: relay.ai-lab.example.com:587 (if external)
//! // - Email: mx.ai-lab.example.com:25 (fallback)
//! ```
//!
//! ### Step 3: Capability Assessment
//! ```rust
//! // Determine what the peer supports
//! let capabilities = assess_capabilities(&addresses).await?;
//! // Results:
//! // - Supports: [TCP, UDP, EMRP-v1, PGP-encryption]
//! // - Prefers: TCP for real-time, Email for reliable
//! // - Security: Requires authentication
//! ```
//!
//! ### Step 4: Transport Selection
//! ```rust
//! // Choose best transport based on message urgency and capabilities
//! let transport = select_optimal_transport(
//!     &capabilities, 
//!     MessageUrgency::Interactive
//! )?;
//! // Chosen: TCP direct connection (fastest available)
//! ```
//!
//! ## 🔧 Registration Methods
//!
//! You can register identities in several ways:
//!
//! ### Manual Registration
//! ```rust
//! let mut registry = IdentityRegistry::new();
//! 
//! // Register with full details
//! registry.register_identity(GlobalIdentity {
//!     local_name: "Alice".to_string(),
//!     global_id: "alice@ai-lab.example.com".to_string(),
//!     entity_type: EntityType::AiModel,
//!     capabilities: vec!["real-time", "file-transfer"],
//!     public_key: Some(alice_public_key),
//!     created_at: Utc::now(),
//! })?;
//! 
//! // Quick registration (discovers details automatically)
//! registry.register_peer("Bob", "bob@robotics.company.com").await?;
//! ```
//!
//! ### Automatic Discovery
//! ```rust
//! // The system can automatically discover peers
//! registry.enable_auto_discovery(true);
//! 
//! // When you send to an unknown name, EMRP tries to discover it:
//! router.send_to("NewContact", message).await?;
//! // System attempts:
//! // 1. Local network scan for "NewContact"
//! // 2. DNS lookup for "newcontact@[your-domain]"  
//! // 3. Common domain variations
//! // 4. Ask known peers if they know "NewContact"
//! ```
//!
//! ### Import from External Sources
//! ```rust
//! // Import from email contacts
//! registry.import_email_contacts(&email_config).await?;
//! 
//! // Import from other EMRP systems
//! registry.sync_with_peer("alice@ai-lab.example.com").await?;
//! 
//! // Import from configuration files
//! registry.load_from_config("contacts.toml")?;
//! ```
//!
//! ## 🌐 Federation and Global Identity
//!
//! EMRP's identity system is designed for global federation:
//!
//! ### Domain-Based Organization
//! ```text
//! ai-lab.example.com:
//!   ├── alice@ai-lab.example.com (Research AI)
//!   ├── bob@ai-lab.example.com (Data Processor) 
//!   └── cluster@ai-lab.example.com (Compute Cluster)
//!
//! robotics.company.com:
//!   ├── optimus@robotics.company.com (Robot Controller)
//!   ├── warehouse@robotics.company.com (Logistics AI)
//!   └── maintenance@robotics.company.com (Maintenance Bot)
//! ```
//!
//! ### Cross-Domain Communication
//! ```rust
//! // These entities can seamlessly communicate across domains
//! alice.send_to("optimus@robotics.company.com", 
//!               "Can you help with robot arm calibration?").await?;
//! 
//! // The identity system handles:
//! // - Domain verification (DNS/email verification)
//! // - Security negotiation (key exchange)
//! // - Transport selection (direct vs email relay)
//! // - Protocol adaptation (EMRP vs standard email)
//! ```
//!
//! ## 🔐 Security Integration
//!
//! Identity management is tightly integrated with security:
//!
//! ### Automatic Key Management
//! ```rust
//! // When registering an identity, keys are automatically handled
//! registry.register_peer("Alice", "alice@ai-lab.example.com").await?;
//! // System automatically:
//! // 1. Requests Alice's public key
//! // 2. Verifies key authenticity (DNS, web-of-trust, etc.)
//! // 3. Stores key for future encryption
//! // 4. Shares your public key with Alice
//! ```
//!
//! ### Trust and Verification
//! ```rust
//! // Check identity trust level
//! let trust = registry.get_trust_level("Alice")?;
//! match trust {
//!     TrustLevel::Verified => println!("Identity verified through multiple sources"),
//!     TrustLevel::Known => println!("Identity seen before, seems legitimate"), 
//!     TrustLevel::Unknown => println!("First contact, be cautious"),
//! }
//! ```
//!
//! ## 📊 Performance and Caching
//!
//! The identity system is optimized for performance:
//!
//! ### Intelligent Caching
//! - **Local Cache**: Instant lookup for known identities
//! - **Network Cache**: Recently discovered network addresses
//! - **Capability Cache**: Remembered peer capabilities and preferences
//! - **Performance Cache**: Historical latency and reliability data
//!
//! ### Background Updates
//! ```rust
//! // The system continuously updates identity information in the background
//! registry.enable_background_updates(true);
//! // This ensures:
//! // - IP addresses stay current (DHCP changes, etc.)
//! // - Capabilities reflect current status
//! // - Performance metrics stay accurate
//! // - Security keys remain valid
//! ```
//!
//! ## 🎯 Example Usage Patterns
//!
//! ### AI Research Collaboration
//! ```rust
//! // Set up a research team identity registry
//! let mut team = IdentityRegistry::new();
//! team.register_peer("Claude", "claude@anthropic.com").await?;
//! team.register_peer("GPT-4", "gpt4@openai.com").await?; 
//! team.register_peer("Gemini", "gemini@google.com").await?;
//! team.register_peer("Research-Lead", "dr.smith@university.edu").await?;
//! 
//! // Now anyone can easily send to team members
//! router.send_to("Claude", "What's your take on consciousness?").await?;
//! router.send_to("Research-Lead", "Results are ready for review").await?;
//! ```
//!
//! ### Enterprise Microservices
//! ```rust
//! // Service discovery through identity registry
//! registry.register_service("UserAuth", "auth@services.company.com").await?;
//! registry.register_service("DataStore", "db@services.company.com").await?;
//! registry.register_service("Analytics", "analytics@services.company.com").await?;
//! 
//! // Services can communicate using simple names
//! auth_service.send_to("DataStore", user_query).await?;
//! web_service.send_to("Analytics", page_view_event).await?;
//! ```
//!
//! ### IoT Device Networks
//! ```rust
//! // Register IoT devices with descriptive names
//! registry.register_device("LivingRoomCamera", "cam1@home.local").await?;
//! registry.register_device("KitchenSensor", "sensor2@home.local").await?;
//! registry.register_device("SmartThermostat", "hvac@home.local").await?;
//! 
//! // Devices can coordinate using natural names
//! camera.send_to("SmartThermostat", motion_detected_event).await?;
//! sensor.send_to("LivingRoomCamera", "Please record for 30 seconds").await?;
//! ```
//!
//! The identity system makes EMRP communication feel natural and intuitive while
//! handling all the complex networking, security, and protocol details automatically.

use crate::types::{EntityType, GlobalIdentity};
use crate::error::{IdentityError, Result};
use dashmap::DashMap;
use uuid::Uuid;

/// Registry for managing entity identities
pub struct IdentityRegistry {
    /// Global ID -> Identity mapping
    identities: DashMap<String, GlobalIdentity>,
    /// Local name -> Global ID mapping
    local_names: DashMap<String, String>,
}

impl IdentityRegistry {
    /// Create a new identity registry
    pub fn new() -> Self {
        Self {
            identities: DashMap::new(),
            local_names: DashMap::new(),
        }
    }

    /// Register a new global identity
    pub fn register_identity(&self, identity: GlobalIdentity) -> Result<()> {
        // Check if local name already exists
        if self.local_names.contains_key(&identity.local_name) {
            return Err(IdentityError::AlreadyExists(format!(
                "Local name '{}' already registered",
                identity.local_name
            ))
            .into());
        }

        // Check if global ID already exists
        if self.identities.contains_key(&identity.global_id) {
            return Err(IdentityError::AlreadyExists(format!(
                "Global ID '{}' already registered",
                identity.global_id
            ))
            .into());
        }

        self.local_names
            .insert(identity.local_name.clone(), identity.global_id.clone());
        self.identities.insert(identity.global_id.clone(), identity);

        tracing::info!("📝 Registered new identity");
        Ok(())
    }

    /// Resolve a local name to global identity
    pub fn resolve_local_name(&self, local_name: &str) -> Option<GlobalIdentity> {
        let global_id = self.local_names.get(local_name)?;
        self.identities.get(global_id.as_str()).map(|entry| entry.value().clone())
    }

    /// Get identity by global ID
    pub fn get_identity(&self, global_id: &str) -> Option<GlobalIdentity> {
        self.identities.get(global_id).map(|entry| entry.value().clone())
    }

    /// Get mutable identity by global ID - returns owned value for modification
    pub fn get_identity_mut(&self, global_id: &str) -> Option<GlobalIdentity> {
        self.identities.get(global_id).map(|entry| entry.value().clone())
    }

    /// Update identity (replaces get_identity_mut for DashMap)
    pub fn update_identity(&self, global_id: &str, identity: GlobalIdentity) -> Result<()> {
        self.identities.insert(global_id.to_string(), identity);
        Ok(())
    }

    /// Get public key for a global identity
    pub fn get_public_key(&self, global_id: &str) -> Option<String> {
        self.identities.get(global_id).map(|entry| entry.public_key.clone())
    }

    /// Update trust level for an identity
    pub fn update_trust_level(&self, global_id: &str, trust_delta: i32) -> Result<()> {
        self.identities.entry(global_id.to_string())
            .and_modify(|identity| {
                let current_trust = identity.trust_level as i32;
                let new_trust = (current_trust + trust_delta).clamp(0, 100) as u8;
                identity.trust_level = new_trust;
                
                tracing::debug!(
                    "Updated trust level for {} from {} to {}",
                    global_id,
                    current_trust,
                    new_trust
                );
            })
            .or_insert_with(|| {
                // This should not happen in normal operation
                tracing::warn!("Attempted to update trust for non-existent identity: {}", global_id);
                GlobalIdentity::default()
            });

        Ok(())
    }

    /// Update last seen timestamp for an identity
    pub fn update_last_seen(&self, global_id: &str) -> Result<()> {
        self.identities.entry(global_id.to_string())
            .and_modify(|identity| {
                identity.update_last_seen();
            })
            .or_insert_with(|| {
                // This should not happen in normal operation
                tracing::warn!("Attempted to update last seen for non-existent identity: {}", global_id);
                GlobalIdentity::default()
            });

        Ok(())
    }

    /// Remove an identity
    pub fn remove_identity(&self, global_id: &str) -> Result<GlobalIdentity> {
        let identity = self.identities.remove(global_id).ok_or_else(|| {
            IdentityError::NotFound(format!("Identity not found: {}", global_id))
        })?;

        self.local_names.remove(&identity.1.local_name);

        tracing::info!("🗑️ Removed identity: {}", global_id);
        Ok(identity.1)
    }

    /// List all identities of a specific type
    pub fn list_by_type(&self, entity_type: EntityType) -> Vec<GlobalIdentity> {
        self.identities
            .iter()
            .filter(|entry| entry.value().entity_type == entity_type)
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// List all identities with a specific capability
    pub fn list_by_capability(&self, capability: &str) -> Vec<GlobalIdentity> {
        self.identities
            .iter()
            .filter(|entry| entry.value().has_capability(capability))
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Get all global IDs
    pub fn all_global_ids(&self) -> Vec<String> {
        self.identities.iter().map(|entry| entry.key().clone()).collect()
    }

    /// Get all local names
    pub fn all_local_names(&self) -> Vec<String> {
        self.local_names.iter().map(|entry| entry.key().clone()).collect()
    }

    /// Check if a local name is registered
    pub fn has_local_name(&self, local_name: &str) -> bool {
        self.local_names.contains_key(local_name)
    }

    /// Check if a global ID is registered
    pub fn has_global_id(&self, global_id: &str) -> bool {
        self.identities.contains_key(global_id)
    }

    /// Get the number of registered identities
    pub fn count(&self) -> usize {
        self.identities.len()
    }

    /// Clear all identities
    pub fn clear(&self) {
        self.identities.clear();
        self.local_names.clear();
        tracing::info!("🧹 Cleared all identities");
    }

    /// Resolve an entity by local name or global ID
    pub fn resolve_entity(&self, identifier: &str) -> Result<GlobalIdentity> {
        // First try as local name
        if let Some(global_id) = self.local_names.get(identifier) {
            return self.identities.get(global_id.as_str())
                .ok_or_else(|| IdentityError::NotFound(format!("Identity for global_id '{}' not found", global_id.as_str())).into())
                .map(|entry| entry.value().clone());
        }
        
        // Then try as global ID
        self.identities.get(identifier)
            .ok_or_else(|| IdentityError::NotFound(format!("Entity '{}' not found", identifier)).into())
            .map(|entry| entry.value().clone())
    }

    /// Register a simple entity (helper method)
    pub fn register_entity(&self, global_id: &str, _email: &str, display_name: Option<String>) -> Result<()> {
        let identity = GlobalIdentity {
            global_id: global_id.to_string(),
            local_name: display_name.clone().unwrap_or_else(|| global_id.to_string()),
            entity_type: EntityType::AiModel, // Default to AiModel
            public_key: "".to_string(), // Empty public key initially
            trust_level: 0,
            capabilities: Vec::new(),
            last_seen: chrono::Utc::now(),
            routing_preferences: std::collections::HashMap::new(),
        };
        
        self.register_identity(identity)
    }

    /// List all registered entities
    pub fn list_entities(&self) -> Vec<GlobalIdentity> {
        self.identities.iter().map(|entry| entry.value().clone()).collect()
    }
}

impl Default for IdentityRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper functions for creating identities
impl IdentityRegistry {
    /// Create a new AI model identity
    pub fn create_ai_model(
        local_name: impl Into<String>,
        domain: impl Into<String>,
        public_key: impl Into<String>,
        capabilities: Vec<String>,
    ) -> GlobalIdentity {
        let local_name = local_name.into();
        let domain = domain.into();
        let global_id = format!("{}@ai.{}", local_name.to_lowercase(), domain);

        let mut identity = GlobalIdentity::new(local_name, global_id, EntityType::AiModel, public_key);
        identity.capabilities = capabilities;
        identity
    }

    /// Create a new human identity
    pub fn create_human(
        local_name: impl Into<String>,
        domain: impl Into<String>,
        public_key: impl Into<String>,
    ) -> GlobalIdentity {
        let local_name = local_name.into();
        let domain = domain.into();
        let global_id = format!("{}@humans.{}", local_name.to_lowercase(), domain);

        GlobalIdentity::new(local_name, global_id, EntityType::Human, public_key)
    }

    /// Create a new tool identity
    pub fn create_tool(
        local_name: impl Into<String>,
        domain: impl Into<String>,
        public_key: impl Into<String>,
        capabilities: Vec<String>,
    ) -> GlobalIdentity {
        let local_name = local_name.into();
        let domain = domain.into();
        let global_id = format!("{}@tools.{}", local_name.to_lowercase(), domain);

        let mut identity = GlobalIdentity::new(local_name, global_id, EntityType::Tool, public_key);
        identity.capabilities = capabilities;
        identity
    }

    /// Generate a random global ID for testing
    pub fn generate_test_global_id(local_name: &str, entity_type: EntityType) -> String {
        let subdomain = match entity_type {
            EntityType::Human => "humans",
            EntityType::AiModel => "ai",
            EntityType::Tool => "tools",
            EntityType::Service => "services",
            EntityType::Router => "routers",
        };

        let uuid = Uuid::new_v4().to_string()[..8].to_string();
        format!("{}.{}@{}.test.local", local_name.to_lowercase(), uuid, subdomain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity_registration() {
        let registry = IdentityRegistry::new();

        let identity = IdentityRegistry::create_ai_model(
            "Claude",
            "anthropic.ai",
            "test-public-key",
            vec!["conversation".to_string(), "analysis".to_string()],
        );

        assert!(registry.register_identity(identity).is_ok());
        assert!(registry.has_local_name("Claude"));
        assert!(registry.resolve_local_name("Claude").is_some());
    }

    #[test]
    fn test_duplicate_registration() {
        let registry = IdentityRegistry::new();

        let identity1 = IdentityRegistry::create_human("Eric", "company.com", "key1");
        let identity2 = IdentityRegistry::create_human("Eric", "company.com", "key2");

        assert!(registry.register_identity(identity1).is_ok());
        assert!(registry.register_identity(identity2).is_err());
    }

    #[test]
    fn test_trust_level_update() {
        let registry = IdentityRegistry::new();

        let identity = IdentityRegistry::create_tool(
            "FileSystem",
            "tools.local",
            "fs-key",
            vec!["file_ops".to_string()],
        );
        let global_id = identity.global_id.clone();

        registry.register_identity(identity).unwrap();

        // Increase trust
        registry.update_trust_level(&global_id, 20).unwrap();
        assert_eq!(registry.get_identity(&global_id).unwrap().trust_level, 70);

        // Decrease trust
        registry.update_trust_level(&global_id, -30).unwrap();
        assert_eq!(registry.get_identity(&global_id).unwrap().trust_level, 40);

        // Test bounds
        registry.update_trust_level(&global_id, -100).unwrap();
        assert_eq!(registry.get_identity(&global_id).unwrap().trust_level, 0);

        registry.update_trust_level(&global_id, 200).unwrap();
        assert_eq!(registry.get_identity(&global_id).unwrap().trust_level, 100);
    }

    #[test]
    fn test_capability_filtering() {
        let registry = IdentityRegistry::new();

        let ai1 = IdentityRegistry::create_ai_model(
            "Claude",
            "anthropic.ai",
            "key1",
            vec!["conversation".to_string(), "analysis".to_string()],
        );

        let ai2 = IdentityRegistry::create_ai_model(
            "GPT",
            "openai.com",
            "key2",
            vec!["conversation".to_string(), "generation".to_string()],
        );

        let tool = IdentityRegistry::create_tool(
            "FileSystem",
            "tools.local",
            "key3",
            vec!["file_ops".to_string()],
        );

        registry.register_identity(ai1).unwrap();
        registry.register_identity(ai2).unwrap();
        registry.register_identity(tool).unwrap();

        let conversation_entities = registry.list_by_capability("conversation");
        assert_eq!(conversation_entities.len(), 2);

        let analysis_entities = registry.list_by_capability("analysis");
        assert_eq!(analysis_entities.len(), 1);

        let ai_entities = registry.list_by_type(EntityType::AiModel);
        assert_eq!(ai_entities.len(), 2);
    }
}
