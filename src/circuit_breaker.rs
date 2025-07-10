//! Circuit Breaker Infrastructure for Synapse
//!
//! This module provides sophisticated circuit breaking capabilities that can be
//! triggered both internally (based on transport metrics) and externally (by
//! higher-layer components). The circuit breaker helps prevent cascading failures
//! and provides graceful degradation under stress.

use crate::transport::TransportMetrics;
use std::{
    time::{Duration, Instant},
    sync::{Arc, RwLock},
    collections::VecDeque,
};
use tracing::{info, warn};
use tokio::sync::broadcast;
use serde::{Serialize, Deserialize};

/// Circuit breaker states following the classic pattern
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CircuitState {
    /// Circuit is closed - normal operation
    Closed,
    /// Circuit is open - requests are rejected immediately
    Open,
    /// Circuit is half-open - testing if service has recovered
    HalfOpen,
}

/// Circuit breaker configuration
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures required to trip the circuit
    pub failure_threshold: u32,
    /// Minimum number of requests before considering failure rate
    pub minimum_requests: u32,
    /// Time window for failure rate calculation
    pub failure_window: Duration,
    /// How long to wait before attempting recovery
    pub recovery_timeout: Duration,
    /// Number of test requests to allow in half-open state
    pub half_open_max_calls: u32,
    /// Success threshold to close circuit from half-open
    pub success_threshold: f32, // 0.0-1.0
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            minimum_requests: 10,
            failure_window: Duration::from_secs(60),
            recovery_timeout: Duration::from_secs(30),
            half_open_max_calls: 3,
            success_threshold: 0.6,
        }
    }
}

/// External trigger interface for circuit breaking
pub trait CircuitTrigger: Send + Sync {
    /// Check if the circuit should be tripped based on external conditions
    fn should_trip(&self, metrics: &TransportMetrics, state: &CircuitState) -> bool;
    
    /// Check if the circuit should recover
    fn should_recover(&self, state: &CircuitState) -> bool;
    
    /// Get a description of this trigger
    fn description(&self) -> String;
}

/// Circuit breaker events for monitoring and logging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CircuitEvent {
    /// Circuit opened due to failures
    Opened {
        reason: String,
        failure_count: u32,
        #[serde(skip, default = "Instant::now")]
        timestamp: Instant,
    },
    /// Circuit half-opened for testing
    HalfOpened {
        #[serde(skip, default = "Instant::now")]
        timestamp: Instant,
    },
    /// Circuit closed after successful recovery
    Closed {
        #[serde(skip, default = "Instant::now")]
        timestamp: Instant,
    },
    /// Request rejected due to open circuit
    RequestRejected {
        #[serde(skip, default = "Instant::now")]
        timestamp: Instant,
    },
    /// External trigger activated
    ExternalTriggerActivated {
        trigger_name: String,
        #[serde(skip, default = "Instant::now")]
        timestamp: Instant,
    },
}

/// Circuit breaker statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitStats {
    pub state: CircuitState,
    pub failure_count: u32,
    pub success_count: u32,
    pub total_requests: u64,
    #[serde(skip, default)]
    pub last_failure_time: Option<Instant>,
    #[serde(skip, default = "Instant::now")]
    pub last_state_change: Instant,
    pub time_in_current_state: Duration,
    pub rejection_count: u64,
}

/// Request outcome for circuit breaker tracking
#[derive(Debug, Clone)]
pub enum RequestOutcome {
    Success,
    Failure(String),
    Timeout,
}

/// Main circuit breaker implementation
pub struct CircuitBreaker {
    /// Current state
    state: Arc<RwLock<CircuitState>>,
    /// Configuration
    config: CircuitBreakerConfig,
    /// Request history for failure rate calculation
    request_history: Arc<RwLock<VecDeque<(Instant, RequestOutcome)>>>,
    /// External triggers
    external_triggers: Vec<Box<dyn CircuitTrigger>>,
    /// Event broadcaster for monitoring
    event_sender: broadcast::Sender<CircuitEvent>,
    /// Statistics
    stats: Arc<RwLock<CircuitStats>>,
    /// Half-open request counter
    half_open_calls: Arc<RwLock<u32>>,
    /// Last state change time
    last_state_change: Arc<RwLock<Instant>>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker
    pub fn new(config: CircuitBreakerConfig) -> Self {
        let (event_sender, _) = broadcast::channel(100);
        let now = Instant::now();
        
        Self {
            state: Arc::new(RwLock::new(CircuitState::Closed)),
            config,
            request_history: Arc::new(RwLock::new(VecDeque::new())),
            external_triggers: Vec::new(),
            event_sender,
            stats: Arc::new(RwLock::new(CircuitStats {
                state: CircuitState::Closed,
                failure_count: 0,
                success_count: 0,
                total_requests: 0,
                last_failure_time: None,
                last_state_change: now,
                time_in_current_state: Duration::from_secs(0),
                rejection_count: 0,
            })),
            half_open_calls: Arc::new(RwLock::new(0)),
            last_state_change: Arc::new(RwLock::new(now)),
        }
    }
    
