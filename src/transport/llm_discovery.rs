//! LLM Discovery and Connection Manager for Synapse
//! 
//! This module provides specialized discovery and connection capabilities
//! for Large Language Models (LLMs) and AI services within the Synapse network.

use super::mdns_enhanced::{EnhancedMdnsServiceBrowser, ServiceRecord, BrowserConfig};
use crate::error::Result;
use serde::{Serialize, Deserialize};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
    sync::{Arc, RwLock},
};
use tracing::{info, debug, warn};

/// LLM Discovery Manager for finding and connecting to AI models
pub struct LlmDiscoveryManager {
    /// mDNS service browser for network discovery
    mdns_browser: EnhancedMdnsServiceBrowser,
    /// Cache of discovered LLMs
    llm_cache: Arc<RwLock<HashMap<String, DiscoveredLlm>>>,
    /// Discovery configuration
    config: LlmDiscoveryConfig,
}

/// Configuration for LLM discovery
#[derive(Debug, Clone)]
pub struct LlmDiscoveryConfig {
    /// How often to scan for new LLMs
    pub scan_interval: Duration,
    /// How long to cache LLM information
    pub cache_ttl: Duration,
    /// Preferred LLM capabilities
    pub preferred_capabilities: Vec<String>,
    /// Maximum number of LLMs to track
    pub max_llms: usize,
    /// Minimum required capabilities for an LLM to be considered
    pub required_capabilities: Vec<String>,
}

impl Default for LlmDiscoveryConfig {
    fn default() -> Self {
        Self {
            scan_interval: Duration::from_secs(30),
            cache_ttl: Duration::from_secs(600), // 10 minutes
            preferred_capabilities: vec![
                "conversation".to_string(),
                "reasoning".to_string(),
                "analysis".to_string(),
                "code_generation".to_string(),
            ],
            max_llms: 50,
            required_capabilities: vec!["conversation".to_string()],
        }
    }
}

/// Information about a discovered LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredLlm {
    /// Unique identifier for the LLM
    pub entity_id: String,
    /// Human-readable name
    pub display_name: String,
    /// Model information
    pub model_info: LlmModelInfo,
    /// Network connection details
    pub connection_info: LlmConnectionInfo,
    /// Capabilities this LLM provides
    pub capabilities: Vec<String>,
    /// Performance metrics
    pub performance_metrics: LlmPerformanceMetrics,
    /// Discovery timestamp
    #[serde(skip, default = "Instant::now")]
    pub discovered_at: Instant,
    /// Last seen timestamp
    #[serde(skip, default = "Instant::now")]
    pub last_seen: Instant,
    /// Availability status
    pub status: LlmStatus,
}

/// LLM model information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmModelInfo {
    /// Model name (e.g., "GPT-4", "Claude-3", "Llama-2")
    pub model_name: String,
    /// Model version
    pub model_version: String,
    /// Model size/parameters (if available)
    pub parameters: Option<String>,
    /// Supported languages
    pub languages: Vec<String>,
    /// Context window size
    pub context_window: Option<u32>,
    /// Model provider/organization
    pub provider: String,
    /// Model description
    pub description: String,
}

/// LLM connection information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConnectionInfo {
    /// Primary connection endpoint
    pub primary_endpoint: String,
    /// Alternative endpoints
    pub backup_endpoints: Vec<String>,
    /// Supported protocols
    pub protocols: Vec<String>,
    /// Authentication requirements
    pub auth_required: bool,
    /// API version
    pub api_version: String,
    /// Rate limiting information
    pub rate_limits: Option<RateLimitInfo>,
}

/// Rate limiting information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitInfo {
    /// Requests per minute
    pub requests_per_minute: u32,
    /// Tokens per minute
    pub tokens_per_minute: Option<u32>,
    /// Burst allowance
    pub burst_allowance: Option<u32>,
}

/// Performance metrics for an LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmPerformanceMetrics {
    /// Average response time in milliseconds
    pub avg_response_time_ms: f64,
    /// Success rate (0.0-1.0)
    pub success_rate: f64,
    /// Load factor (0.0-1.0, where 1.0 is fully loaded)
    pub load_factor: f64,
    /// Quality score (0.0-1.0, based on user feedback)
    pub quality_score: f64,
    /// Total requests handled
    pub total_requests: u64,
    /// Last performance update
    #[serde(skip, default = "Instant::now")]
    pub last_updated: Instant,
}

