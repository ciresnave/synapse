// Synapse Services

pub mod registry;
pub mod discovery;
pub mod trust_manager;
pub mod privacy_manager;

// Re-export key services
pub use registry::ParticipantRegistry;
pub use discovery::DiscoveryService;
pub use trust_manager::TrustManager;
pub use privacy_manager::PrivacyManager;

// Type aliases for compatibility
pub type RegistryService = ParticipantRegistry;
