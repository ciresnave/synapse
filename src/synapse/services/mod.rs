// SPDX-License-Identifier: MIT OR Apache-2.0
// Synapse Services

#[cfg(feature = "database")]
pub mod discovery;
#[cfg(feature = "database")]
pub mod privacy_manager;
#[cfg(feature = "database")]
pub mod registry;
pub mod trust_source;

// Re-export key services
#[cfg(feature = "database")]
pub use discovery::DiscoveryService;
#[cfg(feature = "database")]
pub use privacy_manager::PrivacyManager;
#[cfg(feature = "database")]
pub use registry::ParticipantRegistry;
pub use trust_source::{NoTrustSource, TrustAnswer, TrustSource, TrustUnsupported};

// Type aliases for compatibility
#[cfg(feature = "database")]
pub type RegistryService = ParticipantRegistry;
