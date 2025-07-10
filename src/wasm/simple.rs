//! Simple WebAssembly bindings for Synapse
//! 
//! This module provides basic WebAssembly functionality for the Synapse protocol.

use wasm_bindgen::prelude::*;

/// Simple browser-based Synapse node
#[wasm_bindgen]
pub struct WasmSynapseNode {
    /// Our entity ID in the Synapse network
    entity_id: String,
    
    /// Simple configuration
    config: WasmConfig,
}

/// Simple WASM configuration
#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct WasmConfig {
    entity_name: String,
    entity_type: String,
}

#[wasm_bindgen]
impl WasmConfig {
    /// Get the entity name
    #[wasm_bindgen(getter)]
    pub fn entity_name(&self) -> String {
        self.entity_name.clone()
    }
    
    /// Get the entity type
    #[wasm_bindgen(getter)]
    pub fn entity_type(&self) -> String {
        self.entity_type.clone()
    }
}

#[wasm_bindgen]
impl WasmSynapseNode {
    /// Create a new WASM Synapse node
    #[wasm_bindgen(constructor)]
    pub fn new(entity_name: String, entity_type: String) -> WasmSynapseNode {
        let entity_id = format!("{}@wasm.synapse.local", entity_name);
        
        WasmSynapseNode {
            entity_id,
            config: WasmConfig {
                entity_name,
                entity_type,
            },
        }
    }
    
    /// Get the entity ID
    #[wasm_bindgen(getter)]
    pub fn entity_id(&self) -> String {
        self.entity_id.clone()
    }
    
    /// Get the entity name
    #[wasm_bindgen(getter)]
    pub fn entity_name(&self) -> String {
        self.config.entity_name.clone()
    }
    
    /// Get the entity type
    #[wasm_bindgen(getter)]
    pub fn entity_type(&self) -> String {
        self.config.entity_type.clone()
    }
    
    /// Log a message to the browser console
    #[wasm_bindgen]
    pub fn log(&self, message: &str) {
        web_sys::console::log_1(&format!("[{}] {}", self.entity_id, message).into());
    }
    
    /// Send a simple message (placeholder implementation)
    #[wasm_bindgen]
    pub fn send_message(&self, target: &str, message: &str) -> String {
        let formatted = format!("Message from {} to {}: {}", self.entity_id, target, message);
        self.log(&formatted);
        formatted
    }
}

/// Initialize panic hook for better error messages in WASM
#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
    
    #[cfg(feature = "wasm-logger")]
    wasm_logger::init(wasm_logger::Config::default());
}

/// Get the current timestamp as a string
#[wasm_bindgen]
pub fn get_timestamp() -> String {
    js_sys::Date::new_0().to_iso_string().as_string().unwrap_or_default()
}

/// Simple utility function to test WASM compilation
#[wasm_bindgen]
pub fn test_wasm() -> String {
    "WASM is working! Synapse Neural Communication Network is ready.".to_string()
}
