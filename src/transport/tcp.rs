//! Enhanced TCP transport implementation with circuit breaker integration

use super::{Transport, TransportMetrics};
use crate::{
    types::SecureMessage, 
    error::Result,
    circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, RequestOutcome},
};
use async_trait::async_trait;
use std::time::{Duration, Instant};
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use std::sync::{Arc, RwLock};
use tracing::{info, debug, warn, error};

/// Enhanced TCP transport with circuit breaker for direct peer-to-peer communication
pub struct TcpTransport {
    listen_port: u16,
    listener: Option<TcpListener>,
    connection_timeout: Duration,
    pub received_messages: Arc<Mutex<Vec<SecureMessage>>>,
    /// Circuit breaker for reliability
    circuit_breaker: Arc<CircuitBreaker>,
    /// Performance metrics
    #[allow(dead_code)]
    metrics: Arc<RwLock<TransportMetrics>>,
}

impl TcpTransport {
    pub async fn new(listen_port: u16) -> Result<Self> {
        let listener = TcpListener::bind(format!("0.0.0.0:{}", listen_port)).await.ok();
        
        if listener.is_some() {
            info!("Enhanced TCP transport listening on port {} with circuit breaker", listen_port);
        } else {
            warn!("Failed to bind TCP port {}, will operate in client-only mode", listen_port);
        }
        
        Ok(Self {
            listen_port,
            listener,
            connection_timeout: Duration::from_secs(10),
            received_messages: Arc::new(Mutex::new(Vec::new())),
            circuit_breaker: Arc::new(CircuitBreaker::new(CircuitBreakerConfig::default())),
            metrics: Arc::new(RwLock::new(TransportMetrics::default())),
        })
    }
    
    /// Get circuit breaker reference for monitoring
    pub fn get_circuit_breaker(&self) -> Arc<CircuitBreaker> {
        Arc::clone(&self.circuit_breaker)
    }
    
    /// Get circuit breaker statistics
    pub fn get_circuit_breaker_stats(&self) -> crate::circuit_breaker::CircuitStats {
        self.circuit_breaker.get_stats()
    }
    
    /// Start listening for incoming connections
    pub async fn start_server(&mut self) -> Result<()> {
        if let Some(listener) = &self.listener {
            info!("Starting TCP server on port {}", self.listen_port);
            let message_queue = Arc::clone(&self.received_messages);
            
            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        debug!("Accepted TCP connection from {}", addr);
                        let queue_clone = Arc::clone(&message_queue);
                        tokio::spawn(async move {
                            Self::handle_connection(stream, queue_clone).await;
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept TCP connection: {}", e);
                    }
                }
            }
        } else {
            Err(crate::error::EmrpError::Transport("TCP listener not available".into()))
        }
    }
    
    async fn handle_connection(mut stream: TcpStream, message_queue: Arc<Mutex<Vec<SecureMessage>>>) {
        let mut buffer = vec![0; 8192];
        
        match stream.read(&mut buffer).await {
            Ok(bytes_read) => {
                debug!("Received {} bytes via TCP", bytes_read);
                buffer.truncate(bytes_read);
                
                // Parse and handle the message
                if let Ok(message_str) = String::from_utf8(buffer) {
                    if let Ok(message) = serde_json::from_str::<SecureMessage>(&message_str) {
                        debug!("Received EMRP message via TCP: {}", message.message_id);
                        
                        // Queue the received message
                        if let Ok(mut queue) = message_queue.try_lock() {
                            queue.push(message);
                            debug!("Queued TCP message, total messages: {}", queue.len());
                        }
                    }
                }
            }
            Err(e) => {
                warn!("Error reading from TCP connection: {}", e);
            }
        }
    }
    
    pub async fn connect(&self, host: &str, port: u16) -> Result<TcpStream> {
        let addr = format!("{}:{}", host, port);
        debug!("Attempting TCP connection to {}", addr);
        
        match tokio::time::timeout(self.connection_timeout, TcpStream::connect(&addr)).await {
            Ok(Ok(stream)) => {
                debug!("Successfully connected to {}", addr);
                Ok(stream)
            }
            Ok(Err(e)) => {
                debug!("Failed to connect to {}: {}", addr, e);
                Err(crate::error::EmrpError::Transport(format!("TCP connection failed: {}", e)))
            }
            Err(_) => {
                debug!("Timeout connecting to {}", addr);
                Err(crate::error::EmrpError::Transport("TCP connection timeout".into()))
            }
        }
    }
    
    pub async fn send_via_stream(&self, stream: &mut TcpStream, message: &SecureMessage) -> Result<()> {
        let message_json = serde_json::to_string(message)
            .map_err(|e| crate::error::EmrpError::Transport(format!("Failed to serialize message: {}", e)))?;
        
        stream.write_all(message_json.as_bytes()).await
            .map_err(|e| crate::error::EmrpError::Transport(format!("Failed to send TCP message: {}", e)))?;
        
        stream.flush().await
            .map_err(|e| crate::error::EmrpError::Transport(format!("Failed to flush TCP stream: {}", e)))?;
        
        Ok(())
    }
    
    /// Internal message sending implementation without circuit breaker checks
    async fn send_message_internal(&self, target: &str, message: &SecureMessage) -> Result<String> {
        // Parse target - handle both "host:port" and "host" formats
        if let Some((host, port_str)) = target.rsplit_once(':') {
            if let Ok(port) = port_str.parse::<u16>() {
                // Direct connection to specified host:port
                if let Ok(mut stream) = self.connect(host, port).await {
                    match self.send_via_stream(&mut stream, message).await {
                        Ok(_) => {
                            info!("Successfully sent message via TCP to {}:{}", host, port);
                            return Ok(format!("tcp://{}:{}", host, port));
                        }
                        Err(e) => {
                            warn!("Failed to send via TCP to {}:{}: {}", host, port, e);
                        }
                    }
                }
            }
        }
        
        // Fallback: try common EMRP ports on the target host
        let host = if target.contains(':') {
            target.split(':').next().unwrap_or(target)
        } else {
            target
        };
        
        let ports = vec![8080, 8443, 9090, 7777];
        for port in ports {
            if let Ok(mut stream) = self.connect(host, port).await {
                match self.send_via_stream(&mut stream, message).await {
                    Ok(_) => {
                        info!("Successfully sent message via TCP to {}:{}", host, port);
                        return Ok(format!("tcp://{}:{}", host, port));
                    }
                    Err(e) => {
                        warn!("Failed to send via TCP to {}:{}: {}", host, port, e);
                        continue;
                    }
                }
            }
        }
        
        Err(crate::error::EmrpError::Transport("No TCP ports available".into()))
    }

    /// Internal connectivity test without circuit breaker checks
    async fn test_connectivity_internal(&self, target: &str) -> Result<Duration> {
        let start = Instant::now();
        let ports = vec![8080, 8443, 9090, 7777];
        
        for port in ports {
            if let Ok(_stream) = self.connect(target, port).await {
                return Ok(start.elapsed());
            }
        }
        
        Err(crate::error::EmrpError::Transport("TCP connectivity test failed".into()))
    }
}

