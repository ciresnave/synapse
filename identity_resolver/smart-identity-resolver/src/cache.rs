//! Cache module for storing and retrieving identity resolution results
//! 
//! This module provides caching functionality to improve performance by storing
//! previously resolved identities and their associated metadata.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use crate::types::{Contact, ContactHint};
use crate::config::CacheConfig;
use crate::error::Result;

/// Cache entry with TTL support
#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub contact: Contact,
    pub created_at: Instant,
    pub ttl: Duration,
}

impl CacheEntry {
    /// Check if the cache entry has expired
    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() > self.ttl
    }
}

/// Cache statistics for monitoring and debugging
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub entries: usize,
}

impl CacheStats {
    /// Calculate cache hit ratio
    pub fn hit_ratio(&self) -> f64 {
        if self.hits + self.misses == 0 {
            0.0
        } else {
            self.hits as f64 / (self.hits + self.misses) as f64
        }
    }
}

/// Identity cache manager
pub struct CacheManager {
    cache: HashMap<String, CacheEntry>,
    config: CacheConfig,
    stats: CacheStats,
}

impl CacheManager {
    /// Create a new cache manager with the given configuration
    pub fn new(config: CacheConfig) -> Self {
        Self {
            cache: HashMap::new(),
            config,
            stats: CacheStats::default(),
        }
    }

    /// Get a contact from cache if available and not expired
    pub fn get(&mut self, key: &str) -> Option<Contact> {
        if !self.config.enabled {
            return None;
        }

        match self.cache.get(key) {
            Some(entry) if !entry.is_expired() => {
                self.stats.hits += 1;
                Some(entry.contact.clone())
            }
            Some(_) => {
                // Entry exists but expired, remove it
                self.cache.remove(key);
                self.stats.evictions += 1;
                self.stats.misses += 1;
                None
            }
            None => {
                self.stats.misses += 1;
                None
            }
        }
    }

    /// Store a contact in cache with TTL
    pub fn put(&mut self, key: String, contact: Contact) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        // Check if we need to evict entries to stay within size limit
        if self.cache.len() >= self.config.max_entries {
            self.evict_expired();
            
            // If still at limit, evict oldest entry
            if self.cache.len() >= self.config.max_entries {
                self.evict_oldest();
            }
        }

        let entry = CacheEntry {
            contact,
            created_at: Instant::now(),
            ttl: self.config.default_ttl,
        };

        self.cache.insert(key, entry);
        self.update_stats();
        
        Ok(())
    }

    /// Generate cache key from contact hint
    pub fn generate_key(&self, hint: &ContactHint) -> String {
        match hint {
            ContactHint::Email(email) => format!("email:{}", email.to_lowercase()),
            ContactHint::Phone(phone) => format!("phone:{}", phone),
            ContactHint::Username(username) => format!("username:{}", username.to_lowercase()),
            ContactHint::Name(name) => format!("name:{}", name.to_lowercase()),
            ContactHint::Domain(domain) => format!("domain:{}", domain.to_lowercase()),
        }
    }

    /// Clear all cache entries
    pub fn clear(&mut self) {
        self.cache.clear();
        self.update_stats();
    }

    /// Remove expired entries from cache
    pub fn evict_expired(&mut self) {
        let before_count = self.cache.len();
        self.cache.retain(|_, entry| !entry.is_expired());
        let evicted = before_count - self.cache.len();
        self.stats.evictions += evicted as u64;
        self.update_stats();
    }

    /// Remove the oldest entry from cache
    fn evict_oldest(&mut self) {
        if let Some((oldest_key, _)) = self.cache
            .iter()
            .min_by_key(|(_, entry)| entry.created_at)
            .map(|(k, v)| (k.clone(), v.clone()))
        {
            self.cache.remove(&oldest_key);
            self.stats.evictions += 1;
        }
        self.update_stats();
    }

    /// Update cache statistics
    fn update_stats(&mut self) {
        self.stats.entries = self.cache.len();
    }

    /// Get current cache statistics
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }

    /// Check if cache is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Get cache configuration
    pub fn config(&self) -> &CacheConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Contact;
    use std::thread;
    use std::time::Duration;

    fn create_test_contact() -> Contact {
        Contact {
            id: "test-id".to_string(),
            primary_email: Some("test@example.com".to_string()),
            display_name: Some("Test User".to_string()),
            verified: true,
            last_seen: None,
            created_at: chrono::Utc::now(),
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn test_cache_basic_operations() {
        let config = CacheConfig {
            enabled: true,
            max_entries: 100,
            default_ttl: Duration::from_secs(300),
        };
        let mut cache = CacheManager::new(config);
        let contact = create_test_contact();

        // Test put and get
        cache.put("test-key".to_string(), contact.clone()).unwrap();
        let retrieved = cache.get("test-key");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, contact.id);

        // Test cache miss
        let missing = cache.get("nonexistent");
        assert!(missing.is_none());
    }

    #[test]
    fn test_cache_expiration() {
        let config = CacheConfig {
            enabled: true,
            max_entries: 100,
            default_ttl: Duration::from_millis(10),
        };
        let mut cache = CacheManager::new(config);
        let contact = create_test_contact();

        cache.put("test-key".to_string(), contact).unwrap();
        
        // Sleep longer than TTL
        thread::sleep(Duration::from_millis(20));
        
        let retrieved = cache.get("test-key");
        assert!(retrieved.is_none());
    }

    #[test]
    fn test_cache_disabled() {
        let config = CacheConfig {
            enabled: false,
            max_entries: 100,
            default_ttl: Duration::from_secs(300),
        };
        let mut cache = CacheManager::new(config);
        let contact = create_test_contact();

        cache.put("test-key".to_string(), contact).unwrap();
        let retrieved = cache.get("test-key");
        assert!(retrieved.is_none()); // Should be None because cache is disabled
    }

    #[test]
    fn test_key_generation() {
        let config = CacheConfig::default();
        let cache = CacheManager::new(config);

        let email_key = cache.generate_key(&ContactHint::Email("Test@Example.com".to_string()));
        assert_eq!(email_key, "email:test@example.com");

        let name_key = cache.generate_key(&ContactHint::Name("John Doe".to_string()));
        assert_eq!(name_key, "name:john doe");
    }
}