    /// Add an external trigger
    pub fn add_trigger(&mut self, trigger: Box<dyn CircuitTrigger>) {
        info!("Adding circuit breaker trigger: {}", trigger.description());
        self.external_triggers.push(trigger);
    }
    
    /// Subscribe to circuit breaker events
    pub fn subscribe_events(&self) -> broadcast::Receiver<CircuitEvent> {
        self.event_sender.subscribe()
    }
    
    /// Check if a request can proceed
    pub async fn can_proceed(&self) -> bool {
        let state = *self.state.read().unwrap();
        
        match state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if it's time to try recovery
                if self.should_attempt_recovery().await {
                    self.transition_to_half_open().await;
                    true
                } else {
                    self.record_rejection().await;
                    false
                }
            }
            CircuitState::HalfOpen => {
                let calls = *self.half_open_calls.read().unwrap();
                if calls < self.config.half_open_max_calls {
                    *self.half_open_calls.write().unwrap() += 1;
                    true
                } else {
                    self.record_rejection().await;
                    false
                }
            }
        }
    }
    
    /// Record the outcome of a request
    pub async fn record_outcome(&self, outcome: RequestOutcome) {
        let now = Instant::now();
        
        // Add to request history
        {
            let mut history = self.request_history.write().unwrap();
            history.push_back((now, outcome.clone()));
            
            // Clean old entries
            let cutoff = now - self.config.failure_window;
            while let Some((timestamp, _)) = history.front() {
                if *timestamp < cutoff {
                    history.pop_front();
                } else {
                    break;
                }
            }
        }
        
        // Update statistics
        {
            let mut stats = self.stats.write().unwrap();
            stats.total_requests += 1;
            
            match outcome {
                RequestOutcome::Success => {
                    stats.success_count += 1;
                }
                RequestOutcome::Failure(_) | RequestOutcome::Timeout => {
                    stats.failure_count += 1;
                    stats.last_failure_time = Some(now);
                }
            }
        }
        
        // Check state transitions
        let current_state = *self.state.read().unwrap();
        match current_state {
            CircuitState::Closed => {
                if self.should_trip().await {
                    self.transition_to_open("Failure threshold exceeded").await;
                }
            }
            CircuitState::HalfOpen => {
                match outcome {
                    RequestOutcome::Success => {
                        if self.should_close_from_half_open().await {
                            self.transition_to_closed().await;
                        }
                    }
                    RequestOutcome::Failure(_) | RequestOutcome::Timeout => {
                        self.transition_to_open("Failure during half-open test").await;
                    }
                }
            }
            CircuitState::Open => {
                // No action needed - already open
            }
        }
    }
    
    /// Force the circuit to open (external trigger)
    pub async fn force_open(&self, reason: &str) {
        info!("Circuit breaker forced open: {}", reason);
        self.transition_to_open(reason).await;
        
        let event = CircuitEvent::ExternalTriggerActivated {
            trigger_name: reason.to_string(),
            timestamp: Instant::now(),
        };
        let _ = self.event_sender.send(event);
    }
    
    /// Force the circuit to close (external override)
    pub async fn force_close(&self) {
        info!("Circuit breaker forced closed");
        self.transition_to_closed().await;
    }
    
    /// Check external triggers
    pub async fn check_external_triggers(&self, metrics: &TransportMetrics) {
        let current_state = *self.state.read().unwrap();
        
        for trigger in &self.external_triggers {
            if trigger.should_trip(metrics, &current_state) {
                let reason = format!("External trigger: {}", trigger.description());
                self.force_open(&reason).await;
                return;
            }
            
            if current_state == CircuitState::Open && trigger.should_recover(&current_state) {
                info!("External trigger suggests recovery: {}", trigger.description());
                self.transition_to_half_open().await;
                return;
            }
        }
    }
    
    /// Get current statistics
    pub fn get_stats(&self) -> CircuitStats {
        let mut stats = self.stats.read().unwrap().clone();
        let last_change = *self.last_state_change.read().unwrap();
        stats.time_in_current_state = Instant::now().duration_since(last_change);
        stats
    }
    
    /// Get current state
    pub fn get_state(&self) -> CircuitState {
        *self.state.read().unwrap()
    }
    
    // Private helper methods
    
    async fn should_trip(&self) -> bool {
        let history = self.request_history.read().unwrap();
        
        if history.len() < self.config.minimum_requests as usize {
            return false;
        }
        
        let failure_count = history.iter()
            .filter(|(_, outcome)| matches!(outcome, RequestOutcome::Failure(_) | RequestOutcome::Timeout))
            .count() as u32;
        
        failure_count >= self.config.failure_threshold
    }
    
    async fn should_attempt_recovery(&self) -> bool {
        let last_change = *self.last_state_change.read().unwrap();
        Instant::now().duration_since(last_change) >= self.config.recovery_timeout
    }
    
    async fn should_close_from_half_open(&self) -> bool {
        let history = self.request_history.read().unwrap();
        let half_open_requests: Vec<_> = history.iter()
            .rev()
            .take(self.config.half_open_max_calls as usize)
            .collect();
        
        if half_open_requests.is_empty() {
            return false;
        }
        
        let success_count = half_open_requests.iter()
            .filter(|(_, outcome)| matches!(outcome, RequestOutcome::Success))
            .count();
        
        let success_rate = success_count as f32 / half_open_requests.len() as f32;
        success_rate >= self.config.success_threshold
    }
    
    async fn transition_to_open(&self, reason: &str) {
        let now = Instant::now();
        *self.state.write().unwrap() = CircuitState::Open;
        *self.last_state_change.write().unwrap() = now;
        
        let failure_count = {
            let stats = self.stats.read().unwrap();
            stats.failure_count
        };
        
        {
            let mut stats = self.stats.write().unwrap();
            stats.state = CircuitState::Open;
            stats.last_state_change = now;
        }
        
        warn!("Circuit breaker opened: {} (failures: {})", reason, failure_count);
        
        let event = CircuitEvent::Opened {
            reason: reason.to_string(),
            failure_count,
            timestamp: now,
        };
        let _ = self.event_sender.send(event);
    }
    
    async fn transition_to_half_open(&self) {
        let now = Instant::now();
        *self.state.write().unwrap() = CircuitState::HalfOpen;
        *self.last_state_change.write().unwrap() = now;
        *self.half_open_calls.write().unwrap() = 0;
        
        {
            let mut stats = self.stats.write().unwrap();
            stats.state = CircuitState::HalfOpen;
            stats.last_state_change = now;
        }
        
        info!("Circuit breaker half-opened for testing");
        
        let event = CircuitEvent::HalfOpened { timestamp: now };
        let _ = self.event_sender.send(event);
    }
    
    async fn transition_to_closed(&self) {
        let now = Instant::now();
        *self.state.write().unwrap() = CircuitState::Closed;
        *self.last_state_change.write().unwrap() = now;
        
        // Reset counters
        {
            let mut stats = self.stats.write().unwrap();
            stats.state = CircuitState::Closed;
            stats.failure_count = 0;
            stats.success_count = 0;
            stats.last_state_change = now;
        }
        
        info!("Circuit breaker closed - service recovered");
        
        let event = CircuitEvent::Closed { timestamp: now };
        let _ = self.event_sender.send(event);
    }
    
    async fn record_rejection(&self) {
        {
            let mut stats = self.stats.write().unwrap();
            stats.rejection_count += 1;
        }
        
        let event = CircuitEvent::RequestRejected {
            timestamp: Instant::now(),
        };
        let _ = self.event_sender.send(event);
    }
}

