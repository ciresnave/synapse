# LLM Discovery and Connection Guide

Synapse provides comprehensive support for discovering and connecting to Large Language Models (LLMs) on the local network using advanced mDNS-based service discovery.

## Overview

The LLM Discovery module (`src/transport/llm_discovery.rs`) enables Synapse to:

- **Automatically discover** LLM services on the local network
- **Filter LLMs** by capabilities and performance metrics  
- **Rank and select** the best LLM for specific tasks
- **Establish connections** and communicate with discovered LLMs
- **Monitor performance** and maintain service health

## Supported LLM Services

Synapse can discover and connect to various LLM providers through mDNS service discovery:

### Service Types Supported

- `_llm._tcp.local.` - Generic LLM services
- `_openai._tcp.local.` - OpenAI API-compatible services
- `_anthropic._tcp.local.` - Anthropic Claude services
- `_ollama._tcp.local.` - Ollama local model services
- `_llamacpp._tcp.local.` - llama.cpp inference servers
- `_textgen._tcp.local.` - Text Generation WebUI services
- `_vllm._tcp.local.` - vLLM inference servers
- `_synapse-ai._tcp.local.` - Synapse AI nodes

### Popular LLM Services

1. **Ollama** - Local model management and inference
   ```bash
   ollama serve  # Starts on port 11434
   ```

2. **llama.cpp** - High-performance LLM inference
   ```bash
   ./server -m model.gguf --port 8080
   ```

3. **Text Generation WebUI** - Community LLM interface
   ```bash
   python server.py --api --listen
   ```

4. **vLLM** - High-throughput LLM serving
   ```bash
   python -m vllm.entrypoints.api_server --model model_name
   ```

## Usage Examples

### Basic LLM Discovery

```rust
use synapse::transport::{LlmDiscoveryManager, LlmDiscoveryConfig};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create discovery manager with default config
    let discovery = LlmDiscoveryManager::new(None).await?;
    
    // Start discovery process
    discovery.start_discovery().await?;
    
    // Wait for discovery
    tokio::time::sleep(Duration::from_secs(10)).await;
    
    // Get all discovered LLMs
    let llms = discovery.discover_llms().await?;
    
    for llm in llms {
        println!("Found LLM: {} ({})", llm.display_name, llm.model_info.model_name);
    }
    
    Ok(())
}
```

### Advanced Configuration

```rust
use synapse::transport::{LlmDiscoveryManager, LlmDiscoveryConfig};
use std::time::Duration;

let config = LlmDiscoveryConfig {
    scan_interval: Duration::from_secs(30),
    cache_ttl: Duration::from_secs(600), // 10 minutes
    preferred_capabilities: vec![
        "conversation".to_string(),
        "reasoning".to_string(),
        "code_generation".to_string(),
    ],
    max_llms: 50,
    required_capabilities: vec!["conversation".to_string()],
};

let discovery = LlmDiscoveryManager::new(Some(config)).await?;
```

### Capability-Based LLM Selection

```rust
// Find LLMs with specific capabilities
let code_llms = discovery
    .find_llms_with_capabilities(&["code_generation".to_string()])
    .await?;

// Find the best LLM for a specific task
if let Some(best_llm) = discovery.find_best_llm("reasoning").await? {
    println!("Best LLM for reasoning: {}", best_llm.display_name);
}
```

### Connecting and Communicating

```rust
use synapse::transport::{LlmRequest, LlmConnection};
use std::collections::HashMap;

// Connect to an LLM
let connection = discovery.connect_to_llm(&best_llm).await?;

// Send a simple message
let response = connection.send_message("Hello, can you help me?").await?;
println!("LLM Response: {}", response);

// Send a structured request
let mut metadata = HashMap::new();
metadata.insert("task_type".to_string(), "analysis".to_string());

let request = LlmRequest {
    prompt: "Analyze this data...".to_string(),
    max_tokens: Some(500),
    temperature: Some(0.7),
    system_prompt: Some("You are a data analyst.".to_string()),
    metadata,
};

let structured_response = connection.send_request(request).await?;
println!("Analysis: {}", structured_response.content);
```

## LLM Service Advertisement

For LLM services to be discoverable by Synapse, they must advertise themselves via mDNS with appropriate TXT records:

### Required TXT Records

- `model_name` - Name of the model (e.g., "llama-2-7b", "gpt-3.5-turbo")
- `model_version` - Model version (e.g., "1.0", "0.1.0")
- `provider` - Service provider (e.g., "ollama", "llamacpp", "openai")
- `capabilities` - Comma-separated capabilities (e.g., "conversation,reasoning,code_generation")

### Optional TXT Records

- `entity_id` - Unique identifier for the service
- `display_name` - Human-readable service name
- `description` - Service description
- `parameters` - Model parameter count (e.g., "7B", "13B", "70B")
- `languages` - Supported languages (e.g., "en,es,fr")
- `context_window` - Context window size in tokens
- `api_version` - API version (e.g., "v1", "2.0")
- `status` - Current status ("available", "busy", "at_capacity")
- `avg_response_time` - Average response time in milliseconds
- `success_rate` - Success rate as decimal (0.0-1.0)
- `load_factor` - Current load factor (0.0-1.0)
- `auth_required` - Whether authentication is required

