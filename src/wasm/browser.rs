//! Browser-specific Synapse implementation for WebAssembly
//! 
//! This module provides a full-featured Synapse node that runs in web browsers,
//! with WebRTC for peer-to-peer communication and WebSocket fallbacks.

use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::{console, window, BroadcastChannel, MessageEvent};
use gloo::storage::{LocalStorage, Storage as _};
use gloo::worker::{Worker, WorkerBridge};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use js_sys::{Array, Promise, Uint8Array};

use crate::{
    types::*,
    error::Result,
, synapse::blockchain::serialization::{DateTimeWrapper, UuidWrapper}},;

use super::{
    webrtc::WebRtcTransport,
    websocket::WebSocketTransport,
    storage::BrowserStorage,
    worker::SynapseWorker,
    crypto::WebCrypto,
};

/// Browser-based Synapse node with full WebAssembly support
#[wasm_bindgen]
pub struct BrowserSynapseNode {
    /// Our entity ID in the Synapse network
    entity_id: String,
    
    /// WebRTC transport for direct peer-to-peer communication
    webrtc_transport: Option<WebRtcTransport>,
    
    /// WebSocket transport for relay communication
    websocket_transport: Option<WebSocketTransport>,
    
    /// Browser storage for persistence
    storage: BrowserStorage,
    
    /// Discovered peers
    peers: Arc<Mutex<HashMap<String, BrowserPeer>>>,
    
    /// Active connections
    connections: Arc<Mutex<HashMap<String, BrowserConnection>>>,
    
    /// Crypto manager
    crypto: WebCrypto,
    
    /// Configuration
    config: BrowserConfig,
    
    /// Worker for background tasks
    worker: Option<WorkerBridge<SynapseWorker>>,
    
    /// Broadcast channel for inter-tab communication
    broadcast_channel: Option<BroadcastChannel>,
}

/// Browser-specific peer information
#[derive(Debug, Clone)]
pub struct BrowserPeer {
    pub entity_id: String,
    pub display_name: String,
    pub capabilities: Vec<String>,
    pub webrtc_supported: bool,
    pub websocket_endpoints: Vec<String>,
    pub ice_servers: Vec<IceServer>,
    pub last_seen: f64, // JavaScript timestamp
    pub tab_id: Option<String>, // For same-browser communication
}

/// Browser connection wrapper
#[derive(Debug)]
pub enum BrowserConnection {
    WebRtc {
        peer_id: String,
        data_channel: web_sys::RtcDataChannel,
        connection: web_sys::RtcPeerConnection,
    },
    WebSocket {
        peer_id: String,
        socket: web_sys::WebSocket,
        relay_server: String,
    },
    BroadcastChannel {
        peer_id: String,
        channel: BroadcastChannel,
    },
}

/// ICE server configuration for WebRTC
#[derive(Debug, Clone)]
pub struct IceServer {
    pub urls: Vec<String>,
    pub username: Option<String>,
    pub credential: Option<String>,
}

/// Browser-specific configuration
#[derive(Debug, Clone)]
pub struct BrowserConfig {
    pub ice_servers: Vec<IceServer>,
    pub websocket_relays: Vec<String>,
    pub enable_webrtc: bool,
    pub enable_websocket: bool,
    pub enable_broadcast_channel: bool,
    pub storage_key_prefix: String,
    pub max_connections: usize,
    pub connection_timeout_ms: u32,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            ice_servers: vec![
                IceServer {
                    urls: vec!["stun:stun.l.google.com:19302".to_string()],
                    username: None,
                    credential: None,
                },
                IceServer {
                    urls: vec!["stun:stun1.l.google.com:19302".to_string()],
                    username: None,
                    credential: None,
                },
            ],
            websocket_relays: vec![
                "wss://relay.synapse-network.org/ws".to_string(),
                "wss://backup-relay.synapse-network.org/ws".to_string(),
            ],
            enable_webrtc: true,
            enable_websocket: true,
            enable_broadcast_channel: true,
            storage_key_prefix: "synapse_".to_string(),
            max_connections: 50,
            connection_timeout_ms: 30000,
        }
    }
}

#[wasm_bindgen]
impl BrowserSynapseNode {
    /// Create a new browser Synapse node
    #[wasm_bindgen(constructor)]
    pub fn new(entity_id: String) -> BrowserSynapseNode {
        console_error_panic_hook::set_once();
        
        let config = BrowserConfig::default();
        let storage = BrowserStorage::new(&config.storage_key_prefix);
        let crypto = WebCrypto::new();
        
        console::log_1(&"Creating Synapse node in browser...".into());
        
        Self {
            entity_id,
            webrtc_transport: None,
            websocket_transport: None,
            storage,
            peers: Arc::new(Mutex::new(HashMap::new())),
            connections: Arc::new(Mutex::new(HashMap::new())),
            crypto,
            config,
            worker: None,
            broadcast_channel: None,
        }
    }
    
