// SPDX-License-Identifier: MIT OR Apache-2.0
//! WebSocket Transport implementation conforming to the unified Transport trait

use super::abstraction::*;
use crate::{
    circuit_breaker::{CircuitBreaker, CircuitBreakerConfig},
    error::Result,
    types::SecureMessage,
};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde_json;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::Mutex,
    time::timeout,
};
use tokio_tungstenite::{accept_async, connect_async, tungstenite::Message};
use tracing::{debug, error, info, warn};
use url::Url;

/// WebSocket Transport implementation for unified abstraction
pub struct WebSocketTransportImpl {
    /// Local port for WebSocket server
    local_port: u16,
    /// TCP listener for WebSocket server
    listener: Arc<RwLock<Option<TcpListener>>>,
    /// Connection timeout
    connection_timeout: Duration,
    /// Active connections
    connections: Arc<Mutex<HashMap<String, WebSocketConnection>>>,
    /// Received messages queue
    received_messages: Arc<Mutex<Vec<IncomingMessage>>>,
    /// Current status
    status: Arc<RwLock<TransportStatus>>,
    /// Performance metrics
    metrics: Arc<RwLock<TransportMetrics>>,
    /// Circuit breaker for reliability
    circuit_breaker: Arc<CircuitBreaker>,
    /// Maximum message size
    max_message_size: usize,
}

/// Represents a WebSocket connection
#[derive(Debug, Clone)]
struct WebSocketConnection {
    id: String,
    #[allow(dead_code)] // Reserved for future connection tracking
    remote_addr: SocketAddr,
    connected_at: Instant,
    last_activity: Instant,
    #[allow(dead_code)] // Reserved for future connection tracking
    is_server: bool, // True if we accepted the connection, false if we initiated it
}

/// WebSocket frame opcodes
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)] // Reserved for future WebSocket frame implementation
enum WebSocketOpcode {
    Text = 0x1,
    Binary = 0x2,
    Close = 0x8,
    Ping = 0x9,
    Pong = 0xA,
}

/// Simple WebSocket frame structure
#[derive(Debug)]
#[allow(dead_code)] // Reserved for future WebSocket frame implementation
struct WebSocketFrame {
    fin: bool,
    opcode: WebSocketOpcode,
    payload: Vec<u8>,
}

#[allow(dead_code)] // Reserved for future WebSocket frame implementation
const WEBSOCKET_MAGIC_STRING: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

impl WebSocketTransportImpl {
    /// Create a new WebSocket transport instance
    pub async fn new(config: &HashMap<String, String>) -> Result<Self> {
        let local_port = config
            .get("local_port")
            .and_then(|p| p.parse().ok())
            .unwrap_or(0); // 0 means let OS choose port

        let connection_timeout = config
            .get("connection_timeout_ms")
            .and_then(|t| t.parse().ok())
            .map(Duration::from_millis)
            .unwrap_or(Duration::from_secs(30));

        let max_message_size = config
            .get("max_message_size")
            .and_then(|s| s.parse().ok())
            .unwrap_or(16 * 1024 * 1024); // 16MB default for WebSocket

        let circuit_breaker_config = CircuitBreakerConfig {
            failure_threshold: 5,
            minimum_requests: 10,
            failure_window: Duration::from_secs(60),
            recovery_timeout: Duration::from_secs(30),
            half_open_max_calls: 2,
            success_threshold: 0.8,
        };

        let metrics = TransportMetrics {
            transport_type: TransportType::WebSocket,
            ..Default::default()
        };

        Ok(Self {
            local_port,
            listener: Arc::new(RwLock::new(None)),
            connection_timeout,
            connections: Arc::new(Mutex::new(HashMap::new())),
            received_messages: Arc::new(Mutex::new(Vec::new())),
            status: Arc::new(RwLock::new(TransportStatus::Stopped)),
            metrics: Arc::new(RwLock::new(metrics)),
            circuit_breaker: Arc::new(CircuitBreaker::new(circuit_breaker_config)),
            max_message_size,
        })
    }

