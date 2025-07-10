//! Transport Manager - Unified abstraction layer for all transport mechanisms
//! 
//! The TransportManager provides a single interface for applications to send/receive
//! messages across all available transport types, with intelligent transport selection,
//! automatic failover, and unified metrics.

use crate::{
    types::SecureMessage,
    error::Result,
    circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, RequestOutcome},
};
use super::abstraction::*;
use std::{
    time::{Duration, Instant},
    sync::{Arc, RwLock},
    collections::HashMap,
};
use serde::{Serialize, Deserialize};
use tracing::{info, debug, warn};
use tokio::sync::{Mutex, RwLock as TokioRwLock};

/// Configuration for the TransportManager
#[derive(Debug, Clone)]
pub struct TransportManagerConfig {
    /// Enabled transport types
    pub enabled_transports: Vec<TransportType>,
    /// Transport selection policy
    pub selection_policy: TransportSelectionPolicy,
    /// Failover configuration
    pub failover_config: FailoverConfig,
    /// Maximum time to wait for transport operations
    pub operation_timeout: Duration,
    /// How often to update transport metrics
    pub metrics_update_interval: Duration,
    /// Transport-specific configurations
    pub transport_configs: HashMap<TransportType, HashMap<String, String>>,
    /// Circuit breaker configuration
    pub circuit_breaker_config: CircuitBreakerConfig,
}

impl Default for TransportManagerConfig {
    fn default() -> Self {
        Self {
            enabled_transports: vec![
                TransportType::Tcp,
                TransportType::Udp,
                TransportType::Http,
                TransportType::Email,
                TransportType::Mdns,
            ],
            selection_policy: TransportSelectionPolicy::Adaptive,
            failover_config: FailoverConfig::default(),
            operation_timeout: Duration::from_secs(30),
            metrics_update_interval: Duration::from_secs(60),
            transport_configs: HashMap::new(),
            circuit_breaker_config: CircuitBreakerConfig {
                failure_threshold: 5,
                minimum_requests: 10,
                failure_window: Duration::from_secs(60),
                recovery_timeout: Duration::from_secs(60),
                half_open_max_calls: 3,
                success_threshold: 0.6,
            },
        }
    }
}

/// Transport selection policies
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TransportSelectionPolicy {
    /// Always use the first available transport
    FirstAvailable,
    /// Select based on message urgency
    UrgencyBased,
    /// Select based on past performance metrics
    PerformanceBased,
    /// Adaptive selection using ML-style learning
    Adaptive,
    /// Round-robin across available transports
    RoundRobin,
    /// Prefer specific transport types
    PreferenceOrder,
}

/// Failover configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailoverConfig {
    /// Enable automatic failover
    pub enabled: bool,
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Base delay between retries (with exponential backoff)
    pub retry_delay: Duration,
    /// Maximum delay between retries
    pub max_retry_delay: Duration,
    /// When to consider a transport failed
    pub failure_threshold: f64,
    /// How long to wait before retrying a failed transport
    pub recovery_timeout: Duration,
}

impl Default for FailoverConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_retries: 3,
            retry_delay: Duration::from_millis(500),
            max_retry_delay: Duration::from_secs(30),
            failure_threshold: 0.5, // 50% failure rate
            recovery_timeout: Duration::from_secs(300), // 5 minutes
        }
    }
}

/// Transport selection scoring weights
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionWeights {
    pub latency: f64,
    pub reliability: f64,
    pub bandwidth: f64,
    pub cost: f64,
    pub capability_match: f64,
}

impl Default for SelectionWeights {
    fn default() -> Self {
        Self {
            latency: 0.3,
            reliability: 0.3,
            bandwidth: 0.2,
            cost: 0.1,
            capability_match: 0.1,
        }
    }
}

