//! WebRTC transport implementation for browser-to-browser communication
//! 
//! This module provides direct peer-to-peer communication using WebRTC
//! data channels, enabling real-time communication between browser-based
//! Synapse nodes.

use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    RtcPeerConnection, RtcDataChannel, RtcConfiguration, RtcIceServer,
    RtcDataChannelInit, RtcPeerConnectionIceEvent, RtcSessionDescriptionInit,
    RtcSdpType, MessageEvent, Event,
};
use js_sys::{Object, Reflect, Array, Promise};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::error::Result;
use super::BrowserPeer;

/// WebRTC transport for direct peer-to-peer communication
pub struct WebRtcTransport {
    entity_id: String,
    peer_connections: Arc<Mutex<HashMap<String, WebRtcConnection>>>,
    ice_servers: Vec<super::IceServer>,
    signaling_server: Option<String>,
    data_channel_config: RtcDataChannelInit,
}

/// WebRTC connection wrapper
pub struct WebRtcConnection {
    peer_id: String,
    peer_connection: RtcPeerConnection,
    data_channel: Option<RtcDataChannel>,
    connection_state: WebRtcConnectionState,
    ice_candidates: Vec<RtcIceCandidate>,
}

/// ICE candidate for WebRTC negotiation
#[derive(Debug, Clone)]
pub struct RtcIceCandidate {
    pub candidate: String,
    pub sdp_mid: Option<String>,
    pub sdp_m_line_index: Option<u16>,
}

/// WebRTC connection state
#[derive(Debug, Clone, PartialEq)]
pub enum WebRtcConnectionState {
    New,
    Connecting,
    Connected,
    Disconnected,
    Failed,
    Closed,
}

impl WebRtcTransport {
    /// Create a new WebRTC transport
    pub async fn new(entity_id: String, ice_servers: Vec<super::IceServer>) -> Result<Self> {
        // Create data channel configuration
        let mut data_channel_config = RtcDataChannelInit::new();
        data_channel_config.ordered(false); // Allow out-of-order delivery for speed
        data_channel_config.max_retransmits(3); // Limit retransmissions
        
        Ok(Self {
            entity_id,
            peer_connections: Arc::new(Mutex::new(HashMap::new())),
            ice_servers,
            signaling_server: None,
            data_channel_config,
        })
    }
    
    /// Set signaling server for WebRTC negotiation
    pub fn set_signaling_server(&mut self, server: String) {
        self.signaling_server = Some(server);
    }
    
    /// Create a WebRTC connection to a peer
    pub async fn connect_to_peer(&self, peer_id: String) -> Result<()> {
        web_sys::console::log_1(&format!("Connecting to peer via WebRTC: {}", peer_id).into());
        
        // Create RTC configuration
        let rtc_config = self.create_rtc_configuration()?;
        
        // Create peer connection
        let peer_connection = RtcPeerConnection::new_with_configuration(&rtc_config)
            .map_err(|e| anyhow::anyhow!("Failed to create peer connection: {:?}", e))?;
        
        // Create data channel
        let data_channel = peer_connection.create_data_channel_with_data_channel_dict(
            "synapse",
            &self.data_channel_config,
        );
        
        // Set up event handlers
        self.setup_peer_connection_handlers(&peer_connection, &peer_id).await?;
        self.setup_data_channel_handlers(&data_channel, &peer_id).await?;
        
        // Store connection
        let connection = WebRtcConnection {
            peer_id: peer_id.clone(),
            peer_connection,
            data_channel: Some(data_channel),
            connection_state: WebRtcConnectionState::New,
            ice_candidates: Vec::new(),
        };
        
        {
            let mut connections = self.peer_connections.lock().unwrap();
            connections.insert(peer_id, connection);
        }
        
        Ok(())
    }
    
    /// Send a message via WebRTC data channel
    pub async fn send_message(&self, peer_id: &str, message: &str) -> Result<String> {
        let connections = self.peer_connections.lock().unwrap();
        
        if let Some(connection) = connections.get(peer_id) {
            if let Some(ref data_channel) = connection.data_channel {
                // Check if data channel is open
                if data_channel.ready_state() == web_sys::RtcDataChannelState::Open {
                    data_channel.send_with_str(message)
                        .map_err(|e| anyhow::anyhow!("Failed to send via WebRTC: {:?}", e))?;
                    
                    web_sys::console::log_1(&format!("Sent message via WebRTC to {}", peer_id).into());
                    return Ok(format!("webrtc://{}@{}", peer_id, self.entity_id));
                }
            }
        }
        
        Err(anyhow::anyhow!("No open WebRTC connection to {}", peer_id))
    }
    
