// Synapse API

pub mod participant_api;
pub mod trust_api;
pub mod discovery_api;
pub mod errors;

// Re-export API handlers
pub use participant_api::ParticipantAPI;
pub use trust_api::TrustAPI;
pub use discovery_api::DiscoveryAPI;

// Re-export error types for convenience
pub use errors::{ApiError, ApiResponse, ApiErrorResponse};
