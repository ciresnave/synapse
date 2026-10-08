// SPDX-License-Identifier: MIT OR Apache-2.0
// Synapse data models

pub mod participant;

// Re-export common types
pub use participant::{
    AvailabilityStatus, ContactPreferences, DiscoverabilityLevel, DiscoveryPermissions, EntityType,
    IdentityContext, ParticipantProfile,
};