/// Main TransportManager that provides unified transport abstraction
pub struct TransportManager {
    /// Configuration
    config: TransportManagerConfig,
    /// Available transport instances
    transports: TokioRwLock<HashMap<TransportType, Box<dyn Transport>>>,
    /// Transport factories for creating new instances
    factories: RwLock<HashMap<TransportType, Box<dyn TransportFactory>>>,
    /// Circuit breakers per transport
    circuit_breakers: RwLock<HashMap<TransportType, Arc<CircuitBreaker>>>,
    /// Unified metrics
    metrics: Arc<RwLock<UnifiedMetrics>>,
    /// Current transport status
    transport_status: TokioRwLock<HashMap<TransportType, TransportStatus>>,
    /// Selection weights for adaptive algorithm
    selection_weights: Arc<RwLock<SelectionWeights>>,
    /// Last selection index for round-robin
    round_robin_index: Arc<Mutex<usize>>,
    /// Failed transports and their recovery times
    failed_transports: TokioRwLock<HashMap<TransportType, Instant>>,
}

/// Unified metrics across all transports
#[derive(Debug, Default, Clone)]
pub struct UnifiedMetrics {
    /// Per-transport metrics
    pub transport_metrics: HashMap<TransportType, TransportMetrics>,
    /// Total messages sent across all transports
    pub total_messages_sent: u64,
    /// Total messages received across all transports
    pub total_messages_received: u64,
    /// Total failures across all transports
    pub total_failures: u64,
    /// Overall reliability score
    pub overall_reliability: f64,
    /// Average latency across all transports
    pub average_latency: Duration,
    /// Last updated timestamp
    pub last_updated_timestamp: u64,
}

impl UnifiedMetrics {
    /// Update the last updated timestamp to now
    pub fn touch(&mut self) {
        self.last_updated_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }
}

impl TransportManager {
    /// Create a new TransportManager
    pub fn new(config: TransportManagerConfig) -> Self {
        Self {
            config,
            transports: TokioRwLock::new(HashMap::new()),
            factories: RwLock::new(HashMap::new()),
            circuit_breakers: RwLock::new(HashMap::new()),
            metrics: Arc::new(RwLock::new(UnifiedMetrics::default())),
            transport_status: TokioRwLock::new(HashMap::new()),
            selection_weights: Arc::new(RwLock::new(SelectionWeights::default())),
            round_robin_index: Arc::new(Mutex::new(0)),
            failed_transports: TokioRwLock::new(HashMap::new()),
        }
    }

    /// Register a transport factory
    pub async fn register_factory(&self, factory: Box<dyn TransportFactory>) -> Result<()> {
        let transport_type = factory.transport_type();
        info!("Registering transport factory for {:?}", transport_type);
        
        // Create circuit breaker for this transport
        let circuit_breaker = Arc::new(CircuitBreaker::new(self.config.circuit_breaker_config.clone()));
        
        {
            let mut factories = self.factories.write().unwrap();
            factories.insert(transport_type, factory);
        }
        
        {
            let mut breakers = self.circuit_breakers.write().unwrap();
            breakers.insert(transport_type, circuit_breaker);
        }
        
        {
            let mut status = self.transport_status.write().await;
            status.insert(transport_type, TransportStatus::Stopped);
        }
        
        Ok(())
    }

    /// Initialize and start all enabled transports
    pub async fn start(&self) -> Result<()> {
        info!("Starting TransportManager with {} enabled transports", 
               self.config.enabled_transports.len());
        
        for &transport_type in &self.config.enabled_transports {
            if let Err(e) = self.start_transport(transport_type).await {
                warn!("Failed to start transport {:?}: {}", transport_type, e);
                // Continue with other transports
            }
        }
        
        // Start metrics update task
        self.start_metrics_task().await;
        
        info!("TransportManager started successfully");
        Ok(())
    }

