// Synapse data models

pub mod participant;
pub mod trust;

// Re-export common types
pub use participant::{
    AvailabilityStatus, ContactPreferences, DiscoverabilityLevel, DiscoveryPermissions, EntityType,
    IdentityContext, ParticipantProfile,
};

pub use trust::{
    DirectTrustScore, EntityTrustRatings, NetworkTrustRating, ParticipationMetrics, TrustBalance,
    TrustCategory, TrustRatings,
};
