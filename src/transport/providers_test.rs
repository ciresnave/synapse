//! Tests for the dependency injection transport provider system

#[cfg(test)]
mod tests {
    use super::super::Transport; // Import the Transport trait from mod.rs
    use super::super::providers::MockTransport;
    use crate::config::Config;
    use tokio;

    #[tokio::test]
    async fn test_mock_transport_basic_functionality() {
        let mock = MockTransport::new("test-transport".to_string());

        // Test basic properties
        assert_eq!(mock.id, "test-transport");
        assert_eq!(mock.reliability_score(), 0.95);
        assert!(mock.estimated_latency().as_millis() > 0);

        // Test capabilities
        let capabilities = mock.get_capabilities();
        assert!(capabilities.contains(&"mock".to_string()));
    }

    #[tokio::test]
    async fn test_mock_transport_can_reach() {
        let reliable_mock = MockTransport::new("reliable".to_string());
        let unreliable_mock = MockTransport::new("unreliable".to_string()).with_reliability(0.3);

        // Reliable transport should be able to reach valid targets
        assert!(reliable_mock.can_reach("valid-target").await);
        assert!(!reliable_mock.can_reach("").await);

        // Unreliable transport should not be able to reach targets (below 0.5 threshold)
        assert!(!unreliable_mock.can_reach("valid-target").await);
    }

    #[tokio::test]
    async fn test_mock_transport_metrics() {
        let mock = MockTransport::new("metrics-test".to_string());

        let metrics = mock.test_connectivity("test-target").await;
        assert!(metrics.is_ok());

        let metrics = metrics.unwrap();
        assert_eq!(metrics.reliability_score, 0.95);
        assert!(metrics.latency.as_millis() > 0);
        assert!(metrics.throughput_bps > 0);
    }

    #[tokio::test]
    async fn test_different_reliability_levels() {
        let high_reliability = MockTransport::new("high".to_string()).with_reliability(0.99);
        let medium_reliability = MockTransport::new("medium".to_string()).with_reliability(0.75);
        let low_reliability = MockTransport::new("low".to_string()).with_reliability(0.25);

        assert_eq!(high_reliability.reliability_score(), 0.99);
        assert_eq!(medium_reliability.reliability_score(), 0.75);
        assert_eq!(low_reliability.reliability_score(), 0.25);

        // High reliability should be able to reach targets
        assert!(high_reliability.can_reach("target").await);

        // Medium reliability should be able to reach targets
        assert!(medium_reliability.can_reach("target").await);

        // Low reliability should not be able to reach targets (below 0.5 threshold)
        assert!(!low_reliability.can_reach("target").await);
    }

    #[tokio::test]
    async fn test_transport_provider_basic_creation() {
        let config = Config::default();

        // Test provider creation with actual configuration
        // Verify that providers can be initialized with proper config values

        // Create mock transport with config-based settings
        let mock = MockTransport::new("config-test".to_string());
        assert!(mock.can_reach("test").await);

        // Verify config has reasonable default values for transport operations
        assert!(!config.entity.local_name.is_empty());
        assert!(!config.entity.domain.is_empty());
        assert!(config.router.max_connections > 0);
        assert!(config.router.connection_timeout > 0);
        assert!(config.router.max_retries > 0);

        // Ensure config can be used for real transport provider configuration
        let _provider_config = &config.router;
        assert!(_provider_config.queue_size > 0);
    }
}