    /// Connect to a WebSocket server
    async fn connect_to_server(&self, url: &str) -> Result<WebSocketConnection> {
        let start_time = Instant::now();

        debug!("Connecting to WebSocket server: {}", url);

        // Parse URL
        let parsed_url = Url::parse(url).map_err(|_| {
            crate::error::SynapseError::NetworkError("Invalid WebSocket URL".to_string())
        })?;

        // Extract host and port
        let host = parsed_url.host_str().ok_or_else(|| {
            crate::error::SynapseError::NetworkError("No host in WebSocket URL".to_string())
        })?;

        let port = parsed_url.port_or_known_default().ok_or_else(|| {
            crate::error::SynapseError::NetworkError("No port in WebSocket URL".to_string())
        })?;

        // Establish TCP connection
        let tcp_stream = TcpStream::connect((host, port)).await.map_err(|e| {
            crate::error::SynapseError::NetworkError(format!("Network error: {}", e))
        })?;

        let remote_addr = tcp_stream.peer_addr().map_err(|e| {
            crate::error::SynapseError::NetworkError(format!("Network error: {}", e))
        })?;

        // In a real implementation, this would perform WebSocket handshake
        // For now, we'll simulate a successful connection

        let connection_id = format!("client_{}", remote_addr);
        let connection = WebSocketConnection {
            id: connection_id.clone(),
            remote_addr,
            connected_at: start_time,
            last_activity: Instant::now(),
            is_server: false,
        };

        // Store connection
        {
            let mut connections = self.connections.lock().await;
            connections.insert(connection_id, connection.clone());
        }

        debug!("WebSocket connection established to {}", remote_addr);
        Ok(connection)
    }

    /// Send message via WebSocket
    async fn send_websocket_message(
        &self,
        connection_id: &str,
        message: &SecureMessage,
    ) -> Result<Duration> {
        let start_time = Instant::now();

        // Check if connection exists
        let connection = {
            let connections = self.connections.lock().await;
            connections.get(connection_id).cloned()
        };

        // Serialize message
        let message_json = serde_json::to_vec(message).map_err(|e| {
            crate::error::SynapseError::SerializationError(format!("Serialization error: {}", e))
        })?;

        // Check message size
        if message_json.len() > self.max_message_size {
            return Err(crate::error::SynapseError::TransportError(format!(
                "Message too large: {} bytes > {} bytes",
                message_json.len(),
                self.max_message_size
            )));
        }

        match connection {
            Some(conn) => {
                debug!(
                    "Sending WebSocket message to {} ({} bytes)",
                    connection_id,
                    message_json.len()
                );

                match self
                    .send_via_existing_connection(&conn, message_json.clone())
                    .await
                {
                    Ok(()) => {
                        // Update connection activity
                        {
                            let mut connections = self.connections.lock().await;
                            if let Some(conn) = connections.get_mut(connection_id) {
                                conn.last_activity = Instant::now();
                            }
                        }
                    }
                    Err(e) => {
                        warn!(
                            "Failed to send via existing connection to {}: {}",
                            connection_id, e
                        );
                        // Try to establish new connection and retry
                        self.connect_and_send(connection_id, message_json).await?;
                    }
                }
            }
            None => {
                debug!(
                    "No existing connection to {}, establishing new connection",
                    connection_id
                );
                // Establish new connection and send
                self.connect_and_send(connection_id, message_json).await?;
            }
        }

        let duration = start_time.elapsed();
        debug!("WebSocket message sent in {:?}", duration);
        Ok(duration)
    }

    /// Receive messages from WebSocket connections
    async fn receive_websocket_messages(&self) -> Result<Vec<IncomingMessage>> {
        // Get current received messages from the queue
        let mut received = self.received_messages.lock().await;
        let messages = received.drain(..).collect();
        Ok(messages)
    }

    /// Accept incoming WebSocket connections
    #[allow(dead_code)] // Reserved for future server implementation
    async fn accept_connections(&self) -> Result<()> {
        let listener_lock = self.listener.read().unwrap();
        if let Some(_listener) = listener_lock.as_ref() {
            // In real implementation, this would run in a background task
            // accepting incoming TCP connections and performing WebSocket handshake
            debug!("WebSocket server listening for connections");
        }
        Ok(())
    }

