use std::{
    collections::HashMap,
    time::{Duration, Instant},
    sync::{Arc, RwLock},
    net::SocketAddr,
};

use tokio::{
    io::AsyncWriteExt,
    time::timeout,
    net::TcpStream,
};

use async_trait::async_trait;
use tracing::info;

use crate::{
    error::{SynapseError, Result},
    circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, RequestOutcome},
    types::SecureMessage,
    transport::{
        abstraction::{
            Transport, TransportType, TransportTarget, TransportCapabilities, 
            TransportStatus, TransportMetrics, TransportEstimate, DeliveryReceipt, 
            IncomingMessage, ConnectivityResult, MessageUrgency, DeliveryConfirmation
        },
    },
};

/// Simple TCP transport implementation
pub struct SimpleTcpTransport {
    circuit_breaker: CircuitBreaker,
    connection_timeout: Duration,
    metrics: Arc<RwLock<TransportMetrics>>,
    capabilities: TransportCapabilities,
}

impl SimpleTcpTransport {
    pub fn new() -> Self {
        let config = CircuitBreakerConfig::default();
        
        Self {
            circuit_breaker: CircuitBreaker::new(config),
            connection_timeout: Duration::from_secs(10),
            metrics: Arc::new(RwLock::new(TransportMetrics {
                transport_type: TransportType::Tcp,
                ..Default::default()
            })),
            capabilities: TransportCapabilities {
                max_message_size: 64 * 1024 * 1024, // 64MB
                reliable: true,
                real_time: true,
                broadcast: false,
                bidirectional: true,
                encrypted: false, // TLS would be handled separately
                network_spanning: true,
                supported_urgencies: vec![
                    MessageUrgency::Critical,
                    MessageUrgency::RealTime,
                    MessageUrgency::Interactive,
                    MessageUrgency::Background,
                ],
                features: vec!["tcp".to_string(), "stream".to_string()],
            },
        }
    }
}

#[async_trait]
impl Transport for SimpleTcpTransport {
    fn transport_type(&self) -> TransportType {
        TransportType::Tcp
    }

    fn capabilities(&self) -> TransportCapabilities {
        self.capabilities.clone()
    }

    async fn can_reach(&self, target: &TransportTarget) -> bool {
        if let Some(address) = &target.address {
            address.parse::<SocketAddr>().is_ok()
        } else {
            false
        }
    }

    async fn estimate_metrics(&self, target: &TransportTarget) -> Result<TransportEstimate> {
        let latency = if let Some(address) = &target.address {
            if address.starts_with("127.0.0.1") || address.starts_with("localhost") {
                Duration::from_millis(1)
            } else if address.starts_with("192.168.") || address.starts_with("10.") {
                Duration::from_millis(10)
            } else {
                Duration::from_millis(100)
            }
        } else {
            Duration::from_millis(500)
        };

        Ok(TransportEstimate {
            latency,
            reliability: 0.95,
            bandwidth: 1_000_000, // 1 Mbps estimate
            cost: 1.0,
            available: true,
            confidence: 0.8,
        })
    }

    async fn send_message(&self, target: &TransportTarget, message: &SecureMessage) -> Result<DeliveryReceipt> {
        let _start = Instant::now();
        
        if !self.circuit_breaker.can_proceed().await {
            self.circuit_breaker.record_outcome(RequestOutcome::Failure("Circuit breaker open".to_string())).await;
            return Err(SynapseError::TransportError("Circuit breaker is open".to_string()));
        }

        let addr = if let Some(address) = &target.address {
            address.parse::<SocketAddr>()
                .map_err(|e| SynapseError::TransportError(format!("Invalid address: {}", e)))?
        } else {
            return Err(SynapseError::TransportError("No address provided for TCP transport".to_string()));
        };

        let stream = timeout(self.connection_timeout, TcpStream::connect(addr))
            .await
            .map_err(|_| SynapseError::TransportError("Connection timeout".to_string()))?
            .map_err(|e| SynapseError::TransportError(format!("Connection failed: {}", e)))?;

        match self.send_to_stream(stream, message).await {
            Ok(receipt) => {
                {
                    let mut metrics = self.metrics.write().unwrap();
                    metrics.messages_sent += 1;
                    metrics.bytes_sent += message.encrypted_content.len() as u64;
                    metrics.last_updated_timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                }
                self.circuit_breaker.record_outcome(RequestOutcome::Success).await;
                Ok(receipt)
            }
            Err(e) => {
                {
                    let mut metrics = self.metrics.write().unwrap();
                    metrics.send_failures += 1;
                }
                self.circuit_breaker.record_outcome(RequestOutcome::Failure(format!("Send failed: {}", e))).await;
                Err(e)
            }
        }
    }

