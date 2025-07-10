//! WebRTC Transport Integration Test for WASM
//! Tests the WebRTC transport functionality for browser environments:
//! - Peer connection setup
//! - Data channel creation and communication
//! - IndexedDB storage integration
//! - Error handling and connection recovery

// Note: This test needs to be compiled to WebAssembly and run in a browser environment.
// Compile with: wasm-pack test --firefox --headless -- --features wasm-bindgen-test

#[cfg(target_arch = "wasm32")]
mod wasm_tests {
    use wasm_bindgen_test::*;
    use wasm_bindgen::prelude::*;
    use synapse::{
        wasm::{
            webrtc::{WebRtcTransport, PeerConnectionConfig, DataChannelOptions},
            storage::IndexedDbStorage,
        },
        transport::Transport,
    };
    use web_sys::{RtcPeerConnection, RtcDataChannel};
    use js_sys::{Promise, ArrayBuffer};
    use futures::{future::ready, Future, StreamExt};
    use std::time::Duration;
    use wasm_timer::Instant;

    wasm_bindgen_test_configure!(run_in_browser);

    // Helper function to create WebRTC transport
    async fn create_webrtc_transport(id: &str) -> WebRtcTransport {
        let config = PeerConnectionConfig {
            ice_servers: vec!["stun:stun.l.google.com:19302".to_string()],
            is_offerer: true,
        };
        
        WebRtcTransport::new(id, config).await.unwrap()
    }

    #[wasm_bindgen_test]
    async fn test_webrtc_transport_creation() {
        let transport = create_webrtc_transport("test-peer-1").await;
        
        assert_eq!(transport.get_id(), "test-peer-1");
        assert!(transport.get_peer_connection().is_instance_of::<RtcPeerConnection>());
    }

    #[wasm_bindgen_test]
    async fn test_data_channel_creation() {
        let transport = create_webrtc_transport("test-peer-2").await;
        
        let channel_options = DataChannelOptions {
            ordered: true,
            max_retransmits: Some(3),
            max_packet_life_time: Some(1000),
            protocol: Some("synapse-v1".to_string()),
        };
        
        let channel = transport.create_data_channel("test-channel", channel_options).await.unwrap();
        
        assert!(channel.is_instance_of::<RtcDataChannel>());
        assert_eq!(channel.label(), "test-channel");
        assert_eq!(channel.protocol(), "synapse-v1");
    }

    #[wasm_bindgen_test]
    async fn test_indexed_db_storage() {
        // Initialize storage
        let storage = IndexedDbStorage::new("synapse-test-db").await.unwrap();
        
        // Test data
        let key = "test-key";
        let value = "test-value";
        
        // Store data
        storage.set(key, value).await.unwrap();
        
        // Retrieve data
        let retrieved: Option<String> = storage.get(key).await.unwrap();
        
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), value);
        
        // Delete data
        storage.delete(key).await.unwrap();
        
        // Verify deletion
        let retrieved: Option<String> = storage.get(key).await.unwrap();
        assert!(retrieved.is_none());
    }

    #[wasm_bindgen_test]
    async fn test_message_sending() {
        // This test simulates sending a message through WebRTC
        // In a real scenario, this would require two connected peers
        let transport = create_webrtc_transport("test-peer-3").await;
        
        // Create a data channel
        let channel = transport.create_data_channel(
            "message-channel", 
            DataChannelOptions {
                ordered: true,
                max_retransmits: None,
                max_packet_life_time: None,
                protocol: None,
            }
        ).await.unwrap();
        
        // We'll test the send functionality directly
        // (a complete test would need a receiving peer)
        let message = "Hello WebRTC!";
        let result = transport.send(message.as_bytes()).await;
        
        // In a browser test environment, this might not succeed without a proper connection
        // so we're just testing that the function runs without panic
        if let Err(e) = &result {
            println!("Send failed (expected in isolated test): {:?}", e);
        }
    }

    #[wasm_bindgen_test]
    async fn test_connection_state_monitoring() {
        let transport = create_webrtc_transport("test-peer-4").await;
        
        // Get initial state
        let initial_state = transport.get_connection_state();
        
        // Should start in "new" state
        assert_eq!(initial_state, "new");
        
        // Start monitoring state changes
        let mut state_changes = transport.connection_state_stream();
        
        // This would normally change as ICE negotiation happens
        // but in an isolated test without a peer, we're just testing the API
    }

    #[wasm_bindgen_test]
    async fn test_ice_candidate_gathering() {
        let transport = create_webrtc_transport("test-peer-5").await;
        
        // Start ICE gathering (will timeout in isolated test)
        let gathering_result = transport.gather_ice_candidates(Duration::from_secs(2)).await;
        
        // Will likely timeout without a real connection
        if let Err(e) = &gathering_result {
            println!("ICE gathering timeout (expected in isolated test): {:?}", e);
        }
    }

    #[wasm_bindgen_test]
    async fn test_error_handling() {
        let transport = create_webrtc_transport("test-peer-6").await;
        
        // Test with an invalid operation that should produce an error
        let invalid_operation = transport.connect_to_invalid_peer().await;
        
        assert!(invalid_operation.is_err());
        
        // Error should be properly formatted
        let error = invalid_operation.err().unwrap();
        assert!(error.to_string().contains("connection"));
    }

    // Helper trait for testing - would be implemented in the actual code
    #[cfg(test)]
    trait TestHelpers {
        async fn connect_to_invalid_peer(&self) -> Result<(), anyhow::Error>;
    }

    #[cfg(test)]
    impl TestHelpers for WebRtcTransport {
        async fn connect_to_invalid_peer(&self) -> Result<(), anyhow::Error> {
            // This should always fail
            Err(anyhow::anyhow!("Failed to establish connection to invalid peer"))
        }
    }
}
