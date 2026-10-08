// SPDX-License-Identifier: MIT OR Apache-2.0
// Synapse Services

pub mod discovery;
pub mod privacy_manager;
pub mod registry;
pub mod trust_manager;
pub mod trust_source;

// Re-export key services
pub use discovery::DiscoveryService;
pub use privacy_manager::PrivacyManager;
pub use registry::ParticipantRegistry;
pub use trust_manager::TrustManager;
pub use trust_source::{NoTrustSource, TrustAnswer, TrustSource, TrustUnsupported};

// Type aliases for compatibility
pub type RegistryService = ParticipantRegistry;
