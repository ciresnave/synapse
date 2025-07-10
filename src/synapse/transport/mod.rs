//! Synapse transport layer module
//!
//! This module contains the transport layer components for Synapse, including
//! circuit breaker, retry policies, and connection health monitoring.

pub mod error_recovery;

pub use error_recovery::{CircuitBreaker, RetryPolicy, ConnectionHealthMonitor, ConnectionStatus};