/// LLM availability status
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LlmStatus {
    /// LLM is available and accepting requests
    Available,
    /// LLM is busy but may accept requests
    Busy,
    /// LLM is at capacity
    AtCapacity,
    /// LLM is temporarily unavailable
    Unavailable,
    /// LLM is permanently offline
    Offline,
}

impl LlmDiscoveryManager {
    /// Create a new LLM discovery manager
    pub async fn new(config: Option<LlmDiscoveryConfig>) -> Result<Self> {
        let config = config.unwrap_or_default();
        
        // Create mDNS browser specifically for AI services
        let service_types = vec![
            "_llm._tcp.local.".to_string(),           // Generic LLM service
            "_openai._tcp.local.".to_string(),        // OpenAI API compatible
            "_anthropic._tcp.local.".to_string(),     // Anthropic Claude
            "_ollama._tcp.local.".to_string(),        // Ollama local models
            "_llamacpp._tcp.local.".to_string(),      // llama.cpp servers
            "_textgen._tcp.local.".to_string(),       // Text generation WebUI
            "_vllm._tcp.local.".to_string(),          // vLLM inference server
            "_synapse-ai._tcp.local.".to_string(),    // Synapse AI nodes
        ];
        
        let browser_config = BrowserConfig {
            browse_interval: config.scan_interval,
            cache_ttl: config.cache_ttl,
            max_cache_size: config.max_llms,
            continuous_monitoring: true,
        };
        
        let mdns_browser = EnhancedMdnsServiceBrowser::new(service_types, Some(browser_config)).await?;
        
        Ok(Self {
            mdns_browser,
            llm_cache: Arc::new(RwLock::new(HashMap::new())),
            config,
        })
    }
    
    /// Start discovering LLMs on the network
    pub async fn start_discovery(&self) -> Result<()> {
        info!("Starting LLM discovery on local network");
        
        // Start mDNS browsing
        self.mdns_browser.start_browsing().await?;
        
        // Start periodic cache updates
        self.start_cache_update_task().await;
        
        // Start performance monitoring
        self.start_performance_monitoring().await;
        
        Ok(())
    }
    
    /// Find all available LLMs
    pub async fn discover_llms(&self) -> Result<Vec<DiscoveredLlm>> {
        info!("Scanning for available LLMs");
        
        // Wait a moment for fresh discoveries
        tokio::time::sleep(Duration::from_secs(2)).await;
        
        // Get discovered services from mDNS
        let services = self.mdns_browser.get_discovered_services().await;
        let mut discovered_llms = Vec::new();
        
        for service in services {
            if let Ok(llm) = self.service_to_llm(service).await {
                if self.meets_requirements(&llm) {
                    discovered_llms.push(llm);
                }
            }
        }
        
        // Update cache
        {
            let mut cache = self.llm_cache.write().unwrap();
            cache.clear();
            for llm in &discovered_llms {
                cache.insert(llm.entity_id.clone(), llm.clone());
            }
        }
        
        info!("Discovered {} compatible LLMs", discovered_llms.len());
        Ok(discovered_llms)
    }
    
    /// Find LLMs with specific capabilities
    pub async fn find_llms_with_capabilities(&self, required_caps: &[String]) -> Result<Vec<DiscoveredLlm>> {
        let all_llms = self.get_cached_llms().await;
        
        let matching_llms: Vec<DiscoveredLlm> = all_llms
            .into_iter()
            .filter(|llm| {
                required_caps.iter().all(|cap| llm.capabilities.contains(cap))
            })
            .collect();
        
        debug!("Found {} LLMs with required capabilities: {:?}", matching_llms.len(), required_caps);
        Ok(matching_llms)
    }
    
