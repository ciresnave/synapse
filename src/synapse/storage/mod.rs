// Synapse storage layer

pub mod database;
pub mod cache;
pub mod migrations;

// Re-export storage interfaces with feature guards
#[cfg(feature = "database")]
pub use database::Database;
#[cfg(feature = "cache")]
pub use cache::Cache;
