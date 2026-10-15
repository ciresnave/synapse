// SPDX-License-Identifier: MIT OR Apache-2.0
// Synapse storage layer
//
// `cache` needs redis and `database`/`migrations` need sqlx, so each is behind its feature. The
// client crates build without both (`cargo tree -p synapsed-client` has no sqlx or redis).

#[cfg(feature = "cache")]
pub mod cache;
#[cfg(feature = "database")]
pub mod database;
#[cfg(feature = "database")]
pub mod migrations;

#[cfg(feature = "cache")]
pub use cache::Cache;
#[cfg(feature = "database")]
pub use database::Database;