    /// Start a specific transport
    async fn start_transport(&self, transport_type: TransportType) -> Result<()> {
        debug!("Starting transport {:?}", transport_type);
        
        // Update status to starting
        {
            let mut status = self.transport_status.write().await;
            status.insert(transport_type, TransportStatus::Starting);
        }
        
        // Get factory and create transport instance
        let transport = {
            let factories = self.factories.read().unwrap();
            if let Some(factory) = factories.get(&transport_type) {
                let config = self.config.transport_configs
                    .get(&transport_type)
                    .cloned()
                    .unwrap_or_else(|| factory.default_config());
                factory.create_transport(&config).await?
            } else {
                return Err(crate::error::EmrpError::Transport(
                    format!("No factory registered for transport {:?}", transport_type)
                ));
            }
        };
        
        // Start the transport
        transport.start().await?;
        
        // Store the transport instance
        {
            let mut transports = self.transports.write().await;
            transports.insert(transport_type, transport);
        }
        
        // Update status to running
        {
            let mut status = self.transport_status.write().await;
            status.insert(transport_type, TransportStatus::Running);
        }
        
        info!("Transport {:?} started successfully", transport_type);
        Ok(())
    }

    /// Stop all transports gracefully
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping TransportManager");
        
        let transport_types: Vec<TransportType> = {
            let transports = self.transports.read().await;
            transports.keys().cloned().collect()
        };
        
        for transport_type in transport_types {
            if let Err(e) = self.stop_transport(transport_type).await {
                warn!("Failed to stop transport {:?}: {}", transport_type, e);
            }
        }
        
