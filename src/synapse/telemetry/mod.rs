//! Telemetry module for Synapse
//!
//! This module provides telemetry, monitoring, and error reporting capabilities
//! for the Synapse system.

pub mod error_reporting;

pub use error_reporting::{ErrorTelemetry, ErrorReport, ErrorSource, ErrorSeverity, ErrorTelemetryConfig};

// Re-export the error reporting macro for convenience
pub use crate::report_error;