/// Pre-built external triggers for common scenarios

/// Latency-based circuit breaker trigger
pub struct LatencyTrigger {
    max_latency: Duration,
    #[allow(dead_code)]
    samples_required: usize,
}

impl LatencyTrigger {
    pub fn new(max_latency: Duration, samples_required: usize) -> Self {
        Self {
            max_latency,
            samples_required,
        }
    }
}

impl CircuitTrigger for LatencyTrigger {
    fn should_trip(&self, metrics: &TransportMetrics, state: &CircuitState) -> bool {
        if *state != CircuitState::Closed {
            return false;
        }
        
        metrics.latency > self.max_latency
    }
    
    fn should_recover(&self, _state: &CircuitState) -> bool {
        false // Let the normal recovery process handle this
    }
    
    fn description(&self) -> String {
        format!("Latency trigger (max: {:?})", self.max_latency)
    }
}

/// Reliability-based circuit breaker trigger
pub struct ReliabilityTrigger {
    min_reliability: f32,
}

impl ReliabilityTrigger {
    pub fn new(min_reliability: f32) -> Self {
        Self { min_reliability }
    }
}

impl CircuitTrigger for ReliabilityTrigger {
    fn should_trip(&self, metrics: &TransportMetrics, state: &CircuitState) -> bool {
        if *state != CircuitState::Closed {
            return false;
        }
        
        metrics.reliability_score < self.min_reliability
    }
    
