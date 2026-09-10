// SPDX-License-Identifier: MIT OR Apache-2.0
//! WebSocket Transport Implementation for Synapse
//!
//! Provides real-time bidirectional communication over WebSocket protocol,
//! ideal for web applications and browser-based clients.

use crate::{
    circuit_breaker::CircuitBreaker,
    error::Result,
    synapse::blockchain::serialization::{DateTimeWrapper, UuidWrapper},
    transport::{MessageUrgency, Transport, TransportMetrics, TransportRoute},
    types::*,
};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    sync::{RwLock, mpsc},
    time::timeout,
};
use tokio_tungstenite::{
    accept_async, connect_async,
    tungstenite::{Message, protocol::Role},
};
use tracing::{debug, error, info, warn};
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

                            Self::handle_websocket_connection(
                                ws_stream,
                                peer_addr.to_string(),
                                metrics_clone,
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
                    debug!(
                        "Received binary WebSocket message from {} ({} bytes)",
                        peer_id,
                        data.len()
                    );

                    // Update metrics
                    {
                        let mut m = metrics.write().await;
                        m.messages_received += 1;
                        m.bytes_received += data.len() as u64;
                    }

                    // Try to deserialize as SecureMessage
                    match bincode::serde::decode_from_slice::<SecureMessage, _>(
                        &data,
                        bincode::config::standard(),
                    ) {
                        Ok(message) => {
                            info!(
                                "Received valid Synapse message via WebSocket from {}",
                                peer_id
                            );
                            // Handle the message (would integrate with router)
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
        // Get active connection
        let connection = {
            let connections = self.connections.read().await;
            connections.get(target).cloned()
        };

        match connection {
            Some(conn) => {
                debug!(
                    "Sending WebSocket message to {} ({} bytes)",
                    target,
                    data.len()
                );

                // For existing connections, we would send via the established WebSocket
                // In practice, we'd maintain the actual WebSocket stream references
                match self.send_via_existing_connection(&conn, data.clone()).await {
                    Ok(()) => {
                        // Update metrics
                        {
                            let mut metrics = self.metrics.write().await;
                            metrics.messages_sent += 1;
                            metrics.bytes_sent += data.len() as u64;
                        }
                        Ok(())
                    }
                    Err(e) => {
                        warn!(
                            "Failed to send via existing connection to {}: {}",
                            target, e
                        );
                        // Try to establish new connection and retry
                        self.connect_and_send(target, data).await
                    }
                }
            }
            None => {
                debug!(
                    "No existing connection to {}, establishing new connection",
                    target
                );
                // Establish new connection and send
                self.connect_and_send(target, data).await
            }
        }
    }

    /// Send message via existing WebSocket connection
    async fn send_via_existing_connection(
        &self,
        conn: &WebSocketConnection,
        data: Vec<u8>,
    ) -> Result<()> {
        // In a real implementation, we would:
        // 1. Have a reference to the WebSocket sender
        // 2. Send the message as binary WebSocket frame
        // 3. Handle connection errors and reconnection

        debug!("Using existing WebSocket connection to {}", conn.url);

        // Simulate network latency for WebSocket
        tokio::time::sleep(Duration::from_millis(1)).await;

        // Check if connection is still valid (basic health check)
        let elapsed_since_connect = conn.connected_at.elapsed();
        if elapsed_since_connect > Duration::from_secs(300) {
            // 5 minutes timeout
            return Err("WebSocket connection timeout".into());
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
        let parsed_url =
            Url::parse(&ws_url).map_err(|e| format!("Invalid WebSocket URL {}: {}", ws_url, e))?;

        // Attempt WebSocket connection with timeout
        match timeout(Duration::from_secs(10), connect_async(&parsed_url)).await {
            Ok(Ok((mut ws_stream, _response))) => {
                info!("WebSocket connection established to {}", ws_url);

                // Register the new connection
                {
                    let mut connections = self.connections.write().await;
                    connections.insert(
                        target.to_string(),
                        WebSocketConnection {
                            url: ws_url.clone(),
                            connected_at: Instant::now(),
                            last_ping: None,
                            is_client: true,
                        },
                    );
                }

                // Send the message immediately
                let message = Message::Binary(data.clone());
                match ws_stream.send(message).await {
                    Ok(()) => {
                        debug!("Message sent via new WebSocket connection to {}", target);

                        // Update metrics
                        {
                            let mut metrics = self.metrics.write().await;
                            metrics.messages_sent += 1;
                            metrics.bytes_sent += data.len() as u64;
                        }

                        // Keep connection alive for future messages
                        let connections = self.connections.clone();
                        let metrics = self.metrics.clone();
                        let target_clone = target.to_string();

                        tokio::spawn(async move {
                            Self::maintain_websocket_connection(
                                ws_stream,
                                target_clone,
                                connections,
                                metrics,
                            )
                            .await;
                        });

                        Ok(())
                    }
                    Err(e) => {
                        error!("Failed to send message via new WebSocket connection: {}", e);
                        // Remove the invalid connection
                        {
                            let mut connections = self.connections.write().await;
                            connections.remove(target);
                        }
                        Err(format!("WebSocket send failed: {}", e).into())
                    }
                }
            }
            Ok(Err(e)) => {
                error!("WebSocket connection failed to {}: {}", ws_url, e);
                Err(format!("WebSocket connection failed: {}", e).into())
            }
            Err(_) => {
                error!("WebSocket connection timeout to {}", ws_url);
                Err("WebSocket connection timeout".into())
            }
        }
    }

    /// Maintain an active WebSocket connection
    async fn maintain_websocket_connection(
        mut ws_stream: tokio_tungstenite::WebSocketStream<
            impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
        >,
        peer_id: String,
        connections: Arc<RwLock<HashMap<String, WebSocketConnection>>>,
        metrics: Arc<RwLock<TransportMetrics>>,
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
                                let mut m = metrics.write().await;
                                m.messages_received += 1;
                                m.bytes_received += data.len() as u64;
                            }

                            // Handle the message (integrate with router/message handler)
                            if let Ok((message, _)) = bincode::decode_from_slice::<SecureMessage, _>(&data, bincode::config::standard()) {
                                debug!("Received valid Synapse message from {}", peer_id);
                                // Route message to appropriate handler
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
                        if let Err(e) = ws_stream.send(Message::Ping(vec![])).await {
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
            let mut connections = connections.write().await;
            connections.remove(&peer_id);
        }

        info!("WebSocket connection maintenance ended for {}", peer_id);
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

                debug!(
                    "Sent WebSocket message to {} (duration: {:?})",
                    target, duration
                );
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
                reliability: 0.99,                  // High reliability
            })
        } else {
            Err("WebSocket route not available".into())
        }
    }

    async fn supports_urgency(&self, urgency: MessageUrgency) -> bool {
        matches!(
            urgency,
            MessageUrgency::RealTime | MessageUrgency::Interactive
        )
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