        info!("TransportManager stopped");
        Ok(())
    }

    /// Stop a specific transport
    async fn stop_transport(&self, transport_type: TransportType) -> Result<()> {
        debug!("Stopping transport {:?}", transport_type);
        
        // Update status to stopping
        {
            let mut status = self.transport_status.write().await;
            status.insert(transport_type, TransportStatus::Stopping);
        }
        
        // Stop the transport
        {
            let mut transports = self.transports.write().await;
            if let Some(transport) = transports.remove(&transport_type) {
                transport.stop().await?;
            }
        }
        
        // Update status to stopped
        {
            let mut status = self.transport_status.write().await;
            status.insert(transport_type, TransportStatus::Stopped);
        }
        
        info!("Transport {:?} stopped", transport_type);
        Ok(())
    }

    /// Send a message using the best available transport
    pub async fn send_message(&self, target: &TransportTarget, message: &SecureMessage) -> Result<DeliveryReceipt> {
        debug!("Sending message to target: {}", target.identifier);
        
        let selected_transports = self.select_transports(target).await?;
        
        for transport_type in selected_transports {
            // Check if transport is failed and in recovery
            if self.is_transport_in_recovery(transport_type).await {
                debug!("Transport {:?} is in recovery, skipping", transport_type);
                continue;
            }
            
            match self.try_send_with_transport(transport_type, target, message).await {
                Ok(receipt) => {
                    self.record_success(transport_type).await;
                    self.update_transport_metrics(transport_type, true, receipt.delivery_time).await;
                    return Ok(receipt);
                }
                Err(e) => {
                    warn!("Failed to send via {:?}: {}", transport_type, e);
                    self.record_failure(transport_type).await;
                    self.update_transport_metrics(transport_type, false, Duration::from_secs(0)).await;
                    
                    // Check if we should mark this transport as failed
                    if self.should_mark_transport_failed(transport_type).await {
                        self.mark_transport_failed(transport_type).await;
                    }
                }
            }
        }
        
        Err(crate::error::EmrpError::Transport("All transports failed".to_string()))
    }

    /// Receive messages from all active transports
    pub async fn receive_messages(&self) -> Result<Vec<IncomingMessage>> {
        let mut all_messages = Vec::new();
        
        let transports = self.transports.read().await;
        for (transport_type, transport) in transports.iter() {
            // Skip failed transports
            if self.is_transport_in_recovery(*transport_type).await {
                continue;
            }
            
            match transport.receive_messages().await {
                Ok(mut messages) => {
                    debug!("Received {} messages from {:?}", messages.len(), transport_type);
                    all_messages.append(&mut messages);
                }
                Err(e) => {
                    debug!("Failed to receive from {:?}: {}", transport_type, e);
                }
            }
        }
        
        Ok(all_messages)
    }

    /// Get status of all transports
    pub async fn get_transport_status(&self) -> HashMap<TransportType, TransportStatus> {
        self.transport_status.read().await.clone()
    }

    /// Get unified metrics
    pub async fn get_metrics(&self) -> UnifiedMetrics {
        self.metrics.read().unwrap().clone()
    }

    /// List available transport types
    pub async fn list_available_transports(&self) -> Vec<TransportType> {
        let transports = self.transports.read().await;
        transports.keys().cloned().collect()
    }

    /// Get capabilities for a specific transport type
    pub async fn get_transport_capabilities(&self, transport_type: TransportType) -> Option<TransportCapabilities> {
        let transports = self.transports.read().await;
        if let Some(transport) = transports.get(&transport_type) {
            Some(transport.capabilities())
        } else {
            None
        }
    }

    /// Select optimal transport for a target
    pub async fn select_optimal_transport(&self, target: &TransportTarget) -> Result<TransportType> {
        let available_transports: Vec<_> = {
            let transports = self.transports.read().await;
            transports.keys().cloned().collect()
        };

        if available_transports.is_empty() {
            return Err(crate::error::EmrpError::Transport("No transports available".to_string()));
        }

        // Check target preferences first
        for &preferred in &target.preferred_transports {
            if available_transports.contains(&preferred) {
                let transports = self.transports.read().await;
                if let Some(transport) = transports.get(&preferred) {
                    if transport.can_reach(target).await {
                        return Ok(preferred);
                    }
                }
            }
        }

        // Fall back to the first available transport that can reach the target
        let transports = self.transports.read().await;
        for &transport_type in &available_transports {
            if let Some(transport) = transports.get(&transport_type) {
                if transport.can_reach(target).await {
                    return Ok(transport_type);
                }
            }
        }

        Err(crate::error::EmrpError::Transport("No suitable transport found".to_string()))
    }

    /// Estimate delivery for a specific transport and target
    pub async fn estimate_delivery(&self, target: &TransportTarget, transport_type: TransportType) -> Result<DeliveryEstimate> {
        let transports = self.transports.read().await;
        if let Some(transport) = transports.get(&transport_type) {
            let estimate = transport.estimate_metrics(target).await?;
            Ok(DeliveryEstimate {
                latency: estimate.latency,
                reliability: estimate.reliability,
                throughput_estimate: estimate.bandwidth,
                cost_score: estimate.cost,
            })
        } else {
            Err(crate::error::EmrpError::Transport(
                format!("Transport {:?} not available", transport_type)
            ))
        }
    }

    /// Get metrics summary for all transports
    pub async fn get_metrics_summary(&self) -> std::collections::HashMap<String, TransportMetrics> {
        let metrics = self.metrics.read().unwrap();
        let mut summary = std::collections::HashMap::new();
        
        for (transport_type, transport_metrics) in &metrics.transport_metrics {
            summary.insert(transport_type.to_string(), transport_metrics.clone());
        }
        
        summary
    }

    /// Select optimal transports for a target (ordered by preference)
    async fn select_transports(&self, target: &TransportTarget) -> Result<Vec<TransportType>> {
        match self.config.selection_policy {
            TransportSelectionPolicy::FirstAvailable => {
                self.select_first_available().await
            }
            TransportSelectionPolicy::UrgencyBased => {
                self.select_by_urgency(target.urgency).await
            }
            TransportSelectionPolicy::PerformanceBased => {
                self.select_by_performance(target).await
            }
            TransportSelectionPolicy::Adaptive => {
                self.select_adaptive(target).await
            }
            TransportSelectionPolicy::RoundRobin => {
                self.select_round_robin().await
            }
            TransportSelectionPolicy::PreferenceOrder => {
                self.select_by_preference(target).await
            }
        }
    }

    async fn select_first_available(&self) -> Result<Vec<TransportType>> {
        let status = self.transport_status.read().await;
        let available: Vec<TransportType> = status.iter()
            .filter(|(_, &status)| status == TransportStatus::Running)
            .map(|(&transport_type, _)| transport_type)
            .collect();
        
        if available.is_empty() {
            return Err(crate::error::EmrpError::Transport("No transports available".to_string()));
        }
        
        Ok(available)
    }

    async fn select_by_urgency(&self, urgency: MessageUrgency) -> Result<Vec<TransportType>> {
        let mut suitable_transports = Vec::new();
        
        let transports = self.transports.read().await;
        for (&transport_type, transport) in transports.iter() {
            let capabilities = transport.capabilities();
            if capabilities.supported_urgencies.contains(&urgency) {
                suitable_transports.push(transport_type);
            }
        }
        
        // Sort by urgency preference
        match urgency {
            MessageUrgency::Critical | MessageUrgency::RealTime => {
                // Prefer fast, real-time transports
                suitable_transports.sort_by_key(|&t| match t {
                    TransportType::Udp => 0,
                    TransportType::Quic => 1,
                    TransportType::WebSocket => 2,
                    TransportType::Tcp => 3,
                    TransportType::Mdns => 4,
                    _ => 10,
                });
            }
            MessageUrgency::Interactive => {
                // Balance speed and reliability
                suitable_transports.sort_by_key(|&t| match t {
                    TransportType::Quic => 0,
                    TransportType::WebSocket => 1,
                    TransportType::Tcp => 2,
                    TransportType::Udp => 3,
                    _ => 10,
                });
            }
            MessageUrgency::Background | MessageUrgency::Batch => {
                // Prefer reliable transports
                suitable_transports.sort_by_key(|&t| match t {
                    TransportType::Email => 0,
                    TransportType::Tcp => 1,
                    TransportType::Quic => 2,
                    _ => 10,
                });
            }
        }
        
        Ok(suitable_transports)
    }

    async fn select_by_performance(&self, target: &TransportTarget) -> Result<Vec<TransportType>> {
        let mut candidates = Vec::new();
        
        let transports = self.transports.read().await;
        for (&transport_type, transport) in transports.iter() {
            if transport.can_reach(target).await {
                if let Ok(estimate) = transport.estimate_metrics(target).await {
                    candidates.push((transport_type, estimate));
                }
            }
        }
        
        // Sort by performance score
        let weights = self.selection_weights.read().unwrap().clone();
        candidates.sort_by(|(_, a), (_, b)| {
            let score_a = self.calculate_performance_score(a, &weights);
            let score_b = self.calculate_performance_score(b, &weights);
            score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
        });
        
        Ok(candidates.into_iter().map(|(t, _)| t).collect())
    }

    async fn select_adaptive(&self, target: &TransportTarget) -> Result<Vec<TransportType>> {
        // Start with performance-based selection
        let mut selection = self.select_by_performance(target).await?;
        
        // Adjust based on recent failures and circuit breaker state
        let breakers = self.circuit_breakers.read().unwrap();
        selection.retain(|&transport_type| {
            if let Some(breaker) = breakers.get(&transport_type) {
                breaker.get_state() != crate::circuit_breaker::CircuitState::Open
            } else {
                true
            }
        });
        
        // If no transports pass circuit breaker test, fall back to urgency-based
        if selection.is_empty() {
            warn!("All transports have open circuit breakers, falling back to urgency-based selection");
            selection = self.select_by_urgency(target.urgency).await?;
        }
        
        Ok(selection)
    }

    async fn select_round_robin(&self) -> Result<Vec<TransportType>> {
        let available = self.select_first_available().await?;
        if available.is_empty() {
            return Ok(available);
        }
        
        let mut index = self.round_robin_index.lock().await;
        let selected_index = *index % available.len();
        *index = (*index + 1) % available.len();
        
        // Return selected transport first, then others as fallback
        let mut result = vec![available[selected_index]];
        for (i, &transport) in available.iter().enumerate() {
            if i != selected_index {
                result.push(transport);
            }
        }
        
        Ok(result)
    }

    async fn select_by_preference(&self, target: &TransportTarget) -> Result<Vec<TransportType>> {
        let mut result = Vec::new();
        let available = self.select_first_available().await?;
        
        // Add preferred transports first
        for &preferred in &target.preferred_transports {
            if available.contains(&preferred) {
                result.push(preferred);
            }
        }
        
        // Add remaining available transports
        for &transport in &available {
            if !result.contains(&transport) {
                result.push(transport);
            }
        }
        
        Ok(result)
    }

    fn calculate_performance_score(&self, estimate: &TransportEstimate, weights: &SelectionWeights) -> f64 {
        let latency_score = 1.0 / (1.0 + estimate.latency.as_secs_f64());
        let reliability_score = estimate.reliability;
        let bandwidth_score = (estimate.bandwidth as f64).log10() / 10.0; // Normalize bandwidth
        let cost_score = 1.0 / (1.0 + estimate.cost);
        let availability_score = if estimate.available { 1.0 } else { 0.0 };
        
        (latency_score * weights.latency +
         reliability_score * weights.reliability +
         bandwidth_score * weights.bandwidth +
         cost_score * weights.cost +
         availability_score * weights.capability_match) * estimate.confidence
    }

    async fn try_send_with_transport(
        &self, 
        transport_type: TransportType, 
        target: &TransportTarget, 
        message: &SecureMessage
    ) -> Result<DeliveryReceipt> {
        let transports = self.transports.read().await;
        if let Some(transport) = transports.get(&transport_type) {
            // Check circuit breaker state first
            let circuit_breaker = {
                let breakers = self.circuit_breakers.read().unwrap();
                breakers.get(&transport_type).cloned()
            };
            
            if let Some(breaker) = circuit_breaker {
                // Check if circuit breaker allows the request
                if breaker.get_state() == crate::circuit_breaker::CircuitState::Open {
                    return Err(crate::error::EmrpError::Transport(
                        format!("Circuit breaker is open for transport {:?}", transport_type)
                    ));
                }
                
                // Attempt the operation
                match transport.send_message(target, message).await {
                    Ok(receipt) => {
                        breaker.record_outcome(RequestOutcome::Success).await;
                        Ok(receipt)
                    }
                    Err(e) => {
                        breaker.record_outcome(RequestOutcome::Failure(e.to_string())).await;
                        Err(e)
                    }
                }
            } else {
                transport.send_message(target, message).await
            }
        } else {
            Err(crate::error::EmrpError::Transport(
                format!("Transport {:?} not available", transport_type)
            ))
        }
    }

    async fn record_success(&self, transport_type: TransportType) {
        if let Ok(breakers) = self.circuit_breakers.read() {
            if let Some(breaker) = breakers.get(&transport_type) {
                breaker.record_outcome(RequestOutcome::Success).await;
            }
        }
    }

    async fn record_failure(&self, transport_type: TransportType) {
        if let Ok(breakers) = self.circuit_breakers.read() {
            if let Some(breaker) = breakers.get(&transport_type) {
                breaker.record_outcome(RequestOutcome::Failure("Transport operation failed".to_string())).await;
            }
        }
    }

    async fn should_mark_transport_failed(&self, transport_type: TransportType) -> bool {
        if let Ok(breakers) = self.circuit_breakers.read() {
            if let Some(breaker) = breakers.get(&transport_type) {
                let stats = breaker.get_stats();
                let total_requests = stats.total_requests;
                if total_requests > 0 {
                    let failure_rate = stats.failure_count as f64 / total_requests as f64;
                    return failure_rate > self.config.failover_config.failure_threshold;
                }
            }
        }
        false
    }

    async fn mark_transport_failed(&self, transport_type: TransportType) {
        warn!("Marking transport {:?} as failed", transport_type);
        let recovery_time = Instant::now() + self.config.failover_config.recovery_timeout;
        
        {
            let mut failed = self.failed_transports.write().await;
            failed.insert(transport_type, recovery_time);
        }
        
        {
            let mut status = self.transport_status.write().await;
            status.insert(transport_type, TransportStatus::Failed);
        }
    }

    async fn is_transport_in_recovery(&self, transport_type: TransportType) -> bool {
        let failed = self.failed_transports.read().await;
        if let Some(&recovery_time) = failed.get(&transport_type) {
            Instant::now() < recovery_time
        } else {
            false
        }
    }

    async fn update_transport_metrics(&self, transport_type: TransportType, success: bool, latency: Duration) {
        let mut metrics = self.metrics.write().unwrap();
        
        if success {
            metrics.total_messages_sent += 1;
        } else {
            metrics.total_failures += 1;
        }
        
        // Update per-transport metrics
        let transport_metrics = metrics.transport_metrics
            .entry(transport_type)
            .or_insert_with(|| {
                let mut tm = TransportMetrics::default();
                tm.transport_type = transport_type;
                tm
            });
        
        if success {
            transport_metrics.messages_sent += 1;
            // Update running average latency
            let total_messages = transport_metrics.messages_sent;
            let old_avg_ms = transport_metrics.average_latency_ms as f64;
            let new_latency_ms = latency.as_millis() as f64;
            let new_avg_ms = (old_avg_ms * (total_messages - 1) as f64 + new_latency_ms) / total_messages as f64;
            transport_metrics.average_latency_ms = new_avg_ms as u64;
        } else {
            transport_metrics.send_failures += 1;
        }
        
        // Update reliability score
        let total_attempts = transport_metrics.messages_sent + transport_metrics.send_failures;
        if total_attempts > 0 {
            transport_metrics.reliability_score = transport_metrics.messages_sent as f64 / total_attempts as f64;
        }
        
        transport_metrics.touch();
        metrics.touch();
    }

    async fn start_metrics_task(&self) {
        let metrics = Arc::clone(&self.metrics);
        let interval = self.config.metrics_update_interval;
        
        tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);
            
            loop {
                interval_timer.tick().await;
                
                // Update unified metrics
                let mut metrics_guard = metrics.write().unwrap();
                
                // Calculate overall reliability
                let total_sent = metrics_guard.total_messages_sent;
                let total_failed = metrics_guard.total_failures;
                let total_attempts = total_sent + total_failed;
                
                if total_attempts > 0 {
                    metrics_guard.overall_reliability = total_sent as f64 / total_attempts as f64;
                }
                
                // Calculate average latency across all transports
                let mut total_latency_ms = 0u64;
                let mut transport_count = 0;
                
                for transport_metrics in metrics_guard.transport_metrics.values() {
                    if transport_metrics.messages_sent > 0 {
                        total_latency_ms += transport_metrics.average_latency_ms;
                        transport_count += 1;
                    }
                }
                
                if transport_count > 0 {
                    metrics_guard.average_latency = Duration::from_millis(total_latency_ms / transport_count);
                }
                
                metrics_guard.touch();
            }
        });
    }

    // Note: Individual transport access is not exposed in the public API
    // Use the manager's methods for transport operations
}

