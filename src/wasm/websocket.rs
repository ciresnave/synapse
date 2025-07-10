//! WebSocket transport for browser-based relay communication
//! 
//! This module provides WebSocket-based communication through relay servers,
//! enabling browser-based Synapse nodes to communicate when direct WebRTC
//! connections are not possible.

use wasm_bindgen::prelude::*;
use web_sys::{WebSocket, MessageEvent, CloseEvent, ErrorEvent, BinaryType};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::error::Result;
use super::BrowserPeer;

/// WebSocket transport for relay-based communication
pub struct WebSocketTransport {
    entity_id: String,
    relay_servers: Vec<String>,
    connections: Arc<Mutex<HashMap<String, WebSocketConnection>>>,
    primary_relay: Option<WebSocket>,
    message_handlers: Arc<Mutex<Vec<Box<dyn Fn(String, String) + Send + Sync>>>>,
}

/// WebSocket connection to a relay server
pub struct WebSocketConnection {
    relay_url: String,
    socket: WebSocket,
    connected_peers: Vec<String>,
    connection_state: WebSocketState,
}

/// WebSocket connection state
#[derive(Debug, Clone, PartialEq)]
pub enum WebSocketState {
    Connecting,
    Open,
    Closing,
    Closed,
}

/// WebSocket message types for Synapse protocol
#[derive(Debug, Clone)]
pub enum SynapseWebSocketMessage {
    Register {
        entity_id: String,
        capabilities: Vec<String>,
    },
    Discover {
        query: Option<String>,
    },
    DirectMessage {
        from: String,
        to: String,
        message: String,
        timestamp: f64,
    },
    PeerList {
        peers: Vec<WebSocketPeer>,
    },
    Error {
        code: u32,
        message: String,
    },
}

/// Peer information from WebSocket relay
#[derive(Debug, Clone)]
pub struct WebSocketPeer {
    pub entity_id: String,
    pub display_name: Option<String>,
    pub capabilities: Vec<String>,
    pub relay_url: String,
    pub online: bool,
    pub last_seen: f64,
}

impl WebSocketTransport {
    /// Create a new WebSocket transport
    pub async fn new(entity_id: String, relay_servers: Vec<String>) -> Result<Self> {
        web_sys::console::log_1(&"Creating WebSocket transport...".into());
        
        Ok(Self {
            entity_id,
            relay_servers,
            connections: Arc::new(Mutex::new(HashMap::new())),
            primary_relay: None,
            message_handlers: Arc::new(Mutex::new(Vec::new())),
        })
    }
    
    /// Connect to the primary relay server
    pub async fn connect(&mut self) -> Result<()> {
        if self.relay_servers.is_empty() {
            return Err(anyhow::anyhow!("No relay servers configured"));
        }
        
        let primary_url = &self.relay_servers[0];
        web_sys::console::log_1(&format!("Connecting to primary relay: {}", primary_url).into());
        
        let socket = WebSocket::new(primary_url)
            .map_err(|e| anyhow::anyhow!("Failed to create WebSocket: {:?}", e))?;
        
        // Set binary type for efficient message handling
        socket.set_binary_type(BinaryType::Arraybuffer);
        
        // Set up event handlers
        self.setup_websocket_handlers(&socket).await?;
        
        self.primary_relay = Some(socket);
        
        Ok(())
    }
    
    /// Send a message via WebSocket relay
    pub async fn send_message(&self, target: &str, message: &str) -> Result<String> {
        if let Some(ref socket) = self.primary_relay {
            if socket.ready_state() == WebSocket::OPEN {
                let msg = SynapseWebSocketMessage::DirectMessage {
                    from: self.entity_id.clone(),
                    to: target.to_string(),
                    message: message.to_string(),
                    timestamp: js_sys::Date::now(),
                };
                
                let json_msg = self.serialize_message(&msg)?;
                
                socket.send_with_str(&json_msg)
                    .map_err(|e| anyhow::anyhow!("Failed to send WebSocket message: {:?}", e))?;
                
                web_sys::console::log_1(&format!("Sent message via WebSocket to {}", target).into());
                return Ok(format!("websocket://{}@{}", target, self.entity_id));
            }
        }
        
        Err(anyhow::anyhow!("No open WebSocket connection available"))
    }
    
    /// Discover peers via WebSocket relay
    pub async fn discover_peers(&self) -> Result<Vec<BrowserPeer>> {
        if let Some(ref socket) = self.primary_relay {
            if socket.ready_state() == WebSocket::OPEN {
                let discover_msg = SynapseWebSocketMessage::Discover { query: None };
                let json_msg = self.serialize_message(&discover_msg)?;
                
                socket.send_with_str(&json_msg)
                    .map_err(|e| anyhow::anyhow!("Failed to send discovery message: {:?}", e))?;
                
                web_sys::console::log_1(&"Sent peer discovery request".into());
                
                // In a real implementation, this would wait for the response
                // For now, return empty list
                return Ok(Vec::new());
            }
        }
        
        Err(anyhow::anyhow!("No WebSocket connection for discovery"))
    }
    
    /// Register with the relay server
    pub async fn register(&self) -> Result<()> {
        if let Some(ref socket) = self.primary_relay {
            let register_msg = SynapseWebSocketMessage::Register {
                entity_id: self.entity_id.clone(),
                capabilities: vec![
                    "websocket".to_string(),
                    "browser".to_string(),
                    "synapse-v1".to_string(),
                ],
            };
            
            let json_msg = self.serialize_message(&register_msg)?;
            
            socket.send_with_str(&json_msg)
                .map_err(|e| anyhow::anyhow!("Failed to register with relay: {:?}", e))?;
            
            web_sys::console::log_1(&"Registered with WebSocket relay".into());
        }
        
        Ok(())
    }
    
