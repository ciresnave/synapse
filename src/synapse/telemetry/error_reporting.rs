//! Error telemetry module for Synapse
//! 
//! This module provides facilities for reporting, tracking and analyzing errors
//! that occur in the Synapse system.

use std::{
    sync::{Arc, Mutex},
    collections::{HashMap, VecDeque},
    time::{Instant, Duration},
};
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

/// Error source categories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrorSource {
    Transport,
    Registry,
    Trust,
    Blockchain,
    Storage,
    Identity,
    WebRTC,
    Crypto,
    Config,
    External,
    Unknown,
}

/// Error severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum ErrorSeverity {
    Debug,
    Info,
    Warning,
    Error,
    Critical,
}

/// An error report containing detailed information about an error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorReport {
    /// Unique ID for this error
    pub id: String,
    
    /// When the error occurred
    pub timestamp: DateTime<Utc>,
    
    /// Source system or component
    pub source: ErrorSource,
    
    /// Error severity
    pub severity: ErrorSeverity,
    
    /// Error message
    pub message: String,
    
    /// Optional error code
    pub code: Option<String>,
    
    /// Optional context (e.g. function name, line number)
    pub context: HashMap<String, String>,
    
    /// Optional stack trace
    pub stack_trace: Option<String>,
}

/// Configuration for the error telemetry system
#[derive(Debug, Clone)]
pub struct ErrorTelemetryConfig {
    /// Maximum number of errors to keep in memory
    pub max_history: usize,
    
    /// Minimum severity level to report
    pub min_severity: ErrorSeverity,
    
    /// Whether to log errors to the console
    pub log_to_console: bool,
    
    /// URL for remote error reporting (if enabled)
    pub remote_endpoint: Option<String>,
    
    /// Maximum batch size for remote reporting
    pub batch_size: usize,
    
    /// Interval for sending batched errors
    pub batch_interval: Duration,
}

impl Default for ErrorTelemetryConfig {
    fn default() -> Self {
        Self {
            max_history: 1000,
            min_severity: ErrorSeverity::Warning,
            log_to_console: true,
            remote_endpoint: None,
            batch_size: 50,
            batch_interval: Duration::from_secs(60),
        }
    }
}

/// Error telemetry service for collecting and reporting errors
#[derive(Clone)]
pub struct ErrorTelemetry {
    config: ErrorTelemetryConfig,
    errors: Arc<Mutex<VecDeque<ErrorReport>>>,
    error_counts: Arc<Mutex<HashMap<ErrorSource, usize>>>,
    last_batch_time: Arc<Mutex<Instant>>,
    is_running: Arc<Mutex<bool>>,
}

impl ErrorTelemetry {
    /// Create a new error telemetry service with the given configuration
    pub fn new(config: ErrorTelemetryConfig) -> Self {
        let max_history = config.max_history; // Extract before moving config
        let telemetry = Self {
            config,
            errors: Arc::new(Mutex::new(VecDeque::with_capacity(max_history))),
            error_counts: Arc::new(Mutex::new(HashMap::new())),
            last_batch_time: Arc::new(Mutex::new(Instant::now())),
            is_running: Arc::new(Mutex::new(false)),
        };
        
        // Start the background reporting task if remote endpoint is configured
        if telemetry.config.remote_endpoint.is_some() {
            telemetry.start_reporter();
        }
        
        telemetry
    }
    
    /// Create a new error telemetry service with default configuration
    pub fn default() -> Self {
        Self::new(ErrorTelemetryConfig::default())
    }
    
