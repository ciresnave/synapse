//! Browser storage implementation for Synapse data        // Create new browser storage manager
    pub fn new(key_prefix: &str) -> Self {
        Self {
            key_prefix: key_prefix.to_string(),
            use_local_storage: true,
            use_session_storage: true,
            use_indexed_db: true, // Now implemented
        }
    }
    
    /// Initialize IndexedDB database
    pub async fn init_indexed_db(&self) -> Result<()> {
        // Skip if IndexedDB is not enabled
        if !self.use_indexed_db {
            return Ok(());
        }
        
        let window = web_sys::window().ok_or_else(|| anyhow::anyhow!("No window object"))?;
        let indexed_db = window.indexed_db()?
            .ok_or_else(|| anyhow::anyhow!("IndexedDB not supported"))?;
        
        let db_name = format!("{}_synapse", self.key_prefix);
        let open_request = indexed_db.open_with_u32(&db_name, 1)?;
        
        // Handle database upgrade (first time creation)
        let upgrade_needed = Closure::once_into_js(move |event: web_sys::IdbVersionChangeEvent| {
            web_sys::console::log_1(&"Creating IndexedDB database...".into());
            
            let db = event.target()
                .and_then(|target| target.dyn_into::<web_sys::IdbOpenDbRequest>().ok())
                .and_then(|request| request.result().ok())
                .and_then(|result| result.dyn_into::<web_sys::IdbDatabase>().ok());
                
            if let Some(db) = db {
                // Create object stores
                let _ = db.create_object_store("participants");
                let _ = db.create_object_store("messages");
                let _ = db.create_object_store("blocks");
                let _ = db.create_object_store("trust_reports");
                
                web_sys::console::log_1(&"IndexedDB database created successfully!".into());
            }
        });
        
        open_request.set_onupgradeneeded(Some(upgrade_needed.as_ref().unchecked_ref()));
        
        // Create a promise for the open request completion
        let promise = Promise::new(&mut |resolve, reject| {
            let on_success = Closure::once_into_js(move |_event: web_sys::Event| {
                web_sys::console::log_1(&"IndexedDB opened successfully!".into());
                resolve.call0(&JsValue::NULL).unwrap();
            });
            
            let on_error = Closure::once_into_js(move |event: web_sys::Event| {
                web_sys::console::error_1(&"Error opening IndexedDB".into());
                reject.call1(&JsValue::NULL, &event).unwrap();
            });
            
            open_request.set_onsuccess(Some(on_success.as_ref().unchecked_ref()));
            open_request.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        });
        
        // Wait for the promise to resolve
        JsFuture::from(promise).await?;
        
        Ok(())
    }
//! 
//! This module provides persistent storage capabilities using browser APIs
//! like localStorage, sessionStorage, and IndexedDB for different types of data.

use wasm_bindgen::prelude::*;
use web_sys::{window, Storage};
use gloo::storage::{LocalStorage, SessionStorage, Storage as GlooStorage};
use std::collections::HashMap;

use crate::error::Result;

/// Browser storage manager for Synapse data
pub struct BrowserStorage {
    key_prefix: String,
    use_local_storage: bool,
    use_session_storage: bool,
    use_indexed_db: bool,
}

/// Storage type for different kinds of data
#[derive(Debug, Clone, Copy)]
pub enum StorageType {
    /// Persistent across browser sessions
    Persistent,
    /// Only for current session
    Session,
    /// Large data with complex queries
    Database,
}

/// Stored peer information
#[derive(Debug, Clone)]
pub struct StoredPeer {
    pub entity_id: String,
    pub display_name: String,
    pub last_seen: f64,
    pub capabilities: Vec<String>,
    pub connection_info: StoredConnectionInfo,
}

/// Connection information for peers
#[derive(Debug, Clone)]
pub struct StoredConnectionInfo {
    pub webrtc_supported: bool,
    pub websocket_endpoints: Vec<String>,
    pub ice_servers: Vec<String>,
    pub preferred_transport: String,
}

impl BrowserStorage {
    /// Create a new browser storage manager
    pub fn new(key_prefix: &str) -> Self {
        Self {
            key_prefix: key_prefix.to_string(),
            use_local_storage: true,
            use_session_storage: true,
            use_indexed_db: false, // TODO: Implement IndexedDB support
        }
    }
    
    /// Store peer information
    pub async fn store_peer(&self, peer: &StoredPeer) -> Result<()> {
        let key = format!("{}peer_{}", self.key_prefix, peer.entity_id);
        let serialized = self.serialize_peer(peer)?;
        
        if self.use_local_storage {
            LocalStorage::set(&key, serialized)
                .map_err(|e| anyhow::anyhow!("Failed to store peer in localStorage: {:?}", e))?;
        }
        
        web_sys::console::log_1(&format!("Stored peer: {}", peer.entity_id).into());
        Ok(())
    }
    
