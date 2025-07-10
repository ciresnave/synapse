//! Web Worker implementation for background Synapse tasks
//! 
//! This module provides Web Worker support for running Synapse operations
//! in the background without blocking the main browser thread.

use wasm_bindgen::prelude::*;
use gloo::worker::{Worker, WorkerBridge, Spawnable, HandlerId, WorkerScope};
use std::collections::HashMap;

use crate::error::Result;

/// Background worker for Synapse operations
pub struct SynapseWorker {
    scope: WorkerScope<Self>,
    peers: HashMap<String, WorkerPeer>,
    connections: HashMap<String, WorkerConnection>,
    discovery_interval: Option<gloo::timers::callback::Interval>,
}

/// Worker-side peer information
#[derive(Debug, Clone)]
pub struct WorkerPeer {
    pub entity_id: String,
    pub last_seen: f64,
    pub connection_attempts: u32,
    pub success_rate: f32,
}

/// Worker-side connection information
#[derive(Debug, Clone)]
pub struct WorkerConnection {
    pub peer_id: String,
    pub transport_type: String,
    pub established_at: f64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

/// Messages sent to the worker
#[derive(Debug, Clone)]
pub enum WorkerInput {
    StartDiscovery { interval_ms: u32 },
    StopDiscovery,
    AddPeer { peer: WorkerPeer },
    RemovePeer { entity_id: String },
    UpdateConnection { connection: WorkerConnection },
    PerformHealthCheck,
    CleanupStaleData,
    GetStatistics,
}

/// Messages sent from the worker
#[derive(Debug, Clone)]
pub enum WorkerOutput {
    DiscoveryResult { peers: Vec<WorkerPeer> },
    HealthCheckResult { healthy_peers: Vec<String> },
    StatisticsReport { stats: WorkerStatistics },
    Error { message: String },
    Log { level: String, message: String },
}

/// Worker statistics
#[derive(Debug, Clone)]
pub struct WorkerStatistics {
    pub total_peers: usize,
    pub active_connections: usize,
    pub total_bytes_sent: u64,
    pub total_bytes_received: u64,
    pub uptime_ms: f64,
    pub discovery_cycles: u32,
}

impl Worker for SynapseWorker {
    type Input = WorkerInput;
    type Output = WorkerOutput;
    
    fn create(scope: &WorkerScope<Self>) -> Self {
        web_sys::console::log_1(&"Creating Synapse background worker...".into());
        
        Self {
            scope: scope.clone(),
            peers: HashMap::new(),
            connections: HashMap::new(),
            discovery_interval: None,
        }
    }
    
    fn update(&mut self, msg: Self::Input) {
        match msg {
            WorkerInput::StartDiscovery { interval_ms } => {
                self.start_discovery(interval_ms);
            }
            WorkerInput::StopDiscovery => {
                self.stop_discovery();
            }
            WorkerInput::AddPeer { peer } => {
                self.add_peer(peer);
            }
            WorkerInput::RemovePeer { entity_id } => {
                self.remove_peer(&entity_id);
            }
            WorkerInput::UpdateConnection { connection } => {
                self.update_connection(connection);
            }
            WorkerInput::PerformHealthCheck => {
                self.perform_health_check();
            }
            WorkerInput::CleanupStaleData => {
                self.cleanup_stale_data();
            }
            WorkerInput::GetStatistics => {
                self.send_statistics();
            }
        }
    }
}

impl SynapseWorker {
    /// Start periodic peer discovery
    fn start_discovery(&mut self, interval_ms: u32) {
        self.log("info", &format!("Starting discovery with interval {}ms", interval_ms));
        
        let scope = self.scope.clone();
        let interval = gloo::timers::callback::Interval::new(interval_ms, move || {
            // Perform discovery operation
            scope.respond(
                HandlerId::new(0, false), // Use a default handler ID
                WorkerOutput::Log {
                    level: "debug".to_string(),
                    message: "Performing background discovery...".to_string(),
                }
            );
            
            // In a real implementation, this would:
            // 1. Check WebRTC signaling servers
            // 2. Ping WebSocket relays
            // 3. Test connection quality to known peers
            // 4. Update peer availability status
        });
        
        self.discovery_interval = Some(interval);
    }
    