    /// Discover peers via WebRTC signaling
    pub async fn discover_peers(&self) -> Result<Vec<BrowserPeer>> {
        // This would typically involve communicating with a signaling server
        // to discover other WebRTC-capable peers
        
        web_sys::console::log_1(&"Discovering WebRTC peers...".into());
        
        // For now, return empty list
        // In a full implementation, this would:
        // 1. Connect to signaling server
        // 2. Query for available peers
        // 3. Exchange ICE candidates
        // 4. Return list of connectable peers
        
        Ok(Vec::new())
    }
    
    /// Handle incoming WebRTC connection offer
    pub async fn handle_offer(&self, peer_id: String, offer_sdp: String) -> Result<String> {
        web_sys::console::log_1(&format!("Handling WebRTC offer from {}", peer_id).into());
        
        // Create RTC configuration
        let rtc_config = self.create_rtc_configuration()?;
        
        // Create peer connection
        let peer_connection = RtcPeerConnection::new_with_configuration(&rtc_config)
            .map_err(|e| anyhow::anyhow!("Failed to create peer connection: {:?}", e))?;
        
        // Set up event handlers
        self.setup_peer_connection_handlers(&peer_connection, &peer_id).await?;
        
        // Set remote description (offer)
        let mut offer_desc = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
        offer_desc.sdp(&offer_sdp);
        
        let set_remote_promise = peer_connection.set_remote_description(&offer_desc);
        JsFuture::from(set_remote_promise).await
            .map_err(|e| anyhow::anyhow!("Failed to set remote description: {:?}", e))?;
        
        // Create answer
        let answer_promise = peer_connection.create_answer();
        let answer = JsFuture::from(answer_promise).await
            .map_err(|e| anyhow::anyhow!("Failed to create answer: {:?}", e))?;
        
        // Set local description (answer)
        let set_local_promise = peer_connection.set_local_description(&answer.into());
        JsFuture::from(set_local_promise).await
            .map_err(|e| anyhow::anyhow!("Failed to set local description: {:?}", e))?;
        
        // Get answer SDP
        let answer_sdp = peer_connection.local_description()
            .ok_or_else(|| anyhow::anyhow!("No local description available"))?
            .sdp();
        
        // Store connection
        let connection = WebRtcConnection {
            peer_id: peer_id.clone(),
            peer_connection,
            data_channel: None, // Will be created by remote peer
            connection_state: WebRtcConnectionState::Connecting,
            ice_candidates: Vec::new(),
        };
        
        {
            let mut connections = self.peer_connections.lock().unwrap();
            connections.insert(peer_id, connection);
        }
        
        Ok(answer_sdp)
    }
    
    /// Handle incoming ICE candidate
    pub async fn handle_ice_candidate(&self, peer_id: &str, candidate: RtcIceCandidate) -> Result<()> {
        let connections = self.peer_connections.lock().unwrap();
        
        if let Some(connection) = connections.get(peer_id) {
            // Add ICE candidate to peer connection
            let ice_candidate_init = web_sys::RtcIceCandidateInit::new(&candidate.candidate);
            if let Some(ref sdp_mid) = candidate.sdp_mid {
                ice_candidate_init.set_sdp_mid(Some(sdp_mid));
            }
            if let Some(sdp_m_line_index) = candidate.sdp_m_line_index {
                ice_candidate_init.set_sdp_m_line_index(Some(sdp_m_line_index));
            }
            
            let ice_candidate = web_sys::RtcIceCandidate::new(&ice_candidate_init)
                .map_err(|e| anyhow::anyhow!("Failed to create ICE candidate: {:?}", e))?;
            
            let add_candidate_promise = connection.peer_connection.add_ice_candidate_with_opt_rtc_ice_candidate(Some(&ice_candidate));
            JsFuture::from(add_candidate_promise).await
                .map_err(|e| anyhow::anyhow!("Failed to add ICE candidate: {:?}", e))?;
            
            web_sys::console::log_1(&format!("Added ICE candidate for {}", peer_id).into());
        }
        
        Ok(())
    }
    