    /// Load peer information
    pub async fn load_peer(&self, entity_id: &str) -> Result<Option<StoredPeer>> {
        let key = format!("{}peer_{}", self.key_prefix, entity_id);
        
        if self.use_local_storage {
            match LocalStorage::get::<String>(&key) {
                Ok(serialized) => {
                    let peer = self.deserialize_peer(&serialized)?;
                    return Ok(Some(peer));
                }
                Err(_) => {
                    // Peer not found
                }
            }
        }
        
        Ok(None)
    }
    
    /// Load all stored peers
    pub async fn load_all_peers(&self) -> Result<Vec<StoredPeer>> {
        let mut peers = Vec::new();
        
        if let Some(storage) = self.get_local_storage() {
            let peer_prefix = format!("{}peer_", self.key_prefix);
            
            // Iterate through all keys in localStorage
            for i in 0..storage.length().unwrap_or(0) {
                if let Ok(Some(key)) = storage.key(i) {
                    if key.starts_with(&peer_prefix) {
                        if let Ok(Some(value)) = storage.get_item(&key) {
                            if let Ok(peer) = self.deserialize_peer(&value) {
                                peers.push(peer);
                            }
                        }
                    }
                }
            }
        }
        
        web_sys::console::log_1(&format!("Loaded {} peers from storage", peers.len()).into());
        Ok(peers)
    }
    
    /// Store connection history
    pub async fn store_connection_history(&self, peer_id: &str, success: bool, latency_ms: Option<u32>) -> Result<()> {
        let key = format!("{}connection_history_{}", self.key_prefix, peer_id);
        
        // Load existing history
        let mut history = self.load_connection_history(peer_id).await.unwrap_or_default();
        
        // Add new entry
        let entry = ConnectionHistoryEntry {
            timestamp: js_sys::Date::now(),
            success,
            latency_ms,
        };
        
        history.push(entry);
        
        // Keep only last 100 entries
        if history.len() > 100 {
            history.drain(0..history.len() - 100);
        }
        
        // Store updated history
        let serialized = self.serialize_connection_history(&history)?;
        
        if self.use_local_storage {
            LocalStorage::set(&key, serialized)
                .map_err(|e| anyhow::anyhow!("Failed to store connection history: {:?}", e))?;
        }
        
        Ok(())
    }
    
    /// Load connection history
    pub async fn load_connection_history(&self, peer_id: &str) -> Result<Vec<ConnectionHistoryEntry>> {
        let key = format!("{}connection_history_{}", self.key_prefix, peer_id);
        
        if self.use_local_storage {
            match LocalStorage::get::<String>(&key) {
                Ok(serialized) => {
                    return self.deserialize_connection_history(&serialized);
                }
                Err(_) => {
                    // History not found
                }
            }
        }
        
        Ok(Vec::new())
    }
    
    /// Store application configuration
    pub async fn store_config(&self, config: &HashMap<String, String>) -> Result<()> {
        let key = format!("{}config", self.key_prefix);
        let serialized = self.serialize_config(config)?;
        
        if self.use_local_storage {
            LocalStorage::set(&key, serialized)
                .map_err(|e| anyhow::anyhow!("Failed to store config: {:?}", e))?;
        }
        
        web_sys::console::log_1(&"Stored application configuration".into());
        Ok(())
    }
    
    /// Load application configuration
    pub async fn load_config(&self) -> Result<HashMap<String, String>> {
        let key = format!("{}config", self.key_prefix);
        
        if self.use_local_storage {
            match LocalStorage::get::<String>(&key) {
                Ok(serialized) => {
                    return self.deserialize_config(&serialized);
                }
                Err(_) => {
                    // Config not found
                }
            }
        }
        
        Ok(HashMap::new())
    }
    
    /// Clear all stored data
    pub async fn clear_all(&self) -> Result<()> {
        if let Some(storage) = self.get_local_storage() {
            let mut keys_to_remove = Vec::new();
            
            // Find all keys with our prefix
            for i in 0..storage.length().unwrap_or(0) {
                if let Ok(Some(key)) = storage.key(i) {
                    if key.starts_with(&self.key_prefix) {
                        keys_to_remove.push(key);
                    }
                }
            }
            
            // Remove all found keys
            for key in keys_to_remove {
                storage.remove_item(&key).ok();
            }
        }
        
        web_sys::console::log_1(&"Cleared all Synapse data from storage".into());
        Ok(())
    }
    