    /// Stop periodic discovery
    fn stop_discovery(&mut self) {
        if let Some(_interval) = self.discovery_interval.take() {
            self.log("info", "Stopped background discovery");
        }
    }
    
    /// Add a peer to the worker's tracking
    fn add_peer(&mut self, peer: WorkerPeer) {
        self.log("debug", &format!("Adding peer to worker: {}", peer.entity_id));
        self.peers.insert(peer.entity_id.clone(), peer);
    }
    
    /// Remove a peer from tracking
    fn remove_peer(&mut self, entity_id: &str) {
        if self.peers.remove(entity_id).is_some() {
            self.log("debug", &format!("Removed peer from worker: {}", entity_id));
        }
    }
    
    /// Update connection information
    fn update_connection(&mut self, connection: WorkerConnection) {
        self.log("debug", &format!("Updating connection for peer: {}", connection.peer_id));
        self.connections.insert(connection.peer_id.clone(), connection);
    }
    
    /// Perform health checks on all peers
    fn perform_health_check(&mut self) {
        self.log("info", "Performing health check on all peers");
        
        let mut healthy_peers = Vec::new();
        
        for (entity_id, peer) in &self.peers {
            // Simple health check based on last seen time
            let now = js_sys::Date::now();
            let time_since_seen = now - peer.last_seen;
            
            if time_since_seen < 300000.0 { // 5 minutes
                healthy_peers.push(entity_id.clone());
            }
        }
        
        self.scope.respond(
            HandlerId::new(0, false),
            WorkerOutput::HealthCheckResult { healthy_peers }
        );
    }
    
    /// Clean up stale data and connections
    fn cleanup_stale_data(&mut self) {
        self.log("info", "Cleaning up stale data");
        
        let now = js_sys::Date::now();
        let stale_threshold = 3600000.0; // 1 hour
        
        // Remove stale peers
        let initial_peer_count = self.peers.len();
        self.peers.retain(|_, peer| {
            now - peer.last_seen < stale_threshold
        });
        
        let removed_peers = initial_peer_count - self.peers.len();
        if removed_peers > 0 {
            self.log("info", &format!("Removed {} stale peers", removed_peers));
        }
        
        // Remove stale connections
        let initial_connection_count = self.connections.len();
        self.connections.retain(|peer_id, _| {
            self.peers.contains_key(peer_id)
        });
        
        let removed_connections = initial_connection_count - self.connections.len();
        if removed_connections > 0 {
            self.log("info", &format!("Removed {} stale connections", removed_connections));
        }
    }
    
    /// Send statistics report
    fn send_statistics(&self) {
        let stats = WorkerStatistics {
            total_peers: self.peers.len(),
            active_connections: self.connections.len(),
            total_bytes_sent: self.connections.values()
                .map(|c| c.bytes_sent)
                .sum(),
            total_bytes_received: self.connections.values()
                .map(|c| c.bytes_received)
                .sum(),
            uptime_ms: js_sys::Date::now(), // Simplified - would track actual uptime
            discovery_cycles: 0, // Would track actual cycles
        };
        
        self.scope.respond(
            HandlerId::new(0, false),
            WorkerOutput::StatisticsReport { stats }
        );
    }
    
