//! Multi-Modal Collaboration Example
//! 
//! This example demonstrates how to create multi-modal collaboration
//! between different types of AI entities using the synapse library.

use synapse::{Config, SynapseRouter, SimpleMessage, MessageType};
use anyhow::Result;
use tracing::{info, debug};
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("🎭 Starting Multi-Modal Collaboration Example");

    // Create different types of AI entities
    let entities = vec![
        ("text_ai", "ai_model", "Text processing and generation"),
        ("vision_ai", "ai_model", "Computer vision and image analysis"),
        ("audio_ai", "ai_model", "Audio processing and speech"),
        ("code_ai", "ai_model", "Code generation and analysis"),
        ("coordinator", "tool", "Task coordination and orchestration"),
    ];

    let mut entity_routers = Vec::new();

    // Initialize all entities
    info!("🚀 Initializing multi-modal AI entities...");
    for (name, entity_type, description) in &entities {
        debug!("Initializing {}: {}", name, description);
        
        let config = Config::default_for_entity(name.to_string(), entity_type.to_string());
        
        match SynapseRouter::new(config, name.to_string()).await {
            Ok(router) => {
                info!("✅ {} initialized: {}", name, description);
                entity_routers.push((name, entity_type, description, router));
            }
            Err(e) => {
                info!("❌ Failed to initialize {}: {}", name, e);
            }
        }
    }

    info!("🌐 Multi-modal collaboration network established with {} entities", entity_routers.len());

    // Simulate multi-modal collaboration workflow
    info!("🎯 Starting collaborative task: 'Create a comprehensive project report'");
    
    let collaboration_steps = vec![
        ("coordinator", "text_ai", "generate_outline", "Create an outline for the project report"),
        ("text_ai", "coordinator", "outline_ready", "Project report outline completed with 5 sections"),
        ("coordinator", "vision_ai", "analyze_charts", "Analyze the performance charts in the data folder"),
        ("vision_ai", "coordinator", "charts_analyzed", "Found 3 charts: performance trend, user growth, error rates"),
        ("coordinator", "code_ai", "review_codebase", "Review the codebase for quality metrics"),
        ("code_ai", "coordinator", "code_reviewed", "Codebase analysis: 15K lines, 87% test coverage, clean architecture"),
        ("coordinator", "audio_ai", "create_summary", "Generate audio summary of key findings"),
        ("audio_ai", "coordinator", "audio_ready", "5-minute audio summary generated with key insights"),
        ("coordinator", "text_ai", "final_report", "Compile everything into final comprehensive report"),
        ("text_ai", "coordinator", "report_complete", "Final report ready: 12 pages with visual and audio components"),
    ];

    for (from_entity, to_entity, task, description) in collaboration_steps {
        let message = SimpleMessage {
            to: to_entity.to_string(),
            from_entity: from_entity.to_string(),
            content: format!("{}:{}", task, description),
            message_type: MessageType::Direct,
            metadata: create_collaboration_metadata(task, from_entity, to_entity),
        };

        info!("🔄 {} → {}: {}", from_entity, to_entity, description);
        debug!("Task: {} | Details: {}", task, description);

        // Find the router for the sending entity
        if let Some((_, _, _, router)) = entity_routers.iter()
            .find(|(name, _, _, _)| **name == from_entity) {
            
            match router.convert_to_secure_message(&message).await {
                Ok(secure_msg) => {
                    debug!("✅ Collaboration message secured: {}", secure_msg.message_id);
                }
                Err(e) => {
                    debug!("⚠️  Collaboration message prepared (secure conversion failed: {})", e);
                }
            }
        }

        // Add delay to simulate processing time
        tokio::time::sleep(tokio::time::Duration::from_millis(400)).await;
    }

    // Show final collaboration results
    info!("📊 Multi-Modal Collaboration Results:");
    info!("  ✓ Text AI: Generated outline and final report");
    info!("  ✓ Vision AI: Analyzed performance charts and visualizations");
    info!("  ✓ Code AI: Reviewed codebase and generated quality metrics");
    info!("  ✓ Audio AI: Created audio summary of key findings");
    info!("  ✓ Coordinator: Orchestrated the entire collaborative process");

    info!("🎉 Multi-Modal Collaboration example completed successfully!");
    
    Ok(())
}

/// Create metadata for collaboration operations
fn create_collaboration_metadata(task: &str, from: &str, to: &str) -> HashMap<String, String> {
    let mut metadata = HashMap::new();
    
    metadata.insert("task".to_string(), task.to_string());
    metadata.insert("collaboration_id".to_string(), "project_report_2024".to_string());
    metadata.insert("workflow_step".to_string(), 
                    format!("{}_to_{}", from, to));
    metadata.insert("timestamp".to_string(), 
                    chrono::Utc::now().to_rfc3339());
    metadata.insert("priority".to_string(), "normal".to_string());
    
    // Add modality information
    let modality = match from {
        "text_ai" => "text",
        "vision_ai" => "vision",
        "audio_ai" => "audio",
        "code_ai" => "code",
        "coordinator" => "orchestration",
        _ => "unknown",
    };
    metadata.insert("modality".to_string(), modality.to_string());
    
    // Add collaboration tracking
    metadata.insert("collaboration_step".to_string(), 
                    format!("{}-{}", chrono::Utc::now().timestamp(), uuid::Uuid::new_v4()));
    
    metadata
}
