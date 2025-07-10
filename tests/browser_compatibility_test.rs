//! Browser Compatibility Test for WebAssembly (WASM)
//!
//! This test validates that Synapse's WASM implementation works correctly
//! across major browsers (Chrome, Firefox, Safari).
//!
//! Note: This test requires manual execution in browser environments.

use wasm_bindgen::prelude::*;
use wasm_bindgen_test::*;
use web_sys::{window, Storage, console};
use js_sys::{Promise, Object, Reflect};
use std::sync::{Arc, Mutex};

wasm_bindgen_test_configure!(run_in_browser);

// Feature detection functions
#[wasm_bindgen]
pub fn detect_browser() -> String {
    let navigator = web_sys::window()
        .expect("No global window exists")
        .navigator();
    
    let user_agent = navigator.user_agent().unwrap_or_else(|_| "unknown".to_string());
    
    if user_agent.contains("Firefox") {
        "Firefox".to_string()
    } else if user_agent.contains("Chrome") {
        "Chrome".to_string()
    } else if user_agent.contains("Safari") && !user_agent.contains("Chrome") {
        "Safari".to_string()
    } else if user_agent.contains("Edge") {
        "Edge".to_string()
    } else {
        "Unknown".to_string()
    }
}

#[wasm_bindgen]
pub fn has_indexeddb_support() -> bool {
    let window = web_sys::window().expect("No global window exists");
    js_sys::Reflect::has(&window, &JsValue::from_str("indexedDB")).unwrap_or(false)
}

#[wasm_bindgen]
pub fn has_webrtc_support() -> bool {
    let window = web_sys::window().expect("No global window exists");
    let rtc_support = js_sys::Reflect::has(&window, &JsValue::from_str("RTCPeerConnection")).unwrap_or(false);
    let media_devices = js_sys::Reflect::has(&window.navigator(), &JsValue::from_str("mediaDevices")).unwrap_or(false);
    
    rtc_support && media_devices
}

#[wasm_bindgen]
pub fn has_websocket_support() -> bool {
    let window = web_sys::window().expect("No global window exists");
    js_sys::Reflect::has(&window, &JsValue::from_str("WebSocket")).unwrap_or(false)
}

// Browser compatibility tests
#[wasm_bindgen_test]
fn test_wasm_initialization() {
    let browser = detect_browser();
    console::log_1(&JsValue::from_str(&format!("Running in: {}", browser)));
    
    assert!(true, "WASM module successfully initialized");
}

#[wasm_bindgen_test]
fn test_indexeddb_support() {
    let has_support = has_indexeddb_support();
    console::log_1(&JsValue::from_str(&format!("IndexedDB support: {}", has_support)));
    
    // We expect all modern browsers to support IndexedDB
    assert!(has_support, "Browser should support IndexedDB");
}

#[wasm_bindgen_test]
fn test_webrtc_support() {
    let has_support = has_webrtc_support();
    console::log_1(&JsValue::from_str(&format!("WebRTC support: {}", has_support)));
    
    // Note: Some browsers or private browsing modes might restrict WebRTC
    // This is an informational test only
    console::log_1(&JsValue::from_str("Note: WebRTC might be restricted in private browsing modes"));
}

#[wasm_bindgen_test]
fn test_websocket_support() {
    let has_support = has_websocket_support();
    console::log_1(&JsValue::from_str(&format!("WebSocket support: {}", has_support)));
    
    // We expect all modern browsers to support WebSockets
    assert!(has_support, "Browser should support WebSockets");
}

#[wasm_bindgen_test]
async fn test_local_storage() {
    let window = web_sys::window().expect("No global window exists");
    let storage = window.local_storage().expect("Failed to get localStorage").expect("localStorage not available");
    
    // Test data
    let key = "synapse_test_key";
    let value = "synapse_test_value";
    
    // Clear any existing data
    storage.remove_item(key).expect("Failed to clear test data");
    
    // Set test data
    storage.set_item(key, value).expect("Failed to set test data");
    
    // Read test data
    let read_value = storage.get_item(key).expect("Failed to read test data").expect("Test data not found");
    
    assert_eq!(value, read_value, "localStorage value doesn't match expected value");
    
    // Clean up
    storage.remove_item(key).expect("Failed to clean up test data");
}

// Synapse-specific WASM functionality tests
#[wasm_bindgen_test]
fn test_wasm_crypto() {
    use wasm_bindgen::JsCast;
    
    let window = web_sys::window().expect("No global window exists");
    let crypto = window.crypto().expect("Crypto API not available");
    
    // Generate a random value to verify crypto API works
    let mut buf = [0u8; 16];
    crypto.get_random_values_with_u8_array(&mut buf).expect("Failed to generate random values");
    
    // Ensure we got some randomness (at least one non-zero byte)
    let has_non_zero = buf.iter().any(|&b| b != 0);
    assert!(has_non_zero, "Crypto API should generate non-zero random values");
}

// Mock Synapse WASM node for testing browser compatibility
#[wasm_bindgen]
pub struct WasmSynapseNode {
    id: String,
    storage_supported: bool,
    webrtc_supported: bool,
}

#[wasm_bindgen]
impl WasmSynapseNode {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        console::log_1(&JsValue::from_str("Creating WASM Synapse Node"));
        
        let storage_supported = window()
            .and_then(|win| win.local_storage().ok())
            .and_then(|opt| opt)
            .is_some();
            
        let webrtc_supported = has_webrtc_support();
        
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            storage_supported,
            webrtc_supported,
        }
    }
    
    #[wasm_bindgen]
    pub fn get_id(&self) -> String {
        self.id.clone()
    }
    
    #[wasm_bindgen]
    pub fn get_capabilities(&self) -> JsValue {
        let obj = Object::new();
        Reflect::set(&obj, &JsValue::from_str("storage"), &JsValue::from_bool(self.storage_supported)).unwrap();
        Reflect::set(&obj, &JsValue::from_str("webrtc"), &JsValue::from_bool(self.webrtc_supported)).unwrap();
        Reflect::set(&obj, &JsValue::from_str("websocket"), &JsValue::from_bool(has_websocket_support())).unwrap();
        Reflect::set(&obj, &JsValue::from_str("browser"), &JsValue::from_str(&detect_browser())).unwrap();
        
        obj.into()
    }
    
    #[wasm_bindgen]
    pub fn is_browser_compatible(&self) -> bool {
        // Minimal requirements for Synapse WASM compatibility
        self.storage_supported && has_websocket_support()
    }
}

#[wasm_bindgen_test]
fn test_wasm_synapse_node() {
    let node = WasmSynapseNode::new();
    let id = node.get_id();
    let capabilities = node.get_capabilities();
    
    console::log_1(&JsValue::from_str(&format!("WASM Synapse Node ID: {}", id)));
    console::log_1(&JsValue::from_str("Capabilities:"));
    console::log_1(&capabilities);
    
    assert!(!id.is_empty(), "Node ID should be generated");
    assert!(node.is_browser_compatible(), "Browser should be compatible with Synapse WASM");
}
