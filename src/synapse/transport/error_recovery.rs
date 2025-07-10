// Error recovery and circuit breaker implementation for transport layer

use std::{
    sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}},
    time::{Duration, Instant},
};
use anyhow::Result;

/// Transport circuit breaker to prevent cascading failures
pub struct CircuitBreaker {
    name: String,
    state: Arc<Mutex<CircuitBreakerState>>,
    failure_threshold: u32,
    reset_timeout: Duration,
    half_open_allowed_calls: u32,
}

enum CircuitBreakerState {
    Closed { failures: u32 },
    Open { opened_at: Instant },
    HalfOpen { successful_calls: u32 },
}

impl CircuitBreaker {
    pub fn new(name: &str, failure_threshold: u32, reset_timeout: Duration) -> Self {
        Self {
            name: name.to_string(),
            state: Arc::new(Mutex::new(CircuitBreakerState::Closed { failures: 0 })),
            failure_threshold,
            reset_timeout,
            half_open_allowed_calls: 3,
        }
    }
    
    pub fn allow_request(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        
        match *state {
            CircuitBreakerState::Closed { .. } => true,
            CircuitBreakerState::Open { opened_at } => {
                if opened_at.elapsed() >= self.reset_timeout {
                    // Move to half-open state after timeout
                    *state = CircuitBreakerState::HalfOpen { successful_calls: 0 };
                    true
                } else {
                    false
                }
            }
            CircuitBreakerState::HalfOpen { .. } => {
                // Limited requests allowed in half-open state
                true
            }
        }
    }
    
    pub fn record_success(&self) {
        let mut state = self.state.lock().unwrap();
        
        match *state {
            CircuitBreakerState::Closed { .. } => {
                // Reset failures
                *state = CircuitBreakerState::Closed { failures: 0 };
            }
            CircuitBreakerState::HalfOpen { successful_calls } => {
                if successful_calls + 1 >= self.half_open_allowed_calls {
                    // Return to closed state after enough successful calls
                    *state = CircuitBreakerState::Closed { failures: 0 };
                    tracing::info!("Circuit breaker '{}' closed after successful recovery", self.name);
                } else {
                    // Increment successful calls
                    *state = CircuitBreakerState::HalfOpen { successful_calls: successful_calls + 1 };
                }
            }
            _ => {}
        }
    }
    
    pub fn record_failure(&self) {
        let mut state = self.state.lock().unwrap();
        
        match *state {
            CircuitBreakerState::Closed { failures } => {
                if failures + 1 >= self.failure_threshold {
                    // Open the circuit after reaching threshold
                    *state = CircuitBreakerState::Open { opened_at: Instant::now() };
                    tracing::warn!("Circuit breaker '{}' tripped open after {} failures", 
                                  self.name, self.failure_threshold);
                } else {
                    // Increment failure count
                    *state = CircuitBreakerState::Closed { failures: failures + 1 };
                }
            }
            CircuitBreakerState::HalfOpen { .. } => {
                // Return to open state on failure in half-open
                *state = CircuitBreakerState::Open { opened_at: Instant::now() };
                tracing::warn!("Circuit breaker '{}' reopened after failure in half-open state", self.name);
            }
            _ => {}
        }
    }
    
    pub fn get_state(&self) -> String {
        let state = self.state.lock().unwrap();
        match *state {
            CircuitBreakerState::Closed { failures } => 
                format!("Closed (failures: {})", failures),
            CircuitBreakerState::Open { opened_at } => 
                format!("Open (for {:?})", opened_at.elapsed()),
            CircuitBreakerState::HalfOpen { successful_calls } => 
                format!("Half-Open (successful calls: {})", successful_calls),
        }
    }
    
    pub fn reset(&self) {
        let mut state = self.state.lock().unwrap();
        *state = CircuitBreakerState::Closed { failures: 0 };
        tracing::info!("Circuit breaker '{}' manually reset", self.name);
    }
}

/// Retry policy for transient failures
pub struct RetryPolicy {
    max_attempts: u32,
    initial_backoff: Duration,
    max_backoff: Duration,
    backoff_multiplier: f64,
    jitter_factor: f64,
}

impl RetryPolicy {
    pub fn new(max_attempts: u32, initial_backoff_ms: u64) -> Self {
        Self {
            max_attempts,
            initial_backoff: Duration::from_millis(initial_backoff_ms),
            max_backoff: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            jitter_factor: 0.1,
        }
    }
    
    pub fn with_max_backoff(mut self, max_backoff_secs: u64) -> Self {
        self.max_backoff = Duration::from_secs(max_backoff_secs);
        self
    }
    
    pub fn with_jitter_factor(mut self, jitter_factor: f64) -> Self {
        self.jitter_factor = jitter_factor.clamp(0.0, 0.5);
        self
    }
    