    /// Get storage usage statistics
    pub async fn get_usage_stats(&self) -> Result<StorageStats> {
        let mut total_size = 0;
        let mut peer_count = 0;
        let mut config_size = 0;
        
        if let Some(storage) = self.get_local_storage() {
            for i in 0..storage.length().unwrap_or(0) {
                if let Ok(Some(key)) = storage.key(i) {
                    if key.starts_with(&self.key_prefix) {
                        if let Ok(Some(value)) = storage.get_item(&key) {
                            total_size += key.len() + value.len();
                            
                            if key.contains("peer_") {
                                peer_count += 1;
                            } else if key.contains("config") {
                                config_size += value.len();
                            }
                        }
                    }
                }
            }
        }
        
        Ok(StorageStats {
            total_size_bytes: total_size,
            peer_count,
            config_size_bytes: config_size,
        })
    }
    
    /// Store data in IndexedDB
    pub async fn store_in_indexed_db(&self, store_name: &str, key: &str, value: &str) -> Result<()> {
        if !self.use_indexed_db {
            return Ok(());
        }
        
        let window = web_sys::window().ok_or_else(|| anyhow::anyhow!("No window object"))?;
        let indexed_db = window.indexed_db()?
            .ok_or_else(|| anyhow::anyhow!("IndexedDB not supported"))?;
        
        let db_name = format!("{}_synapse", self.key_prefix);
        let open_request = indexed_db.open(&db_name)?;
        
        // Create a promise for the transaction
        let key_str = key.to_string();
        let val_str = value.to_string();
        let store_name_str = store_name.to_string();
        
        let promise = Promise::new(&mut |resolve, reject| {
            let on_success = Closure::once_into_js(move |event: web_sys::Event| {
                let db = event.target()
                    .and_then(|target| target.dyn_into::<web_sys::IdbOpenDbRequest>().ok())
                    .and_then(|request| request.result().ok())
                    .and_then(|result| result.dyn_into::<web_sys::IdbDatabase>().ok());
                    
                if let Some(db) = db {
                    // Create transaction
                    if let Ok(tx) = db.transaction_with_str_and_mode(
                        &store_name_str,
                        web_sys::IdbTransactionMode::Readwrite,
                    ) {
                        if let Ok(store) = tx.object_store(&store_name_str) {
                            // Store the value
                            let key_js = JsValue::from_str(&key_str);
                            let value_js = JsValue::from_str(&val_str);
                            
                            if let Ok(put_request) = store.put_with_key(&value_js, &key_js) {
                                let put_success = Closure::once_into_js(move |_: web_sys::Event| {
                                    web_sys::console::log_1(&format!("Stored {} in IndexedDB", key_str).into());
                                    resolve.call0(&JsValue::NULL).unwrap();
                                });
                                
                                let put_error = Closure::once_into_js(move |event: web_sys::Event| {
                                    web_sys::console::error_1(&format!("Error storing in IndexedDB: {:?}", event).into());
                                    reject.call1(&JsValue::NULL, &event).unwrap();
                                });
                                
                                put_request.set_onsuccess(Some(put_success.as_ref().unchecked_ref()));
                                put_request.set_onerror(Some(put_error.as_ref().unchecked_ref()));
                            } else {
                                reject.call1(&JsValue::NULL, &JsValue::from_str("Failed to create put request")).unwrap();
                            }
                        } else {
                            reject.call1(&JsValue::NULL, &JsValue::from_str("Failed to open object store")).unwrap();
                        }
                    } else {
                        reject.call1(&JsValue::NULL, &JsValue::from_str("Failed to create transaction")).unwrap();
                    }
                } else {
                    reject.call1(&JsValue::NULL, &JsValue::from_str("Failed to open database")).unwrap();
                }
            });
            
            let on_error = Closure::once_into_js(move |event: web_sys::Event| {
                web_sys::console::error_1(&"Error opening IndexedDB".into());
                reject.call1(&JsValue::NULL, &event).unwrap();
            });
            
            open_request.set_onsuccess(Some(on_success.as_ref().unchecked_ref()));
            open_request.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        });
        
        // Wait for the promise to resolve
        JsFuture::from(promise).await?;
        
        Ok(())
    }
    
