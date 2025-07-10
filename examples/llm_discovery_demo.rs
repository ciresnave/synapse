/*!
 * LLM Discovery and Connection Example for Synapse
 * 
 * This example demonstrates Synapse's ability to discover and connect to 
 * Large Language Models (LLMs) on the local network using mDNS service discovery.
 * 
 * Features demonstrated:
 * - Automatic LLM service discovery via mDNS
 * - Capability-based LLM filtering and selection
 * - Performance-based LLM scoring and ranking
 * - Connection management and message exchange
 * - Support for various LLM providers (Ollama, LLaMA.cpp, OpenAI-compatible APIs)
 * 
 * Run with: cargo run --example llm_discovery_demo
 */

use synapse::transport::{
    LlmDiscoveryManager, LlmDiscoveryConfig, DiscoveredLlm,
    LlmRequest
};
use std::time::Duration;
use tokio::time::{timeout, sleep};
use tracing::{warn, error};
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    println!("🔍 Synapse LLM Discovery Demo");
    println!("=============================");
    println!();

    // Create LLM discovery configuration
    let config = LlmDiscoveryConfig {
        scan_interval: Duration::from_secs(15),
        cache_ttl: Duration::from_secs(300), // 5 minutes
        preferred_capabilities: vec![
            "conversation".to_string(),
            "reasoning".to_string(),
            "code_generation".to_string(),
            "analysis".to_string(),
        ],
        max_llms: 20,
        required_capabilities: vec!["conversation".to_string()],
    };

    println!("📡 Creating LLM discovery manager...");
    let discovery_manager = match LlmDiscoveryManager::new(Some(config)).await {
        Ok(manager) => {
            println!("✅ LLM discovery manager created successfully");
            manager
        }
        Err(e) => {
            error!("❌ Failed to create LLM discovery manager: {}", e);
            return Err(e.into());
        }
    };

    println!("🚀 Starting LLM discovery process...");
    if let Err(e) = discovery_manager.start_discovery().await {
        error!("❌ Failed to start discovery: {}", e);
        return Err(e.into());
    }

    println!("⏳ Waiting for LLM discovery (15 seconds)...");
    sleep(Duration::from_secs(15)).await;

    // Discover all available LLMs
    println!("🔍 Scanning for available LLMs...");
    let discovered_llms = match discovery_manager.discover_llms().await {
        Ok(llms) => llms,
        Err(e) => {
            warn!("⚠️  LLM discovery failed: {}", e);
            vec![]
        }
    };

    if discovered_llms.is_empty() {
        println!("🔍 No LLMs discovered on the network.");
        println!();
        println!("💡 To test this demo, try running one of these LLM services:");
        println!("   • Ollama: ollama serve");
        println!("   • LLaMA.cpp: ./server -m model.gguf --port 8080");
        println!("   • Text Generation WebUI: python server.py --api");
        println!("   • vLLM: python -m vllm.entrypoints.api_server");
        println!();
        println!("🔧 Make sure the service advertises itself via mDNS with service types:");
        println!("   • _llm._tcp.local.");
        println!("   • _ollama._tcp.local.");
        println!("   • _llamacpp._tcp.local.");
        println!("   • _openai._tcp.local.");
        return Ok(());
    }

    println!("🎉 Discovered {} LLM(s):", discovered_llms.len());
    println!();

    // Display discovered LLMs
    for (i, llm) in discovered_llms.iter().enumerate() {
        display_llm_info(i + 1, llm);
    }

    // Demonstrate capability-based filtering
    println!("🎯 Finding LLMs with code generation capabilities...");
    let code_llms = discovery_manager
        .find_llms_with_capabilities(&["code_generation".to_string()])
        .await?;
    
    println!("Found {} LLM(s) with code generation capabilities", code_llms.len());

    // Demonstrate task-based LLM selection
    println!("🧠 Finding best LLM for reasoning tasks...");
    if let Some(best_llm) = discovery_manager.find_best_llm("reasoning").await? {
        println!("✅ Best LLM for reasoning: {} ({})", 
                best_llm.display_name, best_llm.model_info.model_name);
        
        // Demonstrate connection and message exchange
        println!("🔗 Attempting to connect to the best LLM...");
        match connect_and_chat(&discovery_manager, &best_llm).await {
            Ok(_) => println!("✅ Successfully communicated with LLM"),
            Err(e) => warn!("⚠️  Failed to communicate with LLM: {}", e),
        }
    } else {
        println!("❌ No suitable LLM found for reasoning tasks");
    }

    // Show all cached LLMs
    println!("💾 Cached LLMs:");
    let cached_llms = discovery_manager.get_cached_llms().await;
    for llm in &cached_llms {
        println!("  • {} ({}): {} capabilities", 
                llm.display_name, 
                llm.entity_id,
                llm.capabilities.len());
    }

    // Demonstrate getting a specific LLM by ID
    if let Some(first_llm) = discovered_llms.first() {
        if let Some(retrieved_llm) = discovery_manager
            .get_llm_by_id(&first_llm.entity_id).await {
            println!("🔍 Successfully retrieved LLM by ID: {}", retrieved_llm.display_name);
        }
    }

    println!();
    println!("🎊 LLM discovery demo completed successfully!");

    Ok(())
}

