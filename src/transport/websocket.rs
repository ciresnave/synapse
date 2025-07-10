//! WebSocket Transport Implementation for Synapse
//! 
//! Provides real-time bidirectional communication over WebSocket protocol,
//! ideal for web applications and browser-based clients.

use crate::{
    types::*,
    error::Result,
    circuit_breaker::CircuitBreaker,
    transport::{Transport, TransportRoute, TransportMetrics, MessageUrgency},
};
use async_trait::async_trait;
use std::{
    sync::Arc,
    time::{Duration, Instant},
    collections::HashMap,
};
use tokio::{
    sync::{RwLock, mpsc},
    time::timeout,
};
use tokio_tungstenite::{
    connect_async, accept_async,
    tungstenite::{Message, protocol::Role},
};
use futures_util::{SinkExt, StreamExt};
use tracing::{info, debug, warn, error};
use url::Url;

/// WebSocket Transport for real-time web communication
pub struct WebSocketTransport {
    local_port: u16,
    circuit_breaker: Arc<CircuitBreaker>,
    metrics: Arc<RwLock<TransportMetrics>>,
    connections: Arc<RwLock<HashMap<String, WebSocketConnection>>>,
    message_sender: Option<mpsc::UnboundedSender<(String, SecureMessage)>>,
}

/// Represents a WebSocket connection to a peer
#[derive(Debug)]
struct WebSocketConnection {
    url: String,
    connected_at: Instant,
    last_ping: Option<Instant>,
    is_client: bool,
}

impl WebSocketTransport {
    /// Create a new WebSocket transport
    pub async fn new(local_port: u16) -> Result<Self> {
        info!("Initializing WebSocket Transport on port {}", local_port);
        
        Ok(Self {
            local_port,
            circuit_breaker: Arc::new(CircuitBreaker::new(
                Duration::from_secs(10), // Timeout
                5,                       // Failure threshold
                Duration::from_secs(60), // Recovery timeout
            )),
            metrics: Arc::new(RwLock::new(TransportMetrics::default())),
            connections: Arc::new(RwLock::new(HashMap::new())),
            message_sender: None,
        })
    }
    