/// Builder pattern for TransportManager configuration
pub struct TransportManagerBuilder {
    config: TransportManagerConfig,
}

impl TransportManagerBuilder {
    pub fn new() -> Self {
        Self {
            config: TransportManagerConfig::default(),
        }
    }
    
    pub fn enable_transport(mut self, transport_type: TransportType) -> Self {
        if !self.config.enabled_transports.contains(&transport_type) {
            self.config.enabled_transports.push(transport_type);
        }
        self
    }
    
    pub fn disable_transport(mut self, transport_type: TransportType) -> Self {
        self.config.enabled_transports.retain(|&t| t != transport_type);
        self
    }
    
    pub fn selection_policy(mut self, policy: TransportSelectionPolicy) -> Self {
        self.config.selection_policy = policy;
        self
    }
    
    pub fn failover_config(mut self, config: FailoverConfig) -> Self {
        self.config.failover_config = config;
        self
    }
    
    pub fn operation_timeout(mut self, timeout: Duration) -> Self {
        self.config.operation_timeout = timeout;
        self
    }
    
    pub fn transport_config(mut self, transport_type: TransportType, config: HashMap<String, String>) -> Self {
        self.config.transport_configs.insert(transport_type, config);
        self
    }
    
    pub fn build(self) -> TransportManager {
        TransportManager::new(self.config)
    }
}

impl Default for TransportManagerBuilder {
    fn default() -> Self {
        Self::new()
    }
}
