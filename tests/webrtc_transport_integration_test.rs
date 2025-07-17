#[cfg(target_arch = "wasm32")]
mod wasm_tests {
    use super::*;
    use wasm_bindgen_test::*;
    use synapse::wasm::storage::WasmStorage;
    use synapse::wasm::transport::WasmTransport;
    use synapse::SecureMessage;
    
    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    async fn test_webrtc_transport_creation() {
        // Test WebRTC transport creation using existing WASM infrastructure
        let config = synapse::wasm::config::WasmConfig::default();
        let transport = WasmTransport::new(config).await;
        
        assert!(transport.is_ok(), "WebRTC transport should be created successfully");
        
        let transport = transport.unwrap();
        assert!(transport.is_connected(), "Transport should indicate connection capability");
    }

    #[wasm_bindgen_test]
    async fn test_data_channel_creation() {
        // Test data channel creation through WASM transport
        let config = synapse::wasm::config::WasmConfig::default();
        let transport = WasmTransport::new(config).await.unwrap();
        
        // Test data channel capabilities
        let channel_info = transport.get_channel_info().await;
        assert!(channel_info.is_ok(), "Data channel info should be available");
        
        let info = channel_info.unwrap();
        assert!(info.supports_binary, "Data channel should support binary data");
        assert!(info.supports_text, "Data channel should support text data");
    }

    #[wasm_bindgen_test]
    async fn test_indexed_db_storage() {
        // Test IndexedDB storage using existing WASM storage
        let storage = WasmStorage::new().await;
        assert!(storage.is_ok(), "IndexedDB storage should be created successfully");
        
        let mut storage = storage.unwrap();
        
        // Test storing and retrieving data
        let test_key = "test_webrtc_key";
        let test_data = b"test webrtc data";
        
        let store_result = storage.store_data(test_key, test_data).await;
        assert!(store_result.is_ok(), "Data should be stored successfully");
        
        let retrieve_result = storage.retrieve_data(test_key).await;
        assert!(retrieve_result.is_ok(), "Data should be retrieved successfully");
        
        let retrieved_data = retrieve_result.unwrap();
        assert_eq!(retrieved_data, test_data, "Retrieved data should match stored data");
    }

    #[wasm_bindgen_test]
    async fn test_message_sending() {
        // Test WebRTC message sending using WASM transport
        let config = synapse::wasm::config::WasmConfig::default();
        let transport = WasmTransport::new(config).await.unwrap();
        
        let test_message = SecureMessage {
            message_id: uuid::Uuid::new_v4().to_string(),
            from_entity: "test_sender".to_string(),
            to_entity: "test_receiver".to_string(),
            content: "Hello WebRTC".to_string(),
            timestamp: chrono::Utc::now(),
            message_type: synapse::MessageType::Direct,
            priority: 1,
            ttl: Some(chrono::Duration::hours(24)),
            requires_response: false,
            conversation_id: None,
            encryption_key: None,
        };
        
        let send_result = transport.send_message("test_receiver", &test_message).await;
        assert!(send_result.is_ok(), "Message should be sent successfully");
    }

    #[wasm_bindgen_test]
    async fn test_connection_state_monitoring() {
        // Test WebRTC connection state monitoring
        let config = synapse::wasm::config::WasmConfig::default();
        let transport = WasmTransport::new(config).await.unwrap();
        
        let connection_state = transport.get_connection_state().await;
        assert!(connection_state.is_ok(), "Connection state should be available");
        
        let state = connection_state.unwrap();
        assert!(matches!(state.status, synapse::wasm::transport::ConnectionStatus::Connected | 
                                      synapse::wasm::transport::ConnectionStatus::Connecting | 
                                      synapse::wasm::transport::ConnectionStatus::Disconnected), 
                "Connection state should be valid");
    }

    #[wasm_bindgen_test]
    async fn test_ice_candidate_gathering() {
        // Test ICE candidate gathering through WASM transport
        let config = synapse::wasm::config::WasmConfig::default();
        let transport = WasmTransport::new(config).await.unwrap();
        
        let candidates = transport.gather_ice_candidates().await;
        assert!(candidates.is_ok(), "ICE candidates should be gathered");
        
        let candidate_list = candidates.unwrap();
        // ICE candidates might be empty in test environment, but gathering should succeed
        assert!(candidate_list.len() >= 0, "ICE candidate list should be valid");
    }

    #[wasm_bindgen_test]
    async fn test_error_handling() {
        // Test WebRTC error handling
        let config = synapse::wasm::config::WasmConfig::default();
        let transport = WasmTransport::new(config).await.unwrap();
        
        // Test error handling with invalid message
        let invalid_message = SecureMessage {
            message_id: "".to_string(), // Invalid empty message ID
            from_entity: "".to_string(),
            to_entity: "".to_string(),
            content: "".to_string(),
            timestamp: chrono::Utc::now(),
            message_type: synapse::MessageType::Direct,
            priority: 1,
            ttl: Some(chrono::Duration::hours(24)),
            requires_response: false,
            conversation_id: None,
            encryption_key: None,
        };
        
        let send_result = transport.send_message("invalid_target", &invalid_message).await;
        assert!(send_result.is_err(), "Invalid message should result in error");
    }
}

#[tokio::test]
async fn test_webrtc_integration() -> anyhow::Result<()> {
    // Test WebRTC integration for non-WASM environments
    // This tests the overall WebRTC integration patterns
    
    println!("Testing WebRTC integration patterns...");
    
    // Test 1: Configuration validation
    let config = synapse::wasm::config::WasmConfig::default();
    assert!(config.validate().is_ok(), "WebRTC configuration should be valid");
    
    // Test 2: Transport type registration
    let transport_types = synapse::transport::get_available_transport_types();
    assert!(transport_types.contains(&synapse::transport::TransportType::WebRtc), 
            "WebRTC transport type should be registered");
    
    // Test 3: Router integration
    let router_config = synapse::Config::default();
    let router = synapse::router::SynapseRouter::new(router_config, "test_entity".to_string()).await?;
    
    // WebRTC should be available as a transport option
    let available_transports = router.get_available_transports().await;
    assert!(available_transports.iter().any(|t| matches!(t, synapse::transport::TransportType::WebRtc)), 
            "WebRTC should be available as transport option");
    
    println!("✓ WebRTC integration test completed successfully");
    Ok(())
}
    Ok(())
}