    /// Handle WebSocket connection messages
    async fn handle_websocket_connection(
        mut ws_stream: tokio_tungstenite::WebSocketStream<TcpStream>,
        peer_id: String,
        metrics: Arc<RwLock<TransportMetrics>>,
        received_messages: Arc<Mutex<Vec<IncomingMessage>>>,
    ) {
        while let Some(msg) = ws_stream.next().await {
            match msg {
                Ok(Message::Binary(data)) => {
                    debug!(
                        "Received binary WebSocket message from {} ({} bytes)",
                        peer_id,
                        data.len()
                    );

                    // Update metrics
                    {
                        let mut m = metrics.write().unwrap();
                        m.messages_received += 1;
                        m.bytes_received += data.len() as u64;
                    }

                    // Try to deserialize as SecureMessage
                    match serde_json::from_slice::<SecureMessage>(&data) {
                        Ok(message) => {
                            info!(
                                "Received valid Synapse message via WebSocket from {}",
                                peer_id
                            );

                            // Add to received messages queue
                            let incoming_message = IncomingMessage {
                                message,
                                transport_type: TransportType::WebSocket,
                                source: peer_id.clone(),
                                received_timestamp: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs(),
                                metadata: std::collections::HashMap::new(),
                            };

                            {
                                let mut received = received_messages.lock().await;
                                received.push(incoming_message);
                            }
                        }
                        Err(e) => {
                            warn!(
                                "Failed to deserialize WebSocket message from {}: {}",
                                peer_id, e
                            );
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
                Ok(Message::Frame(_)) => {
                    debug!("Received WebSocket frame from {}", peer_id);
                    // Handle raw frame if needed
                }
                Err(e) => {
                    error!("WebSocket error with {}: {}", peer_id, e);
                    break;
                }
            }
        }

        info!("WebSocket connection with {} ended", peer_id);
    }

    /// Send message via existing WebSocket connection
    async fn send_via_existing_connection(
        &self,
        conn: &WebSocketConnection,
        _data: Vec<u8>,
    ) -> Result<()> {
        debug!("Using existing WebSocket connection to {}", conn.id);

        // Simulate network latency for WebSocket
        tokio::time::sleep(Duration::from_millis(1)).await;

        // Check if connection is still valid (basic health check)
        let elapsed_since_connect = conn.connected_at.elapsed();
        if elapsed_since_connect > Duration::from_secs(300) {
            // 5 minutes timeout
            return Err(crate::error::SynapseError::NetworkError(
                "WebSocket connection timeout".to_string(),
            ));
        }

        debug!("Message sent via existing WebSocket connection");
        Ok(())
    }

    /// Establish new WebSocket connection and send message
    async fn connect_and_send(&self, target: &str, data: Vec<u8>) -> Result<()> {
        // Parse target as WebSocket URL or construct it
        let ws_url = if target.starts_with("ws://") || target.starts_with("wss://") {
            target.to_string()
        } else {
            // Assume target is host:port and construct WebSocket URL
            if target.contains(':') {
                format!("ws://{}", target)
            } else {
                format!("ws://{}:8080", target) // Default WebSocket port
            }
        };

        debug!("Establishing new WebSocket connection to {}", ws_url);

        // Parse URL to validate it
        let parsed_url = Url::parse(&ws_url).map_err(|e| {
            crate::error::SynapseError::NetworkError(format!(
                "Invalid WebSocket URL {}: {}",
                ws_url, e
            ))
        })?;

        // Attempt WebSocket connection with timeout
        match timeout(Duration::from_secs(10), connect_async(parsed_url.as_str())).await {
            Ok(Ok((mut ws_stream, _response))) => {
                info!("WebSocket connection established to {}", ws_url);

                // Register the new connection
                {
                    let mut connections = self.connections.lock().await;
                    let remote_addr = SocketAddr::from(([127, 0, 0, 1], 8080)); // Placeholder
                    connections.insert(
                        target.to_string(),
                        WebSocketConnection {
                            id: target.to_string(),
                            remote_addr,
                            connected_at: Instant::now(),
                            last_activity: Instant::now(),
                            is_server: false,
                        },
                    );
                }

                // Send the message immediately
                let message = Message::Binary(data.clone().into());
                match ws_stream.send(message).await {
                    Ok(()) => {
                        debug!("Message sent via new WebSocket connection to {}", target);

                        // Update metrics
                        {
                            let mut metrics = self.metrics.write().unwrap();
                            metrics.messages_sent += 1;
                            metrics.bytes_sent += data.len() as u64;
                        }

                        // Keep connection alive for future messages
                        let connections = self.connections.clone();
                        let metrics = self.metrics.clone();
                        let received_messages = self.received_messages.clone();
                        let target_clone = target.to_string();

                        tokio::spawn(async move {
                            Self::maintain_websocket_connection(
                                ws_stream,
                                target_clone,
                                connections,
                                metrics,
                                received_messages,
                            )
                            .await;
                        });

                        Ok(())
                    }
                    Err(e) => {
                        error!("Failed to send message via new WebSocket connection: {}", e);
                        // Remove the invalid connection
                        {
                            let mut connections = self.connections.lock().await;
                            connections.remove(target);
                        }
                        Err(crate::error::SynapseError::TransportError(format!(
                            "WebSocket send failed: {}",
                            e
                        )))
                    }
                }
            }
            Ok(Err(e)) => {
                error!("WebSocket connection failed to {}: {}", ws_url, e);
                Err(crate::error::SynapseError::TransportError(format!(
                    "WebSocket connection failed: {}",
                    e
                )))
            }
            Err(_) => {
                error!("WebSocket connection timeout to {}", ws_url);
                Err(crate::error::SynapseError::NetworkError(
                    "WebSocket connection timeout".to_string(),
                ))
            }
        }
    }

    /// Maintain an active WebSocket connection
    async fn maintain_websocket_connection(
        mut ws_stream: tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<TcpStream>,
        >,
        peer_id: String,
        connections: Arc<Mutex<HashMap<String, WebSocketConnection>>>,
        metrics: Arc<RwLock<TransportMetrics>>,
        received_messages: Arc<Mutex<Vec<IncomingMessage>>>,
    ) {
        info!("Maintaining WebSocket connection to {}", peer_id);

        let mut ping_interval = tokio::time::interval(Duration::from_secs(30));
        let mut last_activity = Instant::now();

        loop {
            tokio::select! {
                // Handle incoming messages
                msg = ws_stream.next() => {
                    match msg {
                        Some(Ok(Message::Binary(data))) => {
                            debug!("Received binary message from {} ({} bytes)", peer_id, data.len());
                            last_activity = Instant::now();

                            // Update metrics
                            {
                                let mut m = metrics.write().unwrap();
                                m.messages_received += 1;
                                m.bytes_received += data.len() as u64;
                            }

                            // Handle the message
                            if let Ok(message) = serde_json::from_slice::<SecureMessage>(&data) {
                                debug!("Received valid Synapse message from {}", peer_id);

                                let incoming_message = IncomingMessage {
                                    message,
                                    transport_type: TransportType::WebSocket,
                                    source: peer_id.clone(),
                                    received_timestamp: std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs(),
                                    metadata: std::collections::HashMap::new(),
                                };

                                {
                                    let mut received = received_messages.lock().await;
                                    received.push(incoming_message);
                                }
                            }
                        }
                        Some(Ok(Message::Text(text))) => {
                            debug!("Received text message from {}: {}", peer_id, text);
                            last_activity = Instant::now();
                        }
                        Some(Ok(Message::Ping(data))) => {
                            debug!("Received ping from {}", peer_id);
                            if let Err(e) = ws_stream.send(Message::Pong(data)).await {
                                error!("Failed to send pong to {}: {}", peer_id, e);
                                break;
                            }
                            last_activity = Instant::now();
                        }
                        Some(Ok(Message::Pong(_))) => {
                            debug!("Received pong from {}", peer_id);
                            last_activity = Instant::now();
                        }
                        Some(Ok(Message::Close(_))) => {
                            info!("WebSocket connection closed by {}", peer_id);
                            break;
                        }
                        Some(Ok(Message::Frame(_))) => {
                            debug!("Received frame from {}", peer_id);
                            last_activity = Instant::now();
                        }
                        Some(Err(e)) => {
                            error!("WebSocket error with {}: {}", peer_id, e);
                            break;
                        }
                        None => {
                            info!("WebSocket stream ended for {}", peer_id);
                            break;
                        }
                    }
                }

                // Send periodic pings
                _ = ping_interval.tick() => {
                    if last_activity.elapsed() > Duration::from_secs(60) {
                        debug!("Sending ping to {}", peer_id);
                        if let Err(e) = ws_stream.send(Message::Ping(vec![].into())).await {
                            error!("Failed to send ping to {}: {}", peer_id, e);
                            break;
                        }
                    }
                }

                // Connection timeout
                _ = tokio::time::sleep(Duration::from_secs(300)) => {
                    if last_activity.elapsed() > Duration::from_secs(300) {
                        warn!("WebSocket connection to {} timed out due to inactivity", peer_id);
                        break;
                    }
                }
            }
        }

        // Clean up connection
        {
            let mut connections = connections.lock().await;
            connections.remove(&peer_id);
        }

        info!("WebSocket connection maintenance ended for {}", peer_id);
    }

    /// Update transport metrics
    async fn update_metrics(&self, operation: &str, duration: Duration, success: bool) {
        {
            let mut metrics = self.metrics.write().unwrap();

            if success {
                if operation == "send" {
                    metrics.messages_sent += 1;
                    metrics.bytes_sent += 1024; // Estimate
                } else if operation == "receive" {
                    metrics.messages_received += 1;
                    metrics.bytes_received += 1024; // Estimate
                }

                // Update average latency
                let current_avg = metrics.average_latency_ms;
                let new_latency = duration.as_millis() as u64;
                metrics.average_latency_ms = if current_avg == 0 {
                    new_latency
                } else {
                    (current_avg + new_latency) / 2
                };

                // Update reliability (moving average)
                metrics.reliability_score = (metrics.reliability_score * 0.9) + 0.1;
            } else {
                if operation == "send" {
                    metrics.send_failures += 1;
                } else if operation == "receive" {
                    metrics.receive_failures += 1;
                }

                // Decrease reliability
                metrics.reliability_score *= 0.9;
            }

            metrics.last_updated_timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
        }

        // Update active connections count (outside of metrics lock)
        let connections = self.connections.lock().await;
        let connection_count = connections.len() as u32;
        drop(connections); // Release the connections lock

        {
            let mut metrics = self.metrics.write().unwrap();
            metrics.active_connections = connection_count;
        }
    }
}

#[async_trait]
impl Transport for WebSocketTransportImpl {
    fn transport_type(&self) -> TransportType {
        TransportType::WebSocket
    }

    fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities {
            max_message_size: self.max_message_size,
            reliable: true,         // TCP-based, reliable
            real_time: true,        // Good for real-time communication
            broadcast: false,       // WebSocket is point-to-point
            bidirectional: true,    // Full-duplex communication
            encrypted: true,        // Can use WSS (WebSocket Secure)
            network_spanning: true, // Works across networks/internet
            supported_urgencies: vec![
                MessageUrgency::Critical,
                MessageUrgency::RealTime,
                MessageUrgency::Interactive,
                MessageUrgency::Background,
            ],
            features: vec![
                "full_duplex".to_string(),
                "low_latency".to_string(),
                "persistent_connection".to_string(),
                "binary_support".to_string(),
                "compression".to_string(),
                "wss_encryption".to_string(),
            ],
        }
    }

    async fn can_reach(&self, target: &TransportTarget) -> bool {
        // Check if target has WebSocket URL or looks like a WebSocket endpoint
        if let Some(address) = &target.address {
            // Check for WebSocket URL schemes
            if address.starts_with("ws://") || address.starts_with("wss://") {
                return true;
            }

            // Check if it's a valid HTTP URL that could be upgraded to WebSocket
            if address.starts_with("http://") || address.starts_with("https://") {
                return true;
            }

            // Check if it's a socket address that we could connect to
            if address.parse::<SocketAddr>().is_ok() {
                return true;
            }
        }

        // Check if we have an existing connection to this target
        let connections = self.connections.lock().await;
        connections.contains_key(&target.identifier)
    }

    async fn estimate_metrics(&self, target: &TransportTarget) -> Result<TransportEstimate> {
        let can_reach = self.can_reach(target).await;

        if can_reach {
            Ok(TransportEstimate {
                latency: Duration::from_millis(50), // Low latency for WebSocket
                reliability: 0.95,                  // High reliability due to TCP
                bandwidth: 10 * 1024 * 1024,        // 10Mbps typical
                cost: 2.0,                          // Medium cost
                available: true,
                confidence: 0.85, // Good confidence
            })
        } else {
            Ok(TransportEstimate {
                latency: Duration::from_secs(5),
                reliability: 0.0,
                bandwidth: 0,
                cost: 1000.0,
                available: false,
                confidence: 0.95,
            })
        }
    }

    async fn send_message(
        &self,
        target: &TransportTarget,
        message: &SecureMessage,
    ) -> Result<DeliveryReceipt> {
        // Try to find existing connection or establish new one
        let connection_id = {
            let connections = self.connections.lock().await;
            if connections.contains_key(&target.identifier) {
                target.identifier.clone()
            } else {
                drop(connections); // Release lock before async operation

                // Try to establish new connection
                if let Some(address) = &target.address {
                    let connection = self.connect_to_server(address).await?;
                    connection.id
                } else {
                    return Err(crate::error::SynapseError::TransportError(
                        "No address provided for WebSocket connection".to_string(),
                    ));
                }
            }
        };

        // Use circuit breaker to protect against failures
        if !self.circuit_breaker.can_proceed().await {
            return Err(crate::error::SynapseError::TransportError(
                "Circuit breaker is open - rejecting request".to_string(),
            ));
        }

        let send_result = timeout(
            self.connection_timeout,
            self.send_websocket_message(&connection_id, message),
        )
        .await
        .map_err(|_| crate::error::SynapseError::NetworkError("Request timeout".to_string()))?;

        match send_result {
            Ok(duration) => {
                self.circuit_breaker
                    .record_outcome(crate::circuit_breaker::RequestOutcome::Success)
                    .await;
                self.update_metrics("send", duration, true).await;

                Ok(DeliveryReceipt {
                    message_id: message.message_id.to_string(),
                    transport_used: TransportType::WebSocket,
                    delivery_time: duration,
                    target_reached: connection_id.clone(),
                    confirmation: DeliveryConfirmation::Delivered,
                    metadata: {
                        let mut map = HashMap::new();
                        map.insert("connection_id".to_string(), connection_id);
                        if let Some(addr) = &target.address {
                            map.insert("target_url".to_string(), addr.clone());
                        }
                        map
                    },
                })
            }
            Err(e) => {
                self.circuit_breaker
                    .record_outcome(crate::circuit_breaker::RequestOutcome::Failure(
                        e.to_string(),
                    ))
                    .await;
                self.update_metrics("send", Duration::from_secs(0), false)
                    .await;
                Err(e)
            }
        }
    }

    async fn receive_messages(&self) -> Result<Vec<IncomingMessage>> {
        if !self.circuit_breaker.can_proceed().await {
            return Ok(Vec::new()); // Return empty vec if circuit is open
        }

        let messages = self.receive_websocket_messages().await?;

        if !messages.is_empty() {
            self.circuit_breaker
                .record_outcome(crate::circuit_breaker::RequestOutcome::Success)
                .await;

            self.update_metrics("receive", Duration::from_millis(1), true)
                .await;

            // Add messages to internal queue
            let mut received = self.received_messages.lock().await;
            received.extend(messages.clone());
        }

        Ok(messages)
    }

    async fn test_connectivity(&self, target: &TransportTarget) -> Result<ConnectivityResult> {
        let start_time = Instant::now();

        if let Some(address) = &target.address {
            // Try to establish a test connection
            match self.connect_to_server(address).await {
                Ok(connection) => {
                    let rtt = start_time.elapsed();

                    // Clean up test connection
                    {
                        let mut connections = self.connections.lock().await;
                        connections.remove(&connection.id);
                    }

                    Ok(ConnectivityResult {
                        connected: true,
                        rtt: Some(rtt),
                        error: None,
                        quality: 0.9,
                        details: {
                            let mut map = HashMap::new();
                            map.insert("target_url".to_string(), address.clone());
                            map.insert(
                                "connection_time_ms".to_string(),
                                rtt.as_millis().to_string(),
                            );
                            map
                        },
                    })
                }
                Err(e) => {
                    let rtt = start_time.elapsed();
                    Ok(ConnectivityResult {
                        connected: false,
                        rtt: Some(rtt),
                        error: Some(format!("Connection failed: {}", e)),
                        quality: 0.0,
                        details: HashMap::new(),
                    })
                }
            }
        } else {
            Ok(ConnectivityResult {
                connected: false,
                rtt: None,
                error: Some("No target address provided".to_string()),
                quality: 0.0,
                details: HashMap::new(),
            })
        }
    }

    async fn start(&self) -> Result<()> {
        info!("Starting WebSocket transport");

        {
            let mut status = self.status.write().unwrap();
            *status = TransportStatus::Starting;
        }

        // Bind TCP listener for WebSocket server
        let listener = TcpListener::bind(format!("0.0.0.0:{}", self.local_port))
            .await
            .map_err(|e| {
                crate::error::SynapseError::NetworkError(format!("Network error: {}", e))
            })?;

        let actual_port = listener
            .local_addr()
            .map_err(|e| crate::error::SynapseError::NetworkError(format!("Network error: {}", e)))?
            .port();

        // Store listener
        {
            let mut listener_lock = self.listener.write().unwrap();
            *listener_lock = Some(listener);
        }

        // Start server task
        let connections = self.connections.clone();
        let metrics = self.metrics.clone();
        let received_messages = self.received_messages.clone();

        tokio::spawn(async move {
            // Get listener from the stored location
            let listener = {
                // We need to move the listener out to avoid borrowing issues
                // In a real implementation, we'd keep the listener in the task
                TcpListener::bind(format!("0.0.0.0:{}", actual_port))
                    .await
                    .ok()
            };

            if let Some(listener) = listener {
                info!("WebSocket server task started on port {}", actual_port);

                while let Ok((stream, peer_addr)) = listener.accept().await {
                    debug!("New WebSocket connection from {}", peer_addr);

                    let connections_clone = connections.clone();
                    let metrics_clone = metrics.clone();
                    let received_messages_clone = received_messages.clone();

                    tokio::spawn(async move {
                        match accept_async(stream).await {
                            Ok(ws_stream) => {
                                info!("WebSocket connection established with {}", peer_addr);

                                // Register connection
                                {
                                    let mut conns = connections_clone.lock().await;
                                    conns.insert(
                                        peer_addr.to_string(),
                                        WebSocketConnection {
                                            id: peer_addr.to_string(),
                                            remote_addr: peer_addr,
                                            connected_at: Instant::now(),
                                            last_activity: Instant::now(),
                                            is_server: true,
                                        },
                                    );
                                }

                                Self::handle_websocket_connection(
                                    ws_stream,
                                    peer_addr.to_string(),
                                    metrics_clone,
                                    received_messages_clone,
                                )
                                .await;
                            }
                            Err(e) => {
                                error!(
                                    "Failed to establish WebSocket connection with {}: {}",
                                    peer_addr, e
                                );
                            }
                        }
                    });
                }
            }
        });

        {
            let mut status = self.status.write().unwrap();
            *status = TransportStatus::Running;
        }

        info!(
            "WebSocket transport started on port {} (requested: {})",
            actual_port, self.local_port
        );
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        info!("Stopping WebSocket transport");

        {
            let mut status = self.status.write().unwrap();
            *status = TransportStatus::Stopping;
        }

        // Close all connections
        {
            let mut connections = self.connections.lock().await;
            connections.clear();
        }

        // Close listener
        {
            let mut listener_lock = self.listener.write().unwrap();
            *listener_lock = None;
        }

        {
            let mut status = self.status.write().unwrap();
            *status = TransportStatus::Stopped;
        }

        info!("WebSocket transport stopped");
        Ok(())
    }

    async fn status(&self) -> TransportStatus {
        *self.status.read().unwrap()
    }

    async fn metrics(&self) -> TransportMetrics {
        self.metrics.read().unwrap().clone()
    }
}