    /// Close connection to a peer
    pub async fn disconnect_from_peer(&self, peer_id: &str) -> Result<()> {
        let mut connections = self.peer_connections.lock().unwrap();
        
        if let Some(connection) = connections.remove(peer_id) {
            // Close data channel
            if let Some(ref data_channel) = connection.data_channel {
                data_channel.close();
            }
            
            // Close peer connection
            connection.peer_connection.close();
            
            web_sys::console::log_1(&format!("Disconnected from WebRTC peer {}", peer_id).into());
        }
        
        Ok(())
    }
    
    // Private helper methods
    
    /// Set up event handlers for peer connections
    async fn setup_peer_connection_handlers(&self, peer_connection: &RtcPeerConnection, peer_id: &str) -> Result<()> {
        let pc_clone = peer_connection.clone();
        let peer_id_clone = peer_id.to_string();
        
        // Handle ICE candidates
        let on_ice_candidate = Closure::wrap(Box::new(move |event: RtcPeerConnectionIceEvent| {
            let candidate = event.candidate();
            if let Some(candidate) = candidate {
                web_sys::console::log_1(&format!("ICE candidate: {:?}", candidate).into());
                
                // In a real implementation, this would send the candidate to the remote peer
                // via the signaling server
            }
        }) as Box<dyn FnMut(RtcPeerConnectionIceEvent)>);
        
        peer_connection.set_onicecandidate(Some(on_ice_candidate.as_ref().unchecked_ref()));
        on_ice_candidate.forget();
        
        // Handle connection state changes
        let pc_clone2 = pc_clone.clone();
        let peer_id_clone2 = peer_id_clone.clone();
        let on_connection_state_change = Closure::wrap(Box::new(move || {
            let state = pc_clone2.connection_state();
            web_sys::console::log_1(&format!("Connection state changed to: {:?}", state).into());
            
            match state {
                web_sys::RtcPeerConnectionState::Connected => {
                    web_sys::console::log_1(&format!("Connected to peer: {}", peer_id_clone2).into());
                }
                web_sys::RtcPeerConnectionState::Failed => {
                    web_sys::console::log_1(&format!("Connection failed with peer: {}", peer_id_clone2).into());
                }
                web_sys::RtcPeerConnectionState::Disconnected => {
                    web_sys::console::log_1(&format!("Disconnected from peer: {}", peer_id_clone2).into());
                }
                _ => {}
            }
        }) as Box<dyn FnMut()>);
        
        peer_connection.set_onconnectionstatechange(Some(on_connection_state_change.as_ref().unchecked_ref()));
        on_connection_state_change.forget();
        
        // Handle data channel creation
        let peer_id_clone3 = peer_id_clone.clone();
        let self_entity_id = self.entity_id.clone();
        let peer_connections = self.peer_connections.clone();
        
        let on_data_channel = Closure::wrap(Box::new(move |event: web_sys::RtcDataChannelEvent| {
            web_sys::console::log_1(&format!("Data channel received from peer: {}", peer_id_clone3).into());
            
            let data_channel = event.channel();
            
            // Set up data channel handlers (should extract this to a separate method)
            let dc_clone = data_channel.clone();
            let peer_id_clone4 = peer_id_clone3.clone();
            
            let on_message = Closure::wrap(Box::new(move |event: MessageEvent| {
                if let Ok(data) = event.data().dyn_into::<js_sys::ArrayBuffer>() {
                    web_sys::console::log_1(&format!("Received array buffer from {}: {} bytes", 
                        peer_id_clone4, data.byte_length()).into());
                    
                    // Process message data
                    // In a real implementation, we would deserialize and process the message
                    
                } else if let Ok(text) = event.data().dyn_into::<js_sys::JsString>() {
                    web_sys::console::log_1(&format!("Received text from {}: {}", 
                        peer_id_clone4, text.as_string().unwrap_or_default()).into());
                    
                    // Process text message
                }
            }) as Box<dyn FnMut(MessageEvent)>);
            
            dc_clone.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
            on_message.forget();
            
            // Store the data channel
            if let Ok(mut peer_connections) = peer_connections.lock() {
                if let Some(connection) = peer_connections.get_mut(&peer_id_clone3) {
                    connection.data_channel = Some(data_channel);
                    connection.connection_state = WebRtcConnectionState::Connected;
                }
            }
        }) as Box<dyn FnMut(web_sys::RtcDataChannelEvent)>);
        
        peer_connection.set_ondatachannel(Some(on_data_channel.as_ref().unchecked_ref()));
        on_data_channel.forget();
        
        Ok(())
    }
    