    /// Start WebSocket server
    pub async fn start_server(&self) -> Result<()> {
        let addr = format!("127.0.0.1:{}", self.local_port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        
        info!("WebSocket server listening on {}", addr);
        
        let connections = self.connections.clone();
        let metrics = self.metrics.clone();
        
        tokio::spawn(async move {
            while let Ok((stream, peer_addr)) = listener.accept().await {
                debug!("New WebSocket connection from {}", peer_addr);
                
                let connections_clone = connections.clone();
                let metrics_clone = metrics.clone();
                
                tokio::spawn(async move {
                    match accept_async(stream).await {
                        Ok(ws_stream) => {
                            info!("WebSocket connection established with {}", peer_addr);
                            
                            // Register connection
                            {
                                let mut conns = connections_clone.write().await;
                                conns.insert(
                                    peer_addr.to_string(),
                                    WebSocketConnection {
                                        url: format!("ws://{}", peer_addr),
                                        connected_at: Instant::now(),
                                        last_ping: None,
                                        is_client: false,
                                    },
                                );
                            }
                            
                            Self::handle_websocket_connection(ws_stream, peer_addr.to_string(), metrics_clone).await;
                        }
                        Err(e) => {
                            error!("Failed to establish WebSocket connection with {}: {}", peer_addr, e);
                        }
                    }
                });
            }
        });
        
        Ok(())
    }
    
    /// Connect to a WebSocket server
    pub async fn connect_to(&self, url: &str) -> Result<()> {
        let ws_url = Url::parse(url)?;
        
        debug!("Connecting to WebSocket at {}", url);
        
        match connect_async(&ws_url).await {
            Ok((ws_stream, _)) => {
                info!("Connected to WebSocket at {}", url);
                
                // Register connection
                {
                    let mut connections = self.connections.write().await;
                    connections.insert(
                        url.to_string(),
                        WebSocketConnection {
                            url: url.to_string(),
                            connected_at: Instant::now(),
                            last_ping: None,
                            is_client: true,
                        },
                    );
                }
                
                let metrics = self.metrics.clone();
                tokio::spawn(async move {
                    Self::handle_websocket_connection(ws_stream, url.to_string(), metrics).await;
                });
                
                Ok(())
            }
            Err(e) => {
                error!("Failed to connect to WebSocket at {}: {}", url, e);
                Err(e.into())
            }
        }
    }
    
    /// Handle WebSocket connection messages
    async fn handle_websocket_connection(
        mut ws_stream: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
        peer_id: String,
        metrics: Arc<RwLock<TransportMetrics>>,
    ) {
        while let Some(msg) = ws_stream.next().await {
            match msg {
                Ok(Message::Binary(data)) => {
                    debug!("Received binary WebSocket message from {} ({} bytes)", peer_id, data.len());
                    
                    // Update metrics
                    {
                        let mut m = metrics.write().await;
                        m.messages_received += 1;
                        m.bytes_received += data.len() as u64;
                    }
                    
                    // Try to deserialize as SecureMessage
                    match bincode::serde::decode_from_slice::<SecureMessage, _>(&data, bincode::config::standard()) {
                        Ok(message) => {
                            info!("Received valid Synapse message via WebSocket from {}", peer_id);
                            // Handle the message (would integrate with router)
                        }
                        Err(e) => {
                            warn!("Failed to deserialize WebSocket message from {}: {}", peer_id, e);
                        }
                    }
                }
                Ok(Message::Text(text)) => {
                    debug!("Received text WebSocket message from {}: {}", peer_id, text);
                }
                Ok(Message::Ping(data)) => {
                    debug!("Received WebSocket ping from {}", peer_id);
                    if let Err(e) = ws_stream.send(Message::Pong(data)).await {
                        error!("Failed to send WebSocket pong to {}: {}", peer_id, e);
                        break;
                    }
                }
                Ok(Message::Pong(_)) => {
                    debug!("Received WebSocket pong from {}", peer_id);
                }
                Ok(Message::Close(_)) => {
                    info!("WebSocket connection closed by {}", peer_id);
                    break;
                }
                Err(e) => {
                    error!("WebSocket error with {}: {}", peer_id, e);
                    break;
                }
            }
        }
        
        info!("WebSocket connection with {} ended", peer_id);
    }
    
    /// Send message via WebSocket to a specific peer
    async fn send_websocket_message(&self, target: &str, data: Vec<u8>) -> Result<()> {
        // This is a simplified implementation
        // In practice, you'd maintain active WebSocket connections
        // and route messages through them
        
        debug!("Would send WebSocket message to {} ({} bytes)", target, data.len());
        
        // Update metrics
        {
            let mut metrics = self.metrics.write().await;
            metrics.messages_sent += 1;
            metrics.bytes_sent += data.len() as u64;
        }
        
        Ok(())
    }
}

#[async_trait]
impl Transport for WebSocketTransport {
    async fn send_message(&self, target: &str, message: &SecureMessage) -> Result<()> {
        // Check circuit breaker
        if !self.circuit_breaker.can_proceed() {
            return Err("WebSocket circuit breaker is open".into());
        }
        
        let start_time = Instant::now();
        
        // Serialize message
        let serialized = bincode::serde::encode_to_vec(message, bincode::config::standard())?;
        
        match self.send_websocket_message(target, serialized).await {
            Ok(()) => {
                let duration = start_time.elapsed();
                self.circuit_breaker.record_result(start_time, true).await;
                
                // Update metrics
                {
                    let mut metrics = self.metrics.write().await;
                    metrics.average_latency = Some(duration);
                }
                
                debug!("Sent WebSocket message to {} (duration: {:?})", target, duration);
                Ok(())
            }
            Err(e) => {
                self.circuit_breaker.record_result(start_time, false).await;
                error!("Failed to send WebSocket message to {}: {}", target, e);
                Err(e)
            }
        }
    }
    
    async fn get_route_to(&self, target: &str) -> Result<TransportRoute> {
        let connections = self.connections.read().await;
        if connections.contains_key(target) {
            Ok(TransportRoute::WebSocket {
                url: target.to_string(),
                latency: Duration::from_millis(10), // Low latency for WebSocket
                reliability: 0.99, // High reliability
            })
        } else {
            Err("WebSocket route not available".into())
        }
    }
    
    async fn supports_urgency(&self, urgency: MessageUrgency) -> bool {
        matches!(urgency, MessageUrgency::RealTime | MessageUrgency::Interactive)
    }
    
    async fn get_metrics(&self) -> TransportMetrics {
        self.metrics.read().await.clone()
    }
    
    async fn is_connected(&self) -> bool {
        let connections = self.connections.read().await;
        !connections.is_empty()
    }
    
    fn transport_type(&self) -> &'static str {
        "websocket"
    }
}

/// Configuration for WebSocket transport
#[derive(Debug, Clone)]
pub struct WebSocketConfig {
    pub server_port: u16,
    pub auto_ping_interval: Duration,
    pub connection_timeout: Duration,
    pub max_frame_size: usize,
    pub enable_compression: bool,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            server_port: 8080,
            auto_ping_interval: Duration::from_secs(30),
            connection_timeout: Duration::from_secs(10),
            max_frame_size: 16 * 1024 * 1024, // 16MB
            enable_compression: true,
        }
    }
}