    /// Report an error to the telemetry system
    pub fn report_error(&self, 
                        source: ErrorSource, 
                        severity: ErrorSeverity, 
                        message: &str, 
                        code: Option<&str>,
                        context: Option<HashMap<String, String>>,
                        stack_trace: Option<&str>) {
        
        // Check if we should report this error based on severity
        if severity < self.config.min_severity {
            return;
        }
        
        // Create the error report
        let error = ErrorReport {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            source,
            severity,
            message: message.to_string(),
            code: code.map(String::from),
            context: context.unwrap_or_default(),
            stack_trace: stack_trace.map(String::from),
        };
        
        // Log to console if enabled
        if self.config.log_to_console {
            match severity {
                ErrorSeverity::Debug => {
                    tracing::debug!(
                        source = ?source,
                        error_id = %error.id,
                        code = ?error.code,
                        "{}", message
                    );
                    if let Some(stack) = &error.stack_trace {
                        tracing::debug!("{}", stack);
                    }
                },
                ErrorSeverity::Info => {
                    tracing::info!(
                        source = ?source,
                        error_id = %error.id,
                        code = ?error.code,
                        "{}", message
                    );
                    if let Some(stack) = &error.stack_trace {
                        tracing::info!("{}", stack);
                    }
                },
                ErrorSeverity::Warning => {
                    tracing::warn!(
                        source = ?source,
                        error_id = %error.id,
                        code = ?error.code,
                        "{}", message
                    );
                    if let Some(stack) = &error.stack_trace {
                        tracing::warn!("{}", stack);
                    }
                },
                ErrorSeverity::Error => {
                    tracing::error!(
                        source = ?source,
                        error_id = %error.id,
                        code = ?error.code,
                        "{}", message
                    );
                    if let Some(stack) = &error.stack_trace {
                        tracing::error!("{}", stack);
                    }
                },
                ErrorSeverity::Critical => {
                    tracing::error!(
                        source = ?source,
                        error_id = %error.id,
                        code = ?error.code,
                        critical = true,
                        "{}", message
                    );
                    if let Some(stack) = &error.stack_trace {
                        tracing::error!("{}", stack);
                    }
                },
            }
        }
        
        // Update error counts
        {
            let mut counts = self.error_counts.lock().unwrap();
            *counts.entry(source).or_insert(0) += 1;
        }
        
        // Store error in history
        {
            let mut errors = self.errors.lock().unwrap();
            errors.push_back(error);
            
            // Trim history if needed
            while errors.len() > self.config.max_history {
                errors.pop_front();
            }
        }
    }
    
    /// Get recent errors
    pub fn get_recent_errors(&self, max_count: usize) -> Vec<ErrorReport> {
        let errors = self.errors.lock().unwrap();
        errors.iter()
            .rev() // Get most recent first
            .take(max_count)
            .cloned()
            .collect()
    }
    
    /// Get error counts by source
    pub fn get_error_counts(&self) -> HashMap<ErrorSource, usize> {
        self.error_counts.lock().unwrap().clone()
    }
    
    /// Get errors of a specific severity
    pub fn get_errors_by_severity(&self, severity: ErrorSeverity, max_count: usize) -> Vec<ErrorReport> {
        let errors = self.errors.lock().unwrap();
        errors.iter()
            .filter(|e| e.severity == severity)
            .rev() // Get most recent first
            .take(max_count)
            .cloned()
            .collect()
    }
    
    /// Get errors from a specific source
    pub fn get_errors_by_source(&self, source: ErrorSource, max_count: usize) -> Vec<ErrorReport> {
        let errors = self.errors.lock().unwrap();
        errors.iter()
            .filter(|e| e.source == source)
            .rev() // Get most recent first
            .take(max_count)
            .cloned()
            .collect()
    }
    
    /// Clear the error history
    pub fn clear_history(&self) {
        let mut errors = self.errors.lock().unwrap();
        errors.clear();
        
        let mut counts = self.error_counts.lock().unwrap();
        counts.clear();
    }
    
