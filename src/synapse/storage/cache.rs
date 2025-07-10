// Synapse Cache Layer
// Redis-based caching for performance optimization

use anyhow::{Context, Result};
#[cfg(feature = "cache")]
use redis::{AsyncCommands, Client};
use serde::{Deserialize, Serialize};

/// Cache interface for Synapse
#[cfg(feature = "cache")]
pub struct Cache {
    client: Client,
}

#[cfg(feature = "cache")]
impl Cache {
    /// Create new cache connection
    pub async fn new(redis_url: &str) -> Result<Self> {
        let client = Client::open(redis_url)
            .context("Failed to create Redis client")?;
        
        // Test connection by trying to set a test key
        let mut conn = client.get_multiplexed_async_connection().await
            .context("Failed to connect to Redis")?;
        
        let _: () = conn.set("test_connection", "ok").await
            .context("Failed to test Redis connection")?;
        
        let _: () = conn.del("test_connection").await
            .context("Failed to clean up test key")?;
        
        Ok(Self { client })
    }
    
    /// Cache a participant profile
    pub async fn cache_participant<T>(&self, key: &str, value: &T, ttl_seconds: u64) -> Result<()>
    where
        T: Serialize + ?Sized,
    {
        let mut conn = self.client.get_multiplexed_async_connection().await
            .context("Failed to get Redis connection")?;
        
        let serialized = serde_json::to_string(value)
            .context("Failed to serialize value")?;
        
        let _: () = conn.set_ex(key, serialized, ttl_seconds).await
            .context("Failed to cache value")?;
        
        Ok(())
    }
    
    /// Retrieve a cached value
    pub async fn get_cached<T>(&self, key: &str) -> Result<Option<T>>
    where
        T: for<'de> Deserialize<'de>,
    {
        let mut conn = self.client.get_multiplexed_async_connection().await
            .context("Failed to get Redis connection")?;
        
        let cached: Option<String> = conn.get(key).await
            .context("Failed to get cached value")?;
        
        match cached {
            Some(ref serialized) => {
                let value = serde_json::from_str(serialized)
                    .context("Failed to deserialize cached value")?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// Alias for get_cached - for compatibility with discovery service
    pub async fn get_participant<T>(&self, key: &str) -> Result<Option<T>>
    where
        T: for<'de> Deserialize<'de>,
    {
        self.get_cached(key).await
    }
    
    /// Invalidate cache entry
    pub async fn invalidate(&self, key: &str) -> Result<()> {
        let mut conn = self.client.get_multiplexed_async_connection().await
            .context("Failed to get Redis connection")?;
        
        conn.del::<_, ()>(key).await
            .context("Failed to delete cache key")?;
        
        Ok(())
    }
    
    /// Cache search results
    pub async fn cache_search_results(&self, query_hash: &str, results: &[String], ttl_seconds: u64) -> Result<()> {
        let key = format!("search:{}", query_hash);
        self.cache_participant(&key, results, ttl_seconds).await
    }
    
    /// Get cached search results
    pub async fn get_cached_search_results(&self, query_hash: &str) -> Result<Option<Vec<String>>> {
        let key = format!("search:{}", query_hash);
        self.get_cached(&key).await
    }
    
    /// Cache trust calculation
    pub async fn cache_trust_score(&self, participant_id: &str, score: f64, ttl_seconds: u64) -> Result<()> {
        let key = format!("trust_score:{}", participant_id);
        self.cache_participant(&key, &score, ttl_seconds).await
    }
    
    /// Get cached trust score
    pub async fn get_cached_trust_score(&self, participant_id: &str) -> Result<Option<f64>> {
        let key = format!("trust_score:{}", participant_id);
        self.get_cached(&key).await
    }
    
    /// Increment rate limiting counter
    pub async fn increment_rate_limit(&self, key: &str, window_seconds: u64) -> Result<u64> {
        let mut conn = self.client.get_multiplexed_async_connection().await
            .context("Failed to get Redis connection")?;
        
        let count: u64 = conn.incr(&key, 1).await
            .context("Failed to increment counter")?;
        
        if count == 1 {
            let _: () = conn.expire(&key, window_seconds as i64).await
                .context("Failed to set expiration")?;
        }
        
        Ok(count)
    }
    
    /// Check if rate limit exceeded
    pub async fn is_rate_limited(&self, key: &str, max_requests: u64) -> Result<bool> {
        let mut conn = self.client.get_multiplexed_async_connection().await
            .context("Failed to get Redis connection")?;
        
        let count: Option<u64> = conn.get(&key).await
            .context("Failed to get rate limit counter")?;
        
        Ok(count.unwrap_or(0) >= max_requests)
    }
    
    /// Store blockchain block hash for verification
    pub async fn cache_block_hash(&self, block_number: u64, hash: &str) -> Result<()> {
        let key = format!("block:{}:hash", block_number);
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        
        // Store with no expiration - blocks are immutable
        conn.set::<_, _, ()>(&key, hash).await
            .context("Failed to cache block hash")?;
        
        Ok(())
    }
    
    /// Get cached block hash
    pub async fn get_block_hash(&self, block_number: u64) -> Result<Option<String>> {
        let key = format!("block:{}:hash", block_number);
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        
        conn.get(&key).await
            .context("Failed to get cached block hash")
    }
}