    /// Retrieve data from IndexedDB
    pub async fn get_from_indexed_db(&self, store_name: &str, key: &str) -> Result<Option<String>> {
        if !self.use_indexed_db {
            return Ok(None);
        }
        
        let window = web_sys::window().ok_or_else(|| anyhow::anyhow!("No window object"))?;
        let indexed_db = window.indexed_db()?
            .ok_or_else(|| anyhow::anyhow!("IndexedDB not supported"))?;
        
        let db_name = format!("{}_synapse", self.key_prefix);
        let open_request = indexed_db.open(&db_name)?;
        
        // Create a promise for the retrieval
        let key_str = key.to_string();
        let store_name_str = store_name.to_string();
        
        let promise = Promise::new(&mut |resolve, reject| {
            let on_success = Closure::once_into_js(move |event: web_sys::Event| {
                let db = event.target()
                    .and_then(|target| target.dyn_into::<web_sys::IdbOpenDbRequest>().ok())
                    .and_then(|request| request.result().ok())
                    .and_then(|result| result.dyn_into::<web_sys::IdbDatabase>().ok());
                    
                if let Some(db) = db {
                    // Create transaction
                    if let Ok(tx) = db.transaction_with_str(&store_name_str) {
                        if let Ok(store) = tx.object_store(&store_name_str) {
                            // Get the value
                            let key_js = JsValue::from_str(&key_str);
                            
                            if let Ok(get_request) = store.get(&key_js) {
                                let get_success = Closure::once_into_js(move |event: web_sys::Event| {
                                    let result = event.target()
                                        .and_then(|target| target.dyn_into::<web_sys::IdbRequest>().ok())
                                        .and_then(|request| request.result().ok());
                                        
                                    if let Some(value) = result {
                                        if value.is_undefined() || value.is_null() {
                                            // Key not found
                                            resolve.call1(&JsValue::NULL, &JsValue::NULL).unwrap();
                                        } else if let Some(value_str) = value.as_string() {
                                            // Found string value
                                            resolve.call1(&JsValue::NULL, &JsValue::from_str(&value_str)).unwrap();
                                        } else {
                                            // Found value but not a string
                                            resolve.call1(&JsValue::NULL, &JsValue::NULL).unwrap();
                                        }
                                    } else {
                                        // No result
                                        resolve.call1(&JsValue::NULL, &JsValue::NULL).unwrap();
                                    }
                                });
                                
                                let get_error = Closure::once_into_js(move |event: web_sys::Event| {
                                    web_sys::console::error_1(&format!("Error getting from IndexedDB: {:?}", event).into());
                                    reject.call1(&JsValue::NULL, &event).unwrap();
                                });
                                
                                get_request.set_onsuccess(Some(get_success.as_ref().unchecked_ref()));
                                get_request.set_onerror(Some(get_error.as_ref().unchecked_ref()));
                            } else {
                                reject.call1(&JsValue::NULL, &JsValue::from_str("Failed to create get request")).unwrap();
                            }
                        } else {
                            reject.call1(&JsValue::NULL, &JsValue::from_str("Failed to open object store")).unwrap();
                        }
                    } else {
                        reject.call1(&JsValue::NULL, &JsValue::from_str("Failed to create transaction")).unwrap();
                    }
                } else {
                    reject.call1(&JsValue::NULL, &JsValue::from_str("Failed to open database")).unwrap();
                }
            });
            
            let on_error = Closure::once_into_js(move |event: web_sys::Event| {
                web_sys::console::error_1(&"Error opening IndexedDB".into());
                reject.call1(&JsValue::NULL, &event).unwrap();
            });
            
            open_request.set_onsuccess(Some(on_success.as_ref().unchecked_ref()));
            open_request.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        });
        
        // Wait for the promise to resolve
        let result = JsFuture::from(promise).await?;
        
        if result.is_null() || result.is_undefined() {
            Ok(None)
        } else {
            Ok(result.as_string())
        }
    }
    
    /// Store large data objects
    pub async fn store_large_data(&self, _key: &str, _data: &[u8]) -> Result<()> {
        // TODO: Implement large data storage
        Err(anyhow::anyhow!("IndexedDB not yet implemented"))
    }
    
    /// Load large data objects
    pub async fn load_large_data(&self, _key: &str) -> Result<Option<Vec<u8>>> {
        // TODO: Implement large data loading
        Err(anyhow::anyhow!("IndexedDB not yet implemented"))
    }
}

/// Connection history entry
#[derive(Debug, Clone)]
pub struct ConnectionHistoryEntry {
    pub timestamp: f64,
    pub success: bool,
    pub latency_ms: Option<u32>,
}

/// Storage usage statistics
#[derive(Debug, Clone)]
pub struct StorageStats {
    pub total_size_bytes: usize,
    pub peer_count: usize,
    pub config_size_bytes: usize,
}

// Storage type definitions for future IndexedDB implementation
pub mod indexed_db {
    use super::*;
    
    /// IndexedDB storage for large data sets
    pub struct IndexedDbStorage {
        db_name: String,
        version: u32,
    }
    
    impl IndexedDbStorage {
        pub fn new(db_name: String) -> Self {
            Self {
                db_name,
                version: 1,
            }
        }
        
        /// Initialize IndexedDB database
        pub async fn initialize(&self) -> Result<()> {
            // TODO: Implement IndexedDB initialization
            web_sys::console::log_1(&"IndexedDB storage not yet implemented".into());
            Ok(())
        }
    }
}