    /// Find the best LLM for a specific task
    pub async fn find_best_llm(&self, task_type: &str) -> Result<Option<DiscoveredLlm>> {
        let all_llms = self.get_cached_llms().await;
        
        let task_capabilities = self.get_capabilities_for_task(task_type);
        let compatible_llms: Vec<DiscoveredLlm> = all_llms
            .into_iter()
            .filter(|llm| {
                llm.status == LlmStatus::Available &&
                task_capabilities.iter().any(|cap| llm.capabilities.contains(cap))
            })
            .collect();
        
        if compatible_llms.is_empty() {
            return Ok(None);
        }
        
        // Score LLMs based on performance and suitability
        let mut scored_llms: Vec<(DiscoveredLlm, f64)> = compatible_llms
            .into_iter()
            .map(|llm| {
                let score = self.calculate_llm_score(&llm, &task_capabilities);
                (llm, score)
            })
            .collect();
        
        // Sort by score (highest first)
        scored_llms.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        
        Ok(scored_llms.into_iter().next().map(|(llm, _)| llm))
    }
    
    /// Get all cached LLMs
    pub async fn get_cached_llms(&self) -> Vec<DiscoveredLlm> {
        self.llm_cache.read().unwrap().values().cloned().collect()
    }
    
    /// Get a specific LLM by entity ID
    pub async fn get_llm_by_id(&self, entity_id: &str) -> Option<DiscoveredLlm> {
        self.llm_cache.read().unwrap().get(entity_id).cloned()
    }
    
    /// Connect to a specific LLM
    pub async fn connect_to_llm(&self, llm: &DiscoveredLlm) -> Result<LlmConnection> {
        info!("Connecting to LLM: {} ({})", llm.display_name, llm.entity_id);
        
        // Try primary endpoint first
        match self.try_connect_endpoint(&llm.connection_info.primary_endpoint).await {
            Ok(connection) => {
                info!("Successfully connected to LLM via primary endpoint");
                return Ok(connection);
            }
            Err(e) => {
                warn!("Failed to connect via primary endpoint: {}", e);
            }
        }
        
        // Try backup endpoints
        for endpoint in &llm.connection_info.backup_endpoints {
            match self.try_connect_endpoint(endpoint).await {
                Ok(connection) => {
                    info!("Successfully connected to LLM via backup endpoint: {}", endpoint);
                    return Ok(connection);
                }
                Err(e) => {
                    warn!("Failed to connect via backup endpoint {}: {}", endpoint, e);
                }
            }
        }
        
        Err(crate::error::EmrpError::Transport(
            format!("Failed to connect to LLM {}", llm.entity_id)
        ).into())
    }
    
    // Private implementation methods
    
    async fn service_to_llm(&self, service: ServiceRecord) -> Result<DiscoveredLlm> {
        let txt_records = &service.txt_records;
        
        // Extract LLM information from TXT records
        let model_name = txt_records.get("model_name")
            .cloned()
            .unwrap_or_else(|| "Unknown Model".to_string());
        
        let model_version = txt_records.get("model_version")
            .cloned()
            .unwrap_or_else(|| "1.0".to_string());
        
        let provider = txt_records.get("provider")
            .cloned()
            .unwrap_or_else(|| "Unknown Provider".to_string());
        
        let capabilities = txt_records.get("capabilities")
            .map(|caps| caps.split(',').map(|s| s.trim().to_string()).collect())
            .unwrap_or_else(|| vec!["conversation".to_string()]);
        
        let parameters = txt_records.get("parameters").cloned();
        let context_window = txt_records.get("context_window")
            .and_then(|s| s.parse().ok());
        
        let languages = txt_records.get("languages")
            .map(|langs| langs.split(',').map(|s| s.trim().to_string()).collect())
            .unwrap_or_else(|| vec!["en".to_string()]);
        
        let api_version = txt_records.get("api_version")
            .cloned()
            .unwrap_or_else(|| "v1".to_string());
        
        let auth_required = txt_records.get("auth_required")
            .map(|s| s.parse().unwrap_or(false))
            .unwrap_or(false);
        
        // Extract performance metrics
        let avg_response_time = txt_records.get("avg_response_time")
            .and_then(|s| s.parse().ok())
            .unwrap_or(1000.0);
        
        let success_rate = txt_records.get("success_rate")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.95);
        
