// SPDX-License-Identifier: MIT OR Apache-2.0
// Synapse Neural Communication Network
// Core module for federated identity and discovery

pub mod api;
pub mod models;
pub mod services;
pub mod storage;
pub mod telemetry;
pub mod transport;

pub mod auth;

// Re-export key types for convenience
pub use models::participant::{DiscoverabilityLevel, EntityType, ParticipantProfile};