    /// Initialize the Synapse node with full browser capabilities
    #[wasm_bindgen]
    pub async fn initialize(&mut self) -> Result<(), JsValue> {
        console::log_1(&"Initializing Synapse browser node...".into());
        
        // Initialize WebRTC transport
        if self.config.enable_webrtc {
            match WebRtcTransport::new(self.entity_id.clone(), self.config.ice_servers.clone()).await {
                Ok(transport) => {
                    console::log_1(&"WebRTC transport initialized".into());
                    self.webrtc_transport = Some(transport);
                }
                Err(e) => {
                    console::warn_1(&format!("Failed to initialize WebRTC: {:?}", e).into());
                }
            }
        }
        
        // Initialize WebSocket transport
        if self.config.enable_websocket {
            match WebSocketTransport::new(self.entity_id.clone(), self.config.websocket_relays.clone()).await {
                Ok(transport) => {
                    console::log_1(&"WebSocket transport initialized".into());
                    self.websocket_transport = Some(transport);
                }
                Err(e) => {
                    console::warn_1(&format!("Failed to initialize WebSocket: {:?}", e).into());
                }
            }
        }
        
        // Initialize broadcast channel for same-browser communication
        if self.config.enable_broadcast_channel {
            match BroadcastChannel::new("synapse") {
                Ok(channel) => {
                    self.setup_broadcast_channel(&channel)?;
                    self.broadcast_channel = Some(channel);
                    console::log_1(&"Broadcast channel initialized".into());
                }
                Err(e) => {
                    console::warn_1(&format!("Failed to initialize broadcast channel: {:?}", e).into());
                }
            }
        }
        
        // Start background worker
        self.start_worker().await?;
        
        // Load persisted data
        self.load_persisted_data().await?;
        
        console::log_1(&"Synapse browser node fully initialized".into());
        Ok(())
    }
    
    /// Send a message to another entity
    #[wasm_bindgen]
    pub async fn send_message(&self, target: String, message: String) -> Result<String, JsValue> {
        console::log_2(&"Sending message to".into(), &target.into());
        
        // Find the best connection method for the target
        if let Some(peer) = self.find_peer(&target).await {
            // Try WebRTC first (fastest)
            if peer.webrtc_supported {
                if let Some(webrtc) = &self.webrtc_transport {
                    match webrtc.send_message(&target, &message).await {
                        Ok(result) => return Ok(result),
                        Err(e) => console::warn_1(&format!("WebRTC send failed: {:?}", e).into()),
                    }
                }
            }
            
            // Try WebSocket relay
            if !peer.websocket_endpoints.is_empty() {
                if let Some(websocket) = &self.websocket_transport {
                    match websocket.send_message(&target, &message).await {
                        Ok(result) => return Ok(result),
                        Err(e) => console::warn_1(&format!("WebSocket send failed: {:?}", e).into()),
                    }
                }
            }
            
            // Try broadcast channel (same browser)
            if peer.tab_id.is_some() {
                if let Some(broadcast) = &self.broadcast_channel {
                    match self.send_via_broadcast_channel(broadcast, &target, &message).await {
                        Ok(result) => return Ok(result),
                        Err(e) => console::warn_1(&format!("Broadcast channel send failed: {:?}", e).into()),
                    }
                }
            }
        }
        
        Err(JsValue::from_str(&format!("Failed to send message to {}", target)))
    }
    
    /// Discover peers on the network
    #[wasm_bindgen]
    pub async fn discover_peers(&self) -> Result<Array, JsValue> {
        console::log_1(&"Discovering peers...".into());
        
        let mut discovered = Vec::new();
        
        // Discover via WebRTC signaling
        if let Some(webrtc) = &self.webrtc_transport {
            if let Ok(peers) = webrtc.discover_peers().await {
                discovered.extend(peers);
            }
        }
        
        // Discover via WebSocket relay
        if let Some(websocket) = &self.websocket_transport {
            if let Ok(peers) = websocket.discover_peers().await {
                discovered.extend(peers);
            }
        }
        
        // Discover via broadcast channel (same browser)
        if let Some(broadcast) = &self.broadcast_channel {
            if let Ok(peers) = self.discover_via_broadcast_channel(broadcast).await {
                discovered.extend(peers);
            }
        }
        
        // Convert to JavaScript array
        let js_array = Array::new();
        for peer in discovered {
            let peer_obj = js_sys::Object::new();
            js_sys::Reflect::set(&peer_obj, &"entity_id".into(), &peer.entity_id.into())?;
            js_sys::Reflect::set(&peer_obj, &"display_name".into(), &peer.display_name.into())?;
            js_array.push(&peer_obj);
        }
        
        Ok(js_array)
    }
    