    /// Send a log message
    fn log(&self, level: &str, message: &str) {
        self.scope.respond(
            HandlerId::new(0, false),
            WorkerOutput::Log {
                level: level.to_string(),
                message: message.to_string(),
            }
        );
    }
}

/// Worker bridge for communicating with the background worker
pub struct SynapseWorkerBridge {
    bridge: WorkerBridge<SynapseWorker>,
}

impl SynapseWorkerBridge {
    /// Create a new worker bridge
    pub fn new() -> Result<Self> {
        let bridge = SynapseWorker::spawner()
            .callback(|output| {
                match output {
                    WorkerOutput::Log { level, message } => {
                        match level.as_str() {
                            "error" => web_sys::console::error_1(&message.into()),
                            "warn" => web_sys::console::warn_1(&message.into()),
                            "info" => web_sys::console::log_1(&message.into()),
                            "debug" => web_sys::console::log_1(&format!("[DEBUG] {}", message).into()),
                            _ => web_sys::console::log_1(&message.into()),
                        }
                    }
                    WorkerOutput::DiscoveryResult { peers } => {
                        web_sys::console::log_1(&format!("Discovery found {} peers", peers.len()).into());
                    }
                    WorkerOutput::HealthCheckResult { healthy_peers } => {
                        web_sys::console::log_1(&format!("{} healthy peers", healthy_peers.len()).into());
                    }
                    WorkerOutput::StatisticsReport { stats } => {
                        web_sys::console::log_1(&format!(
                            "Stats: {} peers, {} connections, {}KB sent, {}KB received",
                            stats.total_peers,
                            stats.active_connections,
                            stats.total_bytes_sent / 1024,
                            stats.total_bytes_received / 1024
                        ).into());
                    }
                    WorkerOutput::Error { message } => {
                        web_sys::console::error_1(&format!("Worker error: {}", message).into());
                    }
                }
            })
            .spawn()?;
        
        Ok(Self { bridge })
    }
    
    /// Start background discovery
    pub fn start_discovery(&self, interval_ms: u32) {
        self.bridge.send(WorkerInput::StartDiscovery { interval_ms });
    }
    
    /// Stop background discovery
    pub fn stop_discovery(&self) {
        self.bridge.send(WorkerInput::StopDiscovery);
    }
    
    /// Add a peer for background tracking
    pub fn add_peer(&self, peer: WorkerPeer) {
        self.bridge.send(WorkerInput::AddPeer { peer });
    }
    
    /// Remove a peer from background tracking
    pub fn remove_peer(&self, entity_id: String) {
        self.bridge.send(WorkerInput::RemovePeer { entity_id });
    }
    
    /// Update connection information
    pub fn update_connection(&self, connection: WorkerConnection) {
        self.bridge.send(WorkerInput::UpdateConnection { connection });
    }
    
    /// Trigger a health check
    pub fn perform_health_check(&self) {
        self.bridge.send(WorkerInput::PerformHealthCheck);
    }
    
    /// Trigger cleanup of stale data
    pub fn cleanup_stale_data(&self) {
        self.bridge.send(WorkerInput::CleanupStaleData);
    }
    
    /// Request statistics report
    pub fn get_statistics(&self) {
        self.bridge.send(WorkerInput::GetStatistics);
    }
}

/// Utility functions for worker management
pub mod utils {
    use super::*;
    
    /// Check if Web Workers are supported
    pub fn is_worker_supported() -> bool {
        js_sys::Reflect::has(&js_sys::global(), &"Worker".into()).unwrap_or(false)
    }
    
    /// Get worker capabilities
    pub fn get_worker_capabilities() -> Vec<String> {
        let mut capabilities = Vec::new();
        
        if is_worker_supported() {
            capabilities.push("web_workers".to_string());
        }
        
        // Check for SharedArrayBuffer support
        if js_sys::Reflect::has(&js_sys::global(), &"SharedArrayBuffer".into()).unwrap_or(false) {
            capabilities.push("shared_array_buffer".to_string());
        }
        
        // Check for OffscreenCanvas support
        if js_sys::Reflect::has(&js_sys::global(), &"OffscreenCanvas".into()).unwrap_or(false) {
            capabilities.push("offscreen_canvas".to_string());
        }
        
        capabilities
    }
    
    /// Estimate worker performance
    pub fn estimate_worker_performance() -> f32 {
        // Simple performance estimation based on available features
        let mut score = 0.0;
        
        if is_worker_supported() {
            score += 0.5;
        }
        
        // Check for hardware concurrency
        if let Ok(navigator) = web_sys::window().unwrap().navigator().dyn_into::<web_sys::Navigator>() {
            if let Some(concurrency) = navigator.hardware_concurrency() {
                score += (concurrency as f32) * 0.1;
            }
        }
        
        score.min(1.0) // Cap at 1.0
    }
}
