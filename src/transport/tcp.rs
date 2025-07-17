//! Enhanced TCP transport implementation with circuit breaker integration

use super::abstraction::{Transport, TransportMetrics};
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
            Err(crate::error::SynapseError::TransportError("TCP listener not available".into()))
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
                        debug!("Received Synapse message via TCP: {}", message.message_id);
                        
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
                Err(crate::error::SynapseError::TransportError(format!("TCP connection failed: {}", e)))
            }
            Err(_) => {
                debug!("Timeout connecting to {}", addr);
                Err(crate::error::SynapseError::TransportError("TCP connection timeout".into()))
            }
        }
    }
    
    pub async fn send_via_stream(&self, stream: &mut TcpStream, message: &SecureMessage) -> Result<()> {
        let message_json = serde_json::to_string(message)
            .map_err(|e| crate::error::SynapseError::TransportError(format!("Failed to serialize message: {}", e)))?;
        
        stream.write_all(message_json.as_bytes()).await
            .map_err(|e| crate::error::SynapseError::TransportError(format!("Failed to send TCP message: {}", e)))?;
        
        stream.flush().await
            .map_err(|e| crate::error::SynapseError::TransportError(format!("Failed to flush TCP stream: {}", e)))?;
        
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
        
        // Fallback: try common Synapse ports on the target host
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
        
        Err(crate::error::SynapseError::TransportError("No TCP ports available".into()))
    }

    /// Internal connectivity test without circuit breaker checks
    #[allow(dead_code)]
    async fn test_connectivity_internal(&self, target: &str) -> Result<Duration> {
        let start = Instant::now();
        let ports = vec![8080, 8443, 9090, 7777];
        
        for port in ports {
            if let Ok(_stream) = self.connect(target, port).await {
                return Ok(start.elapsed());
            }
        }
        
        Err(crate::error::SynapseError::TransportError("TCP connectivity test failed".into()))
    }
}

#[async_trait]
impl Transport for TcpTransport {
    fn transport_type(&self) -> super::abstraction::TransportType {
        super::abstraction::TransportType::Tcp
    }
    
    fn capabilities(&self) -> super::abstraction::TransportCapabilities {
        super::abstraction::TransportCapabilities {
            max_message_size: 1_048_576, // 1MB
            reliable: true,
            real_time: true,
            broadcast: false,
            bidirectional: true,
            encrypted: false,
            network_spanning: true,
            supported_urgencies: vec![
                super::abstraction::MessageUrgency::RealTime,
                super::abstraction::MessageUrgency::Interactive,
                super::abstraction::MessageUrgency::Background
            ],
            features: vec![
                "circuit_breaker".to_string(),
                "streaming".to_string()
            ],
        }
    }
    
    async fn can_reach(&self, target: &super::abstraction::TransportTarget) -> bool {
        // Parse target into host and port
        if let Some(addr) = &target.address {
            if let Some((host, port_str)) = addr.rsplit_once(':') {
                if let Ok(port) = port_str.parse::<u16>() {
                    return self.connect(host, port).await.is_ok();
                }
            }
        }
        
        // Try with identifier if no specific address
        let host = &target.identifier;
        let ports = vec![8080, 8443, 9090, 7777];
        for port in ports {
            if self.connect(host, port).await.is_ok() {
                return true;
            }
        }
        false
    }
    
    async fn estimate_metrics(&self, target: &super::abstraction::TransportTarget) -> Result<super::abstraction::TransportEstimate> {
        Ok(super::abstraction::TransportEstimate {
            latency: Duration::from_millis(50),
            reliability: 0.95,
            bandwidth: 10_000_000, // 10 Mbps
            cost: 0.1,
            available: self.can_reach(target).await,
            confidence: 0.8,
        })
    }
    
    async fn send_message(&self, target: &super::abstraction::TransportTarget, message: &SecureMessage) -> Result<super::abstraction::DeliveryReceipt> {
        // Check circuit breaker before proceeding
        if !self.circuit_breaker.can_proceed().await {
            return Err(crate::error::SynapseError::TransportError(
                format!("Circuit breaker is open for target {}", target.identifier)
            ));
        }

        let start_time = Instant::now();
        let result = self.send_message_internal(&target.identifier, message).await;

        // Record the outcome with the circuit breaker
        match &result {
            Ok(_) => {
                self.circuit_breaker.record_outcome(RequestOutcome::Success).await;
                Ok(super::abstraction::DeliveryReceipt {
                    message_id: message.message_id.0.to_string(),
                    transport_used: self.transport_type(),
                    delivery_time: start_time.elapsed(),
                    target_reached: target.identifier.clone(),
                    confirmation: super::abstraction::DeliveryConfirmation::Sent,
                    metadata: std::collections::HashMap::new(),
                })
            }
            Err(e) => {
                self.circuit_breaker.record_outcome(RequestOutcome::Failure(e.to_string())).await;
                Err((*e).clone())
            }
        }
    }
    
    async fn receive_messages(&self) -> Result<Vec<super::abstraction::IncomingMessage>> {
        let mut messages = self.received_messages.lock().await;
        let result: Vec<_> = messages.drain(..).map(|msg| {
            super::abstraction::IncomingMessage::new(
                msg,
                self.transport_type(),
                String::new() // Source address set later
            )
        }).collect();
        Ok(result)
    }
    
    async fn test_connectivity(&self, target: &super::abstraction::TransportTarget) -> Result<super::abstraction::ConnectivityResult> {
        let start = Instant::now();
        let can_reach = self.can_reach(target).await;
        let rtt = if can_reach { Some(start.elapsed()) } else { None };
        
        Ok(super::abstraction::ConnectivityResult {
            connected: can_reach,
            rtt,
            error: if can_reach { None } else { Some("Could not establish connection".to_string()) },
            quality: if can_reach { 0.9 } else { 0.0 },
            details: std::collections::HashMap::new(),
        })
    }

    async fn start(&self) -> Result<()> {
        Ok(()) // Already started in new()
    }
    
    async fn stop(&self) -> Result<()> {
        Ok(()) // No cleanup needed
    }
    
    async fn status(&self) -> super::abstraction::TransportStatus {
        if self.listener.is_some() {
            super::abstraction::TransportStatus::Running
        } else {
            super::abstraction::TransportStatus::Degraded
        }
    }
    
    async fn metrics(&self) -> super::abstraction::TransportMetrics {
        let metrics = self.metrics.read().unwrap();
        metrics.clone()
    }
}