    fn should_recover(&self, _state: &CircuitState) -> bool {
        false
    }
    
    fn description(&self) -> String {
        format!("Reliability trigger (min: {:.2})", self.min_reliability)
    }
}

/// Packet loss trigger
pub struct PacketLossTrigger {
    max_packet_loss: f32,
}

impl PacketLossTrigger {
    pub fn new(max_packet_loss: f32) -> Self {
        Self { max_packet_loss }
    }
}

impl CircuitTrigger for PacketLossTrigger {
    fn should_trip(&self, metrics: &TransportMetrics, state: &CircuitState) -> bool {
        if *state != CircuitState::Closed {
            return false;
        }
        
        metrics.packet_loss > self.max_packet_loss
    }
    
    fn should_recover(&self, _state: &CircuitState) -> bool {
        false
    }
    
    fn description(&self) -> String {
        format!("Packet loss trigger (max: {:.2}%)", self.max_packet_loss * 100.0)
    }
}

/// Circuit breaker manager for handling multiple circuit breakers
pub struct CircuitBreakerManager {
    breakers: Arc<RwLock<std::collections::HashMap<String, Arc<CircuitBreaker>>>>,
}

impl CircuitBreakerManager {
    pub fn new() -> Self {
        Self {
            breakers: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }
    
    /// Add a circuit breaker for a specific transport or endpoint
    pub fn add_breaker(&self, name: String, breaker: Arc<CircuitBreaker>) {
        let mut breakers = self.breakers.write().unwrap();
        breakers.insert(name, breaker);
    }
    
    /// Get a circuit breaker by name
    pub fn get_breaker(&self, name: &str) -> Option<Arc<CircuitBreaker>> {
        let breakers = self.breakers.read().unwrap();
        breakers.get(name).cloned()
    }
    
    /// Check all circuit breakers with current metrics
    pub async fn check_all_breakers(&self, metrics: &std::collections::HashMap<String, TransportMetrics>) {
        let breakers = self.breakers.read().unwrap().clone();
        
        for (name, breaker) in breakers {
            if let Some(transport_metrics) = metrics.get(&name) {
                breaker.check_external_triggers(transport_metrics).await;
            }
        }
    }
    
    /// Get stats for all circuit breakers
    pub fn get_all_stats(&self) -> std::collections::HashMap<String, CircuitStats> {
        let breakers = self.breakers.read().unwrap();
        breakers.iter()
            .map(|(name, breaker)| (name.clone(), breaker.get_stats()))
            .collect()
    }
}

impl Default for CircuitBreakerManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    
    #[tokio::test]
    async fn test_circuit_breaker_basic_flow() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            minimum_requests: 3,
            ..Default::default()
        };
        
        let breaker = CircuitBreaker::new(config);
        
        // Initially closed
        assert_eq!(breaker.get_state(), CircuitState::Closed);
        assert!(breaker.can_proceed().await);
        
        // Record failures
        for _ in 0..3 {
            assert!(breaker.can_proceed().await);
            breaker.record_outcome(RequestOutcome::Failure("test error".to_string())).await;
        }
        
        // Should now be open
        assert_eq!(breaker.get_state(), CircuitState::Open);
        assert!(!breaker.can_proceed().await);
    }
    
    #[tokio::test]
    async fn test_external_triggers() {
        let _breaker = Arc::new(CircuitBreaker::new(CircuitBreakerConfig::default()));
        
        let trigger = LatencyTrigger::new(Duration::from_millis(100), 1);
        
        let metrics = TransportMetrics {
            latency: Duration::from_millis(200),
            ..Default::default()
        };
        
        assert!(trigger.should_trip(&metrics, &CircuitState::Closed));
    }
    
    #[test]
    fn test_circuit_breaker_manager() {
        let manager = CircuitBreakerManager::new();
        let breaker = Arc::new(CircuitBreaker::new(CircuitBreakerConfig::default()));
        
        manager.add_breaker("test_transport".to_string(), breaker.clone());
        
        let retrieved = manager.get_breaker("test_transport");
        assert!(retrieved.is_some());
    }
}