    /// Add a message handler
    pub fn add_message_handler<F>(&self, handler: F)
    where
        F: Fn(String, String) + Send + Sync + 'static,
    {
        let mut handlers = self.message_handlers.lock().unwrap();
        handlers.push(Box::new(handler));
    }
    
    /// Disconnect from relay servers
    pub async fn disconnect(&mut self) -> Result<()> {
        if let Some(ref socket) = self.primary_relay {
            socket.close()
                .map_err(|e| anyhow::anyhow!("Failed to close WebSocket: {:?}", e))?;
        }
        
        self.primary_relay = None;
        
        let mut connections = self.connections.lock().unwrap();
        connections.clear();
        
        web_sys::console::log_1(&"Disconnected from WebSocket relays".into());
        Ok(())
    }
    
    /// Get connection status
    pub fn is_connected(&self) -> bool {
        if let Some(ref socket) = self.primary_relay {
            socket.ready_state() == WebSocket::OPEN
        } else {
            false
        }
    }
    
    // Private helper methods
    
    async fn setup_websocket_handlers(&self, socket: &WebSocket) -> Result<()> {
        let entity_id = self.entity_id.clone();
        let message_handlers = Arc::clone(&self.message_handlers);
        
        // Message handler
        let onmessage_callback = Closure::wrap(Box::new(move |event: MessageEvent| {
            if let Ok(message) = event.data().dyn_into::<js_sys::JsString>() {
                let message_str = String::from(message);
                web_sys::console::log_1(&format!("WebSocket message received: {}", message_str).into());
                
                // Parse and handle the message
                if let Ok(parsed_msg) = Self::parse_message(&message_str) {
                    Self::handle_parsed_message(parsed_msg, &message_handlers);
                }
            }
        }) as Box<dyn FnMut(_)>);
        
        socket.set_onmessage(Some(onmessage_callback.as_ref().unchecked_ref()));
        onmessage_callback.forget();
        
        // Open handler
        let entity_id_clone = entity_id.clone();
        let onopen_callback = Closure::wrap(Box::new(move |_event: Event| {
            web_sys::console::log_1(&format!("WebSocket connected for {}", entity_id_clone).into());
        }) as Box<dyn FnMut(_)>);
        
        socket.set_onopen(Some(onopen_callback.as_ref().unchecked_ref()));
        onopen_callback.forget();
        
        // Close handler
        let onclose_callback = Closure::wrap(Box::new(move |event: CloseEvent| {
            web_sys::console::log_1(&format!("WebSocket closed: {} - {}", event.code(), event.reason()).into());
        }) as Box<dyn FnMut(_)>);
        
        socket.set_onclose(Some(onclose_callback.as_ref().unchecked_ref()));
        onclose_callback.forget();
        
        // Error handler
        let onerror_callback = Closure::wrap(Box::new(move |event: ErrorEvent| {
            web_sys::console::error_1(&format!("WebSocket error: {:?}", event).into());
        }) as Box<dyn FnMut(_)>);
        
        socket.set_onerror(Some(onerror_callback.as_ref().unchecked_ref()));
        onerror_callback.forget();
        
        Ok(())
    }
    
    fn serialize_message(&self, message: &SynapseWebSocketMessage) -> Result<String> {
        // Convert message to JSON
        // In a real implementation, this would use serde_json
        match message {
            SynapseWebSocketMessage::Register { entity_id, capabilities } => {
                Ok(format!(
                    r#"{{"type":"register","entity_id":"{}","capabilities":{:?}}}"#,
                    entity_id, capabilities
                ))
            }
            SynapseWebSocketMessage::DirectMessage { from, to, message, timestamp } => {
                Ok(format!(
                    r#"{{"type":"message","from":"{}","to":"{}","message":"{}","timestamp":{}}}"#,
                    from, to, message, timestamp
                ))
            }
            SynapseWebSocketMessage::Discover { query: _ } => {
                Ok(r#"{"type":"discover"}"#.to_string())
            }
            _ => Err(anyhow::anyhow!("Unsupported message type for serialization")),
        }
    }
    
    fn parse_message(json_str: &str) -> Result<SynapseWebSocketMessage> {
        // Parse JSON message
        // In a real implementation, this would use serde_json
        
        if json_str.contains(r#""type":"message""#) {
            // Simple regex-like parsing for demo
            return Ok(SynapseWebSocketMessage::DirectMessage {
                from: "unknown".to_string(),
                to: "unknown".to_string(),
                message: json_str.to_string(),
                timestamp: js_sys::Date::now(),
            });
        }
        
        Err(anyhow::anyhow!("Failed to parse WebSocket message"))
    }
    
    fn handle_parsed_message(
        message: SynapseWebSocketMessage,
        handlers: &Arc<Mutex<Vec<Box<dyn Fn(String, String) + Send + Sync>>>>,
    ) {
        match message {
            SynapseWebSocketMessage::DirectMessage { from, to: _, message, timestamp: _ } => {
                let handlers_guard = handlers.lock().unwrap();
                for handler in handlers_guard.iter() {
                    handler(from.clone(), message.clone());
                }
            }
            SynapseWebSocketMessage::PeerList { peers } => {
                web_sys::console::log_1(&format!("Received peer list with {} peers", peers.len()).into());
            }
            SynapseWebSocketMessage::Error { code, message } => {
                web_sys::console::error_1(&format!("WebSocket error {}: {}", code, message).into());
            }
            _ => {
                web_sys::console::log_1(&"Received other message type".into());
            }
        }
    }
}
