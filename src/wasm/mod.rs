//! WebAssembly support for Synapse Neural Communication Network
//! 
//! This module provides basic WebAssembly bindings for the Synapse protocol.
//! Full WASM implementation is currently under development.

#[cfg(target_arch = "wasm32")]
pub mod simple;

// Re-export main types for WASM
#[cfg(target_arch = "wasm32")]
pub use simple::*;

// Non-WASM stub
#[cfg(not(target_arch = "wasm32"))]
pub mod stub {
    //! Stub implementations for non-WASM builds
    use crate::error::Result;
    
    pub struct WasmSynapseNode;
    
    impl WasmSynapseNode {
        pub async fn new() -> Result<Self> {
            Err("WebAssembly support only available in WASM builds".into())
        }
    }
}