#[async_trait]
impl Transport for TcpTransport {
    async fn send_message(&self, target: &str, message: &SecureMessage) -> Result<String> {
        // Check circuit breaker before proceeding
        if !self.circuit_breaker.can_proceed().await {
            return Err(crate::error::EmrpError::Transport(
                format!("Circuit breaker is open for target {}", target)
            ));
        }

        let _start_time = Instant::now();
        let result = self.send_message_internal(target, message).await;

        // Record the outcome with the circuit breaker
        match &result {
            Ok(_) => {
                self.circuit_breaker.record_outcome(RequestOutcome::Success).await;
            }
            Err(e) => {
                self.circuit_breaker.record_outcome(RequestOutcome::Failure(e.to_string())).await;
            }
        }

        result
    }
    
    async fn test_connectivity(&self, target: &str) -> Result<super::TransportMetrics> {
        // Check circuit breaker before proceeding
        if !self.circuit_breaker.can_proceed().await {
            return Err(crate::error::EmrpError::Transport(
                format!("Circuit breaker is open for target {}", target)
            ));
        }

        let start_time = Instant::now();
        let result = self.test_connectivity_internal(target).await;

        // Record the outcome with the circuit breaker
        match &result {
            Ok(_) => {
                self.circuit_breaker.record_outcome(RequestOutcome::Success).await;
                // Return metrics for successful connectivity test
                Ok(super::TransportMetrics {
                    latency: start_time.elapsed(),
                    throughput_bps: 10_000_000, // 10Mbps for TCP
                    packet_loss: 0.001,
                    jitter_ms: 5,
                    reliability_score: 0.90,
                    last_updated: Instant::now(),
                })
            }
            Err(e) => {
                self.circuit_breaker.record_outcome(RequestOutcome::Failure(e.to_string())).await;
                Err(e.clone())
            }
        }
    }

    async fn receive_messages(&self) -> Result<Vec<SecureMessage>> {
        // Return and clear all received messages
        let mut messages = self.received_messages.lock().await;
        let result = messages.clone();
        messages.clear();
        Ok(result)
    }

    async fn can_reach(&self, target: &str) -> bool {
        // Parse target into host and port
        if let Some((host, port_str)) = target.rsplit_once(':') {
            if let Ok(port) = port_str.parse::<u16>() {
                // Test connectivity to specific host:port
                return self.connect(host, port).await.is_ok();
            }
        }
        
        // Fallback: try EMRP default ports if no port specified
        let ports = vec![8080, 8443, 9090, 7777];
        for port in ports {
            if self.connect(target, port).await.is_ok() {
                return true;
            }
        }
        false
    }
    
    fn get_capabilities(&self) -> Vec<String> {
        let mut caps = vec![
            "tcp".to_string(), 
            "streaming".to_string(), 
            "bidirectional".to_string(),
            "circuit_breaker".to_string(),
        ];
        
        if self.listener.is_some() {
            caps.push("server".to_string());
        }
        
        caps
    }
    
    fn estimated_latency(&self) -> Duration {
        Duration::from_millis(50) // 50ms typical for TCP
    }
    
    fn reliability_score(&self) -> f32 {
        0.90 // High reliability for TCP
    }
}