    pub async fn execute<F, T, E>(&self, operation: F) -> Result<T, E>
    where
        F: Fn() -> futures::future::BoxFuture<'static, Result<T, E>>,
        E: std::fmt::Debug,
    {
        let mut attempt = 0;
        let mut backoff = self.initial_backoff;
        
        loop {
            attempt += 1;
            
            match operation().await {
                Ok(result) => return Ok(result),
                Err(err) => {
                    if attempt >= self.max_attempts {
                        return Err(err);
                    }
                    
                    tracing::debug!("Operation failed on attempt {}/{}: {:?}. Retrying after {:?}...", 
                                   attempt, self.max_attempts, err, backoff);
                    
                    // Apply jitter to backoff
                    let jitter_range = (backoff.as_millis() as f64 * self.jitter_factor) as u64;
                    let jitter = if jitter_range > 0 {
                        Duration::from_millis(rand::random::<u64>() % jitter_range)
                    } else {
                        Duration::from_millis(0)
                    };
                    
                    tokio::time::sleep(backoff + jitter).await;
                    
                    // Calculate next backoff with exponential increase
                    let next_backoff_millis = backoff.as_millis() as f64 * self.backoff_multiplier;
                    backoff = std::cmp::min(
                        Duration::from_millis(next_backoff_millis as u64),
                        self.max_backoff
                    );
                }
            }
        }
    }
}

/// Connection health monitor
pub struct ConnectionHealthMonitor {
    healthy: AtomicBool,
    last_success: Mutex<Option<Instant>>,
    last_failure: Mutex<Option<Instant>>,
    failure_count: Mutex<u32>,
}

impl ConnectionHealthMonitor {
    pub fn new() -> Self {
        Self {
            healthy: AtomicBool::new(true),
            last_success: Mutex::new(Some(Instant::now())),
            last_failure: Mutex::new(None),
            failure_count: Mutex::new(0),
        }
    }
    
    pub fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::SeqCst)
    }
    
    pub fn record_success(&self) {
        let mut last_success = self.last_success.lock().unwrap();
        *last_success = Some(Instant::now());
        
        let mut failure_count = self.failure_count.lock().unwrap();
        *failure_count = 0;
        
        self.healthy.store(true, Ordering::SeqCst);
    }
    
    pub fn record_failure(&self) {
        let mut last_failure = self.last_failure.lock().unwrap();
        *last_failure = Some(Instant::now());
        
        let mut failure_count = self.failure_count.lock().unwrap();
        *failure_count += 1;
        
        // Mark as unhealthy after 3 consecutive failures
        if *failure_count >= 3 {
            self.healthy.store(false, Ordering::SeqCst);
        }
    }
    
    pub fn get_status(&self) -> ConnectionStatus {
        let last_success = self.last_success.lock().unwrap();
        let last_failure = self.last_failure.lock().unwrap();
        let failure_count = *self.failure_count.lock().unwrap();
        
        ConnectionStatus {
            healthy: self.is_healthy(),
            last_success: *last_success,
            last_failure: *last_failure,
            consecutive_failures: failure_count,
        }
    }
}

pub struct ConnectionStatus {
    pub healthy: bool,
    pub last_success: Option<Instant>,
    pub last_failure: Option<Instant>,
    pub consecutive_failures: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::test;
    
    #[test]
    async fn test_circuit_breaker() {
        let breaker = CircuitBreaker::new("test", 3, Duration::from_millis(100));
        
        // Initial state should be closed
        assert!(breaker.allow_request());
        
        // Record failures to trip the breaker
        breaker.record_failure();
        breaker.record_failure();
        assert!(breaker.allow_request());
        
        // This should trip the breaker
        breaker.record_failure();
        assert!(!breaker.allow_request());
        
        // Wait for timeout
        tokio::time::sleep(Duration::from_millis(150)).await;
        
        // Should be in half-open state
        assert!(breaker.allow_request());
        
        // Record success to return to closed state
        breaker.record_success();
        breaker.record_success();
        breaker.record_success();
        assert!(breaker.allow_request());
        
        // Should be fully closed now
        breaker.record_failure();
        assert!(breaker.allow_request());
    }
    
    #[test]
    async fn test_retry_policy() {
        let policy = RetryPolicy::new(3, 10);
        let counter = Arc::new(Mutex::new(0));
        
        // Test successful operation after retries
        {
            let counter_clone = counter.clone();
            let result = policy.execute(move || {
                let counter = counter_clone.clone();
                Box::pin(async move {
                    let mut count = counter.lock().unwrap();
                    *count += 1;
                    
                    if *count < 3 {
                        Err("Simulated failure")
                    } else {
                        Ok(*count)
                    }
                })
            }).await;
            
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), 3);
        }
        
        // Reset counter
        *counter.lock().unwrap() = 0;
        
        // Test operation that always fails
        {
            let counter_clone = counter.clone();
            let result: Result<(), &str> = policy.execute(move || {
                let counter = counter_clone.clone();
                Box::pin(async move {
                    let mut count = counter.lock().unwrap();
                    *count += 1;
                    
                    Err("Always fails")
                })
            }).await;
            
            assert!(result.is_err());
            assert_eq!(*counter.lock().unwrap(), 3); // Should have tried 3 times
        }
    }
    
    #[test]
    async fn test_health_monitor() {
        let monitor = ConnectionHealthMonitor::new();
        
        // Should start healthy
        assert!(monitor.is_healthy());
        
        // Record failures
        monitor.record_failure();
        assert!(monitor.is_healthy());
        
        monitor.record_failure();
        assert!(monitor.is_healthy());
        
        // Third failure should mark as unhealthy
        monitor.record_failure();
        assert!(!monitor.is_healthy());
        
        // Success should restore health
        monitor.record_success();
        assert!(monitor.is_healthy());
    }
}
