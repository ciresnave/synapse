// SPDX-License-Identifier: MIT OR Apache-2.0
// Synapse API

pub mod discovery_api;
pub mod errors;
pub mod participant_api;

// Re-export API handlers
pub use discovery_api::DiscoveryAPI;
pub use participant_api::ParticipantAPI;

// Re-export error types for convenience
pub use errors::{ApiError, ApiErrorResponse, ApiResponse};