    /// Set up data channel event handlers
    async fn setup_data_channel_handlers(&self, data_channel: &RtcDataChannel, peer_id: &str) -> Result<()> {
        let dc_clone = data_channel.clone();
        let peer_id_clone = peer_id.to_string();
        
        // Handle open event
        let on_open = Closure::wrap(Box::new(move |_event: Event| {
            web_sys::console::log_1(&format!("Data channel opened with peer: {}", peer_id_clone).into());
            
            // Send initial handshake message
            let message = format!("{{\"type\":\"handshake\",\"entity_id\":\"{}\"}}", peer_id_clone);
            if let Err(e) = dc_clone.send_with_str(&message) {
                web_sys::console::error_1(&format!("Error sending handshake: {:?}", e).into());
            }
        }) as Box<dyn FnMut(Event)>);
        
        data_channel.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        on_open.forget();
        
        // Handle error
        let dc_clone = data_channel.clone();
        let peer_id_clone = peer_id.to_string();
        let on_error = Closure::wrap(Box::new(move |event: Event| {
            web_sys::console::error_1(&format!("Data channel error with peer {}: {:?}", peer_id_clone, event).into());
        }) as Box<dyn FnMut(Event)>);
        
        data_channel.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        on_error.forget();
        
        // Handle close
        let peer_id_clone = peer_id.to_string();
        let peer_connections = self.peer_connections.clone();
        let on_close = Closure::wrap(Box::new(move |_event: Event| {
            web_sys::console::log_1(&format!("Data channel closed with peer: {}", peer_id_clone).into());
            
            // Update connection state
            if let Ok(mut peer_connections) = peer_connections.lock() {
                if let Some(connection) = peer_connections.get_mut(&peer_id_clone) {
                    connection.connection_state = WebRtcConnectionState::Closed;
                    connection.data_channel = None;
                }
            }
        }) as Box<dyn FnMut(Event)>);
        
        data_channel.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        on_close.forget();
        
        // Handle messages
        let peer_id_clone = peer_id.to_string();
        let on_message = Closure::wrap(Box::new(move |event: MessageEvent| {
            if let Ok(data) = event.data().dyn_into::<js_sys::ArrayBuffer>() {
                web_sys::console::log_1(&format!("Received array buffer from {}: {} bytes", 
                    peer_id_clone, data.byte_length()).into());
                
                // Process binary message
                
            } else if let Ok(text) = event.data().dyn_into::<js_sys::JsString>() {
                web_sys::console::log_1(&format!("Received text from {}: {}", 
                    peer_id_clone, text.as_string().unwrap_or_default()).into());
                
                // Process text message
            }
        }) as Box<dyn FnMut(MessageEvent)>);
        
        data_channel.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        on_message.forget();
        
        Ok(())
    }
    
    /// Create RTC configuration with ICE servers
    fn create_rtc_configuration(&self) -> Result<RtcConfiguration> {
        let config = RtcConfiguration::new();
        let ice_servers = Array::new();
        
        // Add ICE servers from config
        for server in &self.ice_servers {
            let ice_server = RtcIceServer::new();
            Reflect::set(&ice_server, &JsValue::from_str("urls"), &JsValue::from_str(&server.url))?;
            
            if let Some(username) = &server.username {
                Reflect::set(&ice_server, &JsValue::from_str("username"), &JsValue::from_str(username))?;
            }
            
            if let Some(credential) = &server.credential {
                Reflect::set(&ice_server, &JsValue::from_str("credential"), &JsValue::from_str(credential))?;
            }
            
            ice_servers.push(&ice_server);
        }
        
        Reflect::set(&config, &JsValue::from_str("iceServers"), &ice_servers)?;
        
        Ok(config)
    }
}