        let load_factor = txt_records.get("load_factor")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.5);
        
        // Determine status
        let status = txt_records.get("status")
            .and_then(|s| match s.as_str() {
                "available" => Some(LlmStatus::Available),
                "busy" => Some(LlmStatus::Busy),
                "at_capacity" => Some(LlmStatus::AtCapacity),
                "unavailable" => Some(LlmStatus::Unavailable),
                "offline" => Some(LlmStatus::Offline),
                _ => None,
            })
            .unwrap_or(LlmStatus::Available);
        
        let primary_endpoint = if service.addresses.is_empty() {
            format!("{}:{}", service.host_name, service.port)
        } else {
            format!("{}:{}", service.addresses[0], service.port)
        };
        
        Ok(DiscoveredLlm {
            entity_id: txt_records.get("entity_id")
                .cloned()
                .unwrap_or_else(|| service.service_name.clone()),
            display_name: txt_records.get("display_name")
                .cloned()
                .unwrap_or_else(|| model_name.clone()),
            model_info: LlmModelInfo {
                model_name,
                model_version,
                parameters,
                languages,
                context_window,
                provider,
                description: txt_records.get("description")
                    .cloned()
                    .unwrap_or_else(|| "AI Language Model".to_string()),
            },
            connection_info: LlmConnectionInfo {
                primary_endpoint,
                backup_endpoints: vec![], // Could be extracted from additional TXT records
                protocols: txt_records.get("protocols")
                    .map(|p| p.split(',').map(|s| s.trim().to_string()).collect())
                    .unwrap_or_else(|| vec!["http".to_string()]),
                auth_required,
                api_version,
                rate_limits: None, // Could be extracted if provided
            },
            capabilities,
            performance_metrics: LlmPerformanceMetrics {
                avg_response_time_ms: avg_response_time,
                success_rate,
                load_factor,
                quality_score: 0.8, // Default value
                total_requests: 0,
                last_updated: Instant::now(),
            },
            discovered_at: service.discovered_at,
            last_seen: service.last_updated,
            status,
        })
    }
    
    fn meets_requirements(&self, llm: &DiscoveredLlm) -> bool {
        self.config.required_capabilities
            .iter()
            .all(|req| llm.capabilities.contains(req))
    }
    
    fn get_capabilities_for_task(&self, task_type: &str) -> Vec<String> {
        match task_type {
            "conversation" => vec!["conversation".to_string()],
            "code_generation" => vec!["code_generation".to_string(), "reasoning".to_string()],
            "analysis" => vec!["analysis".to_string(), "reasoning".to_string()],
            "translation" => vec!["translation".to_string(), "multilingual".to_string()],
            "summarization" => vec!["summarization".to_string(), "analysis".to_string()],
            "creative_writing" => vec!["creative_writing".to_string(), "conversation".to_string()],
            "math" => vec!["mathematical_reasoning".to_string(), "reasoning".to_string()],
            _ => vec!["conversation".to_string()],
        }
    }
    
    fn calculate_llm_score(&self, llm: &DiscoveredLlm, task_capabilities: &[String]) -> f64 {
        let mut score = 0.0;
        
        // Performance factors
        score += llm.performance_metrics.success_rate * 0.3;
        score += (1.0 - llm.performance_metrics.load_factor) * 0.2; // Lower load is better
        score += llm.performance_metrics.quality_score * 0.2;
        
        // Response time factor (lower is better)
        let response_time_score = (5000.0 - llm.performance_metrics.avg_response_time_ms.min(5000.0)) / 5000.0;
        score += response_time_score * 0.1;
        
        // Capability matching
        let capability_match = task_capabilities.iter()
            .filter(|cap| llm.capabilities.contains(cap))
            .count() as f64 / task_capabilities.len() as f64;
        score += capability_match * 0.2;
        
        score.min(1.0)
    }
    
    async fn try_connect_endpoint(&self, endpoint: &str) -> Result<LlmConnection> {
        // This would implement the actual connection logic
        // For now, return a mock connection
        Ok(LlmConnection {
            endpoint: endpoint.to_string(),
            connected_at: Instant::now(),
        })
    }
    
    async fn start_cache_update_task(&self) {
        let llm_cache = Arc::clone(&self.llm_cache);
        let _browser = &self.mdns_browser;
        let cache_ttl = self.config.cache_ttl;
        
        // Clone the browser for the task (this would need proper Arc handling in real implementation)
        tokio::spawn(async move {
            let mut update_interval = tokio::time::interval(Duration::from_secs(60));
            
            loop {
                update_interval.tick().await;
                
                // Remove stale entries
                let now = Instant::now();
                let mut cache = llm_cache.write().unwrap();
                cache.retain(|_, llm| now.duration_since(llm.last_seen) < cache_ttl);
            }
        });
    }
    
    async fn start_performance_monitoring(&self) {
        // This would implement periodic performance checks
        tokio::spawn(async move {
            let mut monitor_interval = tokio::time::interval(Duration::from_secs(300)); // 5 minutes
            
            loop {
                monitor_interval.tick().await;
                // Implement performance monitoring logic
                debug!("Running LLM performance monitoring");
            }
        });
    }
}

