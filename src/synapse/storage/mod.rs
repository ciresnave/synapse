// Synapse storage layer

pub mod database;
pub mod cache;
pub mod migrations;

// Re-export storage interfaces
pub use database::Database;
pub use cache::Cache;