    /// Start the background error reporter task
    fn start_reporter(&self) {
        // Only start if we have a remote endpoint and not already running
        if self.config.remote_endpoint.is_none() || *self.is_running.lock().unwrap() {
            return;
        }
        
        let telemetry = self.clone();
        *self.is_running.lock().unwrap() = true;
        
        tokio::spawn(async move {
            while *telemetry.is_running.lock().unwrap() {
                // Sleep for the batch interval
                tokio::time::sleep(telemetry.config.batch_interval).await;
                
                // Check if we have any errors to report
                let batch = {
                    let mut errors = telemetry.errors.lock().unwrap();
                    let mut batch = Vec::new();
                    
                    // Take up to batch_size errors
                    for _ in 0..telemetry.config.batch_size {
                        if let Some(error) = errors.pop_front() {
                            batch.push(error);
                        } else {
                            break;
                        }
                    }
                    
                    batch
                };
                
                // If we have errors, send them to the remote endpoint
                if !batch.is_empty() && telemetry.config.remote_endpoint.is_some() {
                    if let Err(e) = telemetry.send_batch_to_remote(&batch).await {
                        tracing::error!("Failed to send error batch: {}", e);
                        
                        // Re-queue the errors
                        let mut errors = telemetry.errors.lock().unwrap();
                        for error in batch {
                            errors.push_back(error);
                        }
                    }
                }
                
                // Update last batch time
                *telemetry.last_batch_time.lock().unwrap() = Instant::now();
            }
        });
    }
    
    /// Send a batch of errors to the remote endpoint
    async fn send_batch_to_remote(&self, batch: &[ErrorReport]) -> Result<(), anyhow::Error> {
        if let Some(endpoint) = &self.config.remote_endpoint {
            // Serialize the batch to JSON
            let json = serde_json::to_string(batch)?;
            
            // Send to remote endpoint
            let client = reqwest::Client::new();
            let response = client.post(endpoint)
                .header("Content-Type", "application/json")
                .body(json)
                .send()
                .await?;
            
            // Check for success
            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await?;
                anyhow::bail!("Remote endpoint returned error: {}, {}", status, body);
            }
        }
        
        Ok(())
    }
    
    /// Stop the background reporter
    pub fn stop(&self) {
        *self.is_running.lock().unwrap() = false;
    }
}

// Implement Drop to ensure the background task is stopped
impl Drop for ErrorTelemetry {
    fn drop(&mut self) {
        self.stop();
    }
}