    async fn receive_messages(&self) -> Result<Vec<IncomingMessage>> {
        // For this simple implementation, we don't maintain persistent listeners
        // This would typically be implemented with a background task
        Ok(vec![])
    }

    async fn test_connectivity(&self, target: &TransportTarget) -> Result<ConnectivityResult> {
        let addr = if let Some(address) = &target.address {
            address.parse::<SocketAddr>()
                .map_err(|e| SynapseError::TransportError(format!("Invalid address: {}", e)))?
        } else {
            return Ok(ConnectivityResult {
                connected: false,
                rtt: None,
                error: Some("No address provided".to_string()),
                quality: 0.0,
                details: HashMap::new(),
            });
        };

        let start = Instant::now();
        match timeout(Duration::from_secs(5), TcpStream::connect(addr)).await {
            Ok(Ok(_stream)) => {
                let rtt = start.elapsed();
                Ok(ConnectivityResult {
                    connected: true,
                    rtt: Some(rtt),
                    error: None,
                    quality: if rtt.as_millis() < 100 { 1.0 } else { 0.5 },
                    details: HashMap::new(),
                })
            }
            Ok(Err(e)) => {
                Ok(ConnectivityResult {
                    connected: false,
                    rtt: None,
                    error: Some(format!("Connection failed: {}", e)),
                    quality: 0.0,
                    details: HashMap::new(),
                })
            }
            Err(_) => {
                Ok(ConnectivityResult {
                    connected: false,
                    rtt: None,
                    error: Some("Connection timeout".to_string()),
                    quality: 0.0,
                    details: HashMap::new(),
                })
            }
        }
    }

    async fn start(&self) -> Result<()> {
        info!("TCP Simple transport started");
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        info!("TCP Simple transport stopped");
        Ok(())
    }

    async fn status(&self) -> TransportStatus {
        TransportStatus::Running
    }

    async fn metrics(&self) -> TransportMetrics {
        self.metrics.read().unwrap().clone()
    }
}

impl SimpleTcpTransport {
    async fn send_to_stream(&self, mut stream: TcpStream, message: &SecureMessage) -> Result<DeliveryReceipt> {
        let serialized = serde_json::to_vec(message)
            .map_err(|e| SynapseError::TransportError(format!("Serialization failed: {}", e)))?;

        let length_bytes = (serialized.len() as u32).to_be_bytes();
        stream.write_all(&length_bytes).await
            .map_err(|e| SynapseError::TransportError(format!("Failed to send length: {}", e)))?;
        
        stream.write_all(&serialized).await
            .map_err(|e| SynapseError::TransportError(format!("Failed to send message: {}", e)))?;

        stream.flush().await
            .map_err(|e| SynapseError::TransportError(format!("Failed to flush stream: {}", e)))?;

        Ok(DeliveryReceipt {
            message_id: message.message_id.0.to_string(),
            transport_used: TransportType::Tcp,
            delivery_time: Duration::from_millis(1), // Placeholder
            target_reached: message.to_global_id.clone(),
            confirmation: DeliveryConfirmation::Sent,
            metadata: HashMap::new(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct SimpleTcpTransportFactory;

impl SimpleTcpTransportFactory {
    pub fn new() -> Self {
        Self
    }

    pub fn create(&self) -> Box<dyn Transport> {
        Box::new(SimpleTcpTransport::new())
    }
}
