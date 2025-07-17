#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_discovery_transport_initialization() {
        let config = DiscoveryConfig {
            service_name: "test-service".to_string(),
            port: 8080,
            discovery_interval: Duration::from_secs(60),
            protocols: vec!["mdns".to_string(), "ssdp".to_string()],
        };

        let transport = DiscoveryTransport::new(config).await;
        assert!(transport.is_ok());
    }

    #[tokio::test]
    async fn test_discovery_transport_capabilities() {
        let config = DiscoveryConfig {
            service_name: "test-service".to_string(),
            port: 8080,
            discovery_interval: Duration::from_secs(60),
            protocols: vec!["mdns".to_string()],
        };

        let transport = DiscoveryTransport::new(config).await.unwrap();
        let capabilities = transport.get_capabilities().await;

        assert_eq!(capabilities.transport_type, TransportType::AutoDiscovery);
        assert!(capabilities.supports_broadcast);
        assert!(capabilities.supports_multicast);
        assert!(capabilities.supports_direct);
    }

    #[tokio::test]
    async fn test_discovery_transport_lifecycle() {
        let config = DiscoveryConfig {
            service_name: "test-service".to_string(),
            port: 8080,
            discovery_interval: Duration::from_secs(60),
            protocols: vec!["mdns".to_string()],
        };

        let mut transport = DiscoveryTransport::new(config).await.unwrap();
        
        // Test start
        assert!(transport.start().await.is_ok());
        
        // Give it time to discover
        tokio::time::sleep(Duration::from_secs(1)).await;
        
        // Test stop
        assert!(transport.stop().await.is_ok());
    }
}