// Helper macros for reporting errors
#[macro_export]
macro_rules! report_error {
    ($telemetry:expr, $source:expr, $severity:expr, $message:expr) => {
        $telemetry.report_error($source, $severity, $message, None, None, None)
    };
    
    ($telemetry:expr, $source:expr, $severity:expr, $message:expr, $code:expr) => {
        $telemetry.report_error($source, $severity, $message, Some($code), None, None)
    };
    
    ($telemetry:expr, $source:expr, $severity:expr, $message:expr, $code:expr, $context:expr) => {
        $telemetry.report_error($source, $severity, $message, Some($code), Some($context), None)
    };
    
    ($telemetry:expr, $source:expr, $severity:expr, $message:expr, $code:expr, $context:expr, $stack:expr) => {
        $telemetry.report_error($source, $severity, $message, Some($code), Some($context), Some($stack))
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_error_telemetry_basic() {
        let telemetry = ErrorTelemetry::default();
        
        // Report some errors
        telemetry.report_error(
            ErrorSource::Transport, 
            ErrorSeverity::Error, 
            "Connection failed", 
            Some("CONN-001"),
            None,
            None
        );
        
        telemetry.report_error(
            ErrorSource::Registry, 
            ErrorSeverity::Warning, 
            "Participant not found", 
            Some("REG-404"),
            None,
            None
        );
        
        // Check error counts
        let counts = telemetry.get_error_counts();
        assert_eq!(counts.get(&ErrorSource::Transport), Some(&1));
        assert_eq!(counts.get(&ErrorSource::Registry), Some(&1));
        
        // Check recent errors
        let recent = telemetry.get_recent_errors(10);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].source, ErrorSource::Registry);
        assert_eq!(recent[0].severity, ErrorSeverity::Warning);
        
        // Check filtering by severity
        let errors = telemetry.get_errors_by_severity(ErrorSeverity::Error, 10);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].source, ErrorSource::Transport);
        
        // Check filtering by source
        let errors = telemetry.get_errors_by_source(ErrorSource::Transport, 10);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].severity, ErrorSeverity::Error);
    }
    
    #[test]
    fn test_error_telemetry_history_limit() {
        let config = ErrorTelemetryConfig {
            max_history: 5,
            ..Default::default()
        };
        
        let telemetry = ErrorTelemetry::new(config);
        
        // Report more errors than the history limit
        for i in 0..10 {
            telemetry.report_error(
                ErrorSource::Transport, 
                ErrorSeverity::Error, 
                &format!("Error {}", i), 
                None,
                None,
                None
            );
        }
        
        // Check that we only have the last 5 errors
        let recent = telemetry.get_recent_errors(10);
        assert_eq!(recent.len(), 5);
        
        // The most recent error should be "Error 9"
        assert!(recent[0].message.contains("Error 9"));
        
        // The oldest error should be "Error 5"
        assert!(recent[4].message.contains("Error 5"));
    }
    
    #[test]
    fn test_error_telemetry_severity_filter() {
        let config = ErrorTelemetryConfig {
            min_severity: ErrorSeverity::Error,
            ..Default::default()
        };
        
        let telemetry = ErrorTelemetry::new(config);
        
        // Report errors with different severities
        telemetry.report_error(
            ErrorSource::Transport, 
            ErrorSeverity::Debug, 
            "Debug message", 
            None, None, None
        );
        
        telemetry.report_error(
            ErrorSource::Transport, 
            ErrorSeverity::Warning, 
            "Warning message", 
            None, None, None
        );
        
        telemetry.report_error(
            ErrorSource::Transport, 
            ErrorSeverity::Error, 
            "Error message", 
            None, None, None
        );
        
        telemetry.report_error(
            ErrorSource::Transport, 
            ErrorSeverity::Critical, 
            "Critical message", 
            None, None, None
        );
        
        // Check that only Error and Critical messages were recorded
        let recent = telemetry.get_recent_errors(10);
        assert_eq!(recent.len(), 2);
        
        assert_eq!(recent[0].severity, ErrorSeverity::Critical);
        assert_eq!(recent[1].severity, ErrorSeverity::Error);
    }
    
    #[test]
    fn test_macro() {
        let telemetry = ErrorTelemetry::default();
        
        // Test basic macro
        report_error!(telemetry, ErrorSource::Transport, ErrorSeverity::Error, "Test message");
        
        // Test with code
        report_error!(telemetry, ErrorSource::Registry, ErrorSeverity::Warning, "Test with code", "TST-001");
        
        // Test with context
        let mut context = HashMap::new();
        context.insert("function".to_string(), "test_macro".to_string());
        context.insert("file".to_string(), "error_telemetry.rs".to_string());
        
        report_error!(telemetry, ErrorSource::Blockchain, ErrorSeverity::Critical, 
                     "Test with context", "BLK-001", context);
        
        // Check that all errors were recorded
        let recent = telemetry.get_recent_errors(10);
        assert_eq!(recent.len(), 3);
        
        assert_eq!(recent[0].severity, ErrorSeverity::Critical);
        assert_eq!(recent[0].source, ErrorSource::Blockchain);
        assert_eq!(recent[0].code, Some("BLK-001".to_string()));
        
        assert_eq!(recent[1].severity, ErrorSeverity::Warning);
        assert_eq!(recent[1].source, ErrorSource::Registry);
        assert_eq!(recent[1].code, Some("TST-001".to_string()));
        
        assert_eq!(recent[2].severity, ErrorSeverity::Error);
        assert_eq!(recent[2].source, ErrorSource::Transport);
        assert_eq!(recent[2].code, None);
    }
}