/// Active connection to an LLM
pub struct LlmConnection {
    pub endpoint: String,
    pub connected_at: Instant,
}

impl LlmConnection {
    /// Send a message to the connected LLM
    pub async fn send_message(&self, message: &str) -> Result<String> {
        // This would implement the actual LLM communication
        info!("Sending message to LLM at {}: {}", self.endpoint, message);
        
        // Mock response for now
        Ok(format!("Response from LLM at {}: Processed '{}'", self.endpoint, message))
    }
    
    /// Send a structured request to the LLM
    pub async fn send_request(&self, request: LlmRequest) -> Result<LlmResponse> {
        // This would implement structured LLM communication
        info!("Sending structured request to LLM: {:?}", request);
        
        // Mock response
        Ok(LlmResponse {
            content: format!("Processed request: {}", request.prompt),
            metadata: LlmResponseMetadata {
                model_used: "unknown".to_string(),
                tokens_used: 100,
                processing_time_ms: 500,
                confidence_score: 0.95,
            },
        })
    }
}

/// Request to an LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRequest {
    pub prompt: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub system_prompt: Option<String>,
    pub metadata: HashMap<String, String>,
}

/// Response from an LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub content: String,
    pub metadata: LlmResponseMetadata,
}

/// Metadata about an LLM response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponseMetadata {
    pub model_used: String,
    pub tokens_used: u32,
    pub processing_time_ms: u64,
    pub confidence_score: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_llm_discovery_creation() {
        let discovery = LlmDiscoveryManager::new(None).await;
        assert!(discovery.is_ok());
    }
    
    #[tokio::test]
    async fn test_capability_scoring() -> crate::error::Result<()> {
        let llm = DiscoveredLlm {
            entity_id: "test-llm".to_string(),
            display_name: "Test LLM".to_string(),
            model_info: LlmModelInfo {
                model_name: "TestModel".to_string(),
                model_version: "1.0".to_string(),
                parameters: None,
                languages: vec!["en".to_string()],
                context_window: Some(4096),
                provider: "Test Provider".to_string(),
                description: "Test model".to_string(),
            },
            connection_info: LlmConnectionInfo {
                primary_endpoint: "localhost:8080".to_string(),
                backup_endpoints: vec![],
                protocols: vec!["http".to_string()],
                auth_required: false,
                api_version: "v1".to_string(),
                rate_limits: None,
            },
            capabilities: vec!["conversation".to_string(), "reasoning".to_string()],
            performance_metrics: LlmPerformanceMetrics {
                avg_response_time_ms: 500.0,
                success_rate: 0.95,
                load_factor: 0.3,
                quality_score: 0.8,
                total_requests: 0,
                last_updated: Instant::now(),
            },
            discovered_at: Instant::now(),
            last_seen: Instant::now(),
            status: LlmStatus::Available,
        };
        
        let config = LlmDiscoveryConfig::default();
        let discovery = LlmDiscoveryManager {
            mdns_browser: crate::transport::mdns_enhanced::EnhancedMdnsServiceBrowser::new(
                vec!["_test._tcp".to_string()],
                None,
            ).await?,
            llm_cache: Arc::new(RwLock::new(HashMap::new())),
            config,
        };
        
        let task_capabilities = vec!["conversation".to_string()];
        let score = discovery.calculate_llm_score(&llm, &task_capabilities);
        
        assert!(score > 0.0 && score <= 1.0);
        Ok(())
    }
}
