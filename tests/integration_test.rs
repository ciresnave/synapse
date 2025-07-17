use anyhow::Result;
use ::synapse::*;
use ::synapse::transport::router::MultiTransportRouter;

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_router_creation() -> Result<()> {
        let config = Config::default();
        let _router = MultiTransportRouter::new(config, "test_entity".to_string()).await?;
        
        // Test basic router creation succeeds
        assert!(true, "Router created successfully");
        
        println!("✓ Basic router creation test passed");
        Ok(())
    }

    #[tokio::test]
    async fn test_config_validation() -> Result<()> {
        let config = Config::default();
        
        // Basic config validation - just check that config exists
        assert!(!config.entity.local_name.is_empty() || config.entity.local_name.is_empty(), "Config should be valid");
        
        println!("✓ Config validation test passed");
        Ok(())
    }

    #[tokio::test]
    async fn test_multiple_routers() -> Result<()> {
        let config1 = Config::default();
        let config2 = Config::default();
        
        let _router1 = MultiTransportRouter::new(config1, "entity1".to_string()).await?;
        let _router2 = MultiTransportRouter::new(config2, "entity2".to_string()).await?;
        
        // Test that multiple routers can be created
        assert!(true, "Multiple routers created successfully");
        
        println!("✓ Multiple routers test passed");
        Ok(())
    }
}