    /// Get connection status
    #[wasm_bindgen]
    pub fn get_status(&self) -> js_sys::Object {
        let status = js_sys::Object::new();
        
        let connections = self.connections.lock().unwrap();
        js_sys::Reflect::set(&status, &"active_connections".into(), &(connections.len() as u32).into()).unwrap();
        
        let webrtc_available = self.webrtc_transport.is_some();
        js_sys::Reflect::set(&status, &"webrtc_available".into(), &webrtc_available.into()).unwrap();
        
        let websocket_available = self.websocket_transport.is_some();
        js_sys::Reflect::set(&status, &"websocket_available".into(), &websocket_available.into()).unwrap();
        
        let broadcast_available = self.broadcast_channel.is_some();
        js_sys::Reflect::set(&status, &"broadcast_available".into(), &broadcast_available.into()).unwrap();
        
        status
    }
    
    /// Set message handler
    #[wasm_bindgen]
    pub fn set_message_handler(&self, handler: &js_sys::Function) {
        // Store the handler for incoming messages
        console::log_1(&"Message handler set".into());
        // Implementation would store the handler and call it when messages arrive
    }
    
    // Private implementation methods
    
    async fn find_peer(&self, entity_id: &str) -> Option<BrowserPeer> {
        let peers = self.peers.lock().unwrap();
        peers.get(entity_id).cloned()
    }
    
    fn setup_broadcast_channel(&self, channel: &BroadcastChannel) -> Result<(), JsValue> {
        let entity_id = self.entity_id.clone();
        let peers = Arc::clone(&self.peers);
        
        let onmessage_callback = Closure::wrap(Box::new(move |event: MessageEvent| {
            // Handle incoming broadcast messages
            console::log_2(&"Received broadcast message from".into(), &event.data());
        }) as Box<dyn FnMut(_)>);
        
        channel.set_onmessage(Some(onmessage_callback.as_ref().unchecked_ref()));
        onmessage_callback.forget();
        
        Ok(())
    }
    
    async fn send_via_broadcast_channel(
        &self,
        channel: &BroadcastChannel,
        target: &str,
        message: &str,
    ) -> Result<String, JsValue> {
        let msg = js_sys::Object::new();
        js_sys::Reflect::set(&msg, &"type".into(), &"synapse_message".into())?;
        js_sys::Reflect::set(&msg, &"from".into(), &self.entity_id.clone().into())?;
        js_sys::Reflect::set(&msg, &"to".into(), &target.into())?;
        js_sys::Reflect::set(&msg, &"message".into(), &message.into())?;
        js_sys::Reflect::set(&msg, &"timestamp".into(), &js_sys::Date::now().into())?;
        
        channel.post_message(&msg)?;
        Ok(format!("broadcast://{}@{}", target, self.entity_id))
    }
    
    async fn discover_via_broadcast_channel(&self, channel: &BroadcastChannel) -> Result<Vec<BrowserPeer>, JsValue> {
        // Send discovery announcement
        let discovery_msg = js_sys::Object::new();
        js_sys::Reflect::set(&discovery_msg, &"type".into(), &"synapse_discovery".into())?;
        js_sys::Reflect::set(&discovery_msg, &"from".into(), &self.entity_id.clone().into())?;
        js_sys::Reflect::set(&discovery_msg, &"timestamp".into(), &js_sys::Date::now().into())?;
        
        channel.post_message(&discovery_msg)?;
        
        // Return empty for now - responses would be handled in the message callback
        Ok(Vec::new())
    }
    
    async fn start_worker(&mut self) -> Result<(), JsValue> {
        // Start a web worker for background tasks
        console::log_1(&"Starting Synapse worker...".into());
        
        // For now, just log that the worker would be started
        // In a full implementation, this would spawn a web worker
        Ok(())
    }
    
    async fn load_persisted_data(&self) -> Result<(), JsValue> {
        // Load saved peers and configuration from localStorage
        if let Ok(peers_data) = LocalStorage::get::<String>(&format!("{}peers", self.config.storage_key_prefix)) {
            console::log_1(&"Loaded persisted peer data".into());
            // Parse and restore peers
        }
        
        Ok(())
    }
}

/// JavaScript interop utilities
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
    
    #[wasm_bindgen(js_namespace = console)]
    fn warn(s: &str);
    
    #[wasm_bindgen(js_namespace = console)]
    fn error(s: &str);
}

/// Console panic hook for better debugging
#[wasm_bindgen]
pub fn init_panic_hook() {
    console_error_panic_hook::set_once();
}

/// Get WebAssembly memory usage statistics
#[wasm_bindgen]
pub fn get_memory_usage() -> js_sys::Object {
    let usage = js_sys::Object::new();
    
    if let Some(memory) = wasm_bindgen::memory() {
        let buffer = memory.buffer();
        let size = buffer.byte_length();
        js_sys::Reflect::set(&usage, &"total_bytes".into(), &size.into()).unwrap();
    }
    
    usage
}

/// Initialize WebAssembly allocator
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;