### Example mDNS Advertisement

```text
Service: my-llm._llm._tcp.local.
Port: 8080
TXT Records:
  model_name=llama-2-7b-chat
  model_version=1.0
  provider=ollama
  capabilities=conversation,reasoning,analysis
  display_name=My Local LLaMA
  description=LLaMA 2 7B Chat model via Ollama
  parameters=7B
  languages=en
  context_window=4096
  status=available
  avg_response_time=500
  success_rate=0.95
  load_factor=0.3
```

## Performance Metrics and Scoring

Synapse uses a sophisticated scoring system to rank LLMs based on:

### Performance Factors

1. **Response Time** - Average response latency
2. **Success Rate** - Percentage of successful requests
3. **Load Factor** - Current system load (0.0 = idle, 1.0 = max capacity)
4. **Quality Score** - Overall service quality assessment
5. **Capability Match** - How well capabilities match the task requirements

### Scoring Algorithm

The scoring algorithm considers:
- Capability overlap with task requirements
- Performance metrics (weighted)
- Current availability status
- Historical reliability

LLMs are scored on a scale of 0.0 to 1.0, with higher scores indicating better suitability for the task.

## Integration with Synapse

### Router Integration

The LLM discovery can be integrated with Synapse's enhanced router:

```rust
use synapse::router_enhanced::EnhancedEmrpRouter;

// The router can use LLM discovery for AI-assisted routing decisions
let router = EnhancedEmrpRouter::new(config).await?;
// LLM discovery integration would be added here
```

### Entity Types and Capabilities

Synapse's entity system already supports AI-related capabilities:

```rust
// From src/config.rs
let ai_capabilities = vec![
    "ai_model".to_string(),
    "natural_language_processing".to_string(),
    "conversation".to_string(),
    "reasoning".to_string(),
    "code_generation".to_string(),
];
```

## Example Applications

### 1. AI Assistant Network

Create a network of AI assistants that can discover and delegate tasks to specialized LLMs:

```rust
// Discovery manager finds code-generation specialists
let code_llms = discovery.find_llms_with_capabilities(&["code_generation".to_string()]).await?;

// Route coding questions to specialized models
if let Some(code_llm) = code_llms.first() {
    let connection = discovery.connect_to_llm(code_llm).await?;
    let response = connection.send_message("Write a function to sort an array").await?;
}
```

### 2. Load Balancing Across LLMs

Distribute requests across multiple LLMs based on their current load:

```rust
let available_llms: Vec<_> = discovery.get_cached_llms().await
    .into_iter()
    .filter(|llm| llm.status == LlmStatus::Available)
    .filter(|llm| llm.performance_metrics.load_factor < 0.8)
    .collect();

// Select LLM with lowest load factor
if let Some(best_llm) = available_llms.iter()
    .min_by(|a, b| a.performance_metrics.load_factor.partial_cmp(&b.performance_metrics.load_factor).unwrap()) {
    // Use this LLM for the request
}
```

### 3. Specialized Task Routing

Route different types of tasks to LLMs optimized for those tasks:

```rust
async fn route_task(task_type: &str, content: &str, discovery: &LlmDiscoveryManager) -> Result<String> {
    if let Some(llm) = discovery.find_best_llm(task_type).await? {
        let connection = discovery.connect_to_llm(&llm).await?;
        return connection.send_message(content).await;
    }
    Err("No suitable LLM found".into())
}

// Route different tasks
let analysis = route_task("analysis", "Analyze this data...", &discovery).await?;
let code = route_task("code_generation", "Write a sorting function", &discovery).await?;
let chat = route_task("conversation", "How are you today?", &discovery).await?;
```

## Running the Demo

To see LLM discovery in action:

```bash
# Run the comprehensive demo
cargo run --example llm_discovery_demo

# The demo will:
# 1. Start LLM discovery
# 2. Wait for services to be discovered
# 3. Display found LLMs with their capabilities
# 4. Demonstrate capability-based selection
# 5. Show connection and communication
```

For testing, you can run local LLM services like Ollama or llama.cpp that support mDNS advertisement.

## Future Enhancements

Potential improvements to the LLM discovery system:

1. **Dynamic Service Registration** - Allow Synapse nodes to register as LLM services
2. **Advanced Load Balancing** - Implement weighted round-robin and other algorithms
3. **Health Monitoring** - Continuous health checks and automatic failover
4. **Security Integration** - Add authentication and authorization for LLM access
5. **Caching Layer** - Cache LLM responses for frequently asked questions
6. **Model Chaining** - Combine multiple LLMs for complex tasks
7. **Cost Optimization** - Factor in cost metrics for LLM selection

The LLM discovery system provides a solid foundation for building sophisticated AI networks with Synapse.