fn display_llm_info(index: usize, llm: &DiscoveredLlm) {
    println!("{}. 🤖 {}", index, llm.display_name);
    println!("   📊 Model: {} v{}", llm.model_info.model_name, llm.model_info.model_version);
    println!("   🏢 Provider: {}", llm.model_info.provider);
    println!("   🌐 Endpoint: {}", llm.connection_info.primary_endpoint);
    println!("   📡 Protocols: {}", llm.connection_info.protocols.join(", "));
    println!("   🧠 Capabilities: {}", llm.capabilities.join(", "));
    
    if let Some(context_window) = llm.model_info.context_window {
        println!("   📏 Context Window: {} tokens", context_window);
    }
    
    if let Some(params) = &llm.model_info.parameters {
        println!("   ⚙️  Parameters: {}", params);
    }
    
    println!("   🚦 Status: {:?}", llm.status);
    println!("   ⚡ Avg Response Time: {:.0}ms", llm.performance_metrics.avg_response_time_ms);
    println!("   ✅ Success Rate: {:.1}%", llm.performance_metrics.success_rate * 100.0);
    println!("   📈 Quality Score: {:.1}/1.0", llm.performance_metrics.quality_score);
    
    if !llm.model_info.languages.is_empty() {
        println!("   🌍 Languages: {}", llm.model_info.languages.join(", "));
    }
    
    println!();
}

async fn connect_and_chat(
    discovery_manager: &LlmDiscoveryManager,
    llm: &DiscoveredLlm
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔗 Connecting to {} at {}...", llm.display_name, llm.connection_info.primary_endpoint);
    
    let connection = discovery_manager.connect_to_llm(llm).await?;
    println!("✅ Connected successfully!");
    
    // Send a simple message
    let simple_message = "Hello! Can you help me understand what you're capable of?";
    println!("💬 Sending message: '{}'", simple_message);
    
    let response = timeout(
        Duration::from_secs(30),
        connection.send_message(simple_message)
    ).await??;
    
    println!("🤖 Response: {}", response);
    
    // Send a structured request
    let mut metadata = HashMap::new();
    metadata.insert("task_type".to_string(), "capability_inquiry".to_string());
    metadata.insert("priority".to_string(), "normal".to_string());
    
    let structured_request = LlmRequest {
        prompt: "Please list your main capabilities and strengths.".to_string(),
        max_tokens: Some(200),
        temperature: Some(0.7),
        system_prompt: Some("You are a helpful AI assistant. Be concise and informative.".to_string()),
        metadata,
    };
    
    println!("📝 Sending structured request...");
    let structured_response = timeout(
        Duration::from_secs(30),
        connection.send_request(structured_request)
    ).await??;
    
    println!("🤖 Structured response: {}", structured_response.content);
    println!("📊 Response metadata:");
    println!("   • Model used: {}", structured_response.metadata.model_used);
    println!("   • Tokens used: {}", structured_response.metadata.tokens_used);
    println!("   • Processing time: {}ms", structured_response.metadata.processing_time_ms);
    println!("   • Confidence: {:.1}%", structured_response.metadata.confidence_score * 100.0);
    
    Ok(())
}
