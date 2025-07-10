// Synapse data models

pub mod participant;
pub mod trust;

// Re-export common types
pub use participant::{
    ParticipantProfile, EntityType, IdentityContext, DiscoveryPermissions, 
    DiscoverabilityLevel, AvailabilityStatus, ContactPreferences,
};

pub use trust::{
    TrustRatings, EntityTrustRatings, NetworkTrustRating, TrustBalance,
    DirectTrustScore, TrustCategory, ParticipationMetrics,
};
