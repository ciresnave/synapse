// SPDX-License-Identifier: MIT OR Apache-2.0
// Synapse API

#[cfg(feature = "database")]
pub mod discovery_api;
pub mod errors;
#[cfg(feature = "database")]
pub mod participant_api;

// Re-export API handlers
#[cfg(feature = "database")]
pub use discovery_api::DiscoveryAPI;
#[cfg(feature = "database")]
pub use participant_api::ParticipantAPI;

// Re-export error types for convenience
pub use errors::{ApiError, ApiErrorResponse, ApiResponse};
