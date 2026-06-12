// Synapse storage layer

pub mod cache;
pub mod database;
pub mod migrations;

// Always re-export storage interfaces for monolithic build
pub use cache::Cache;
pub use database::Database;
