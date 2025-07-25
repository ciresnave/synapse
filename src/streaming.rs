//! Real-time streaming support for EMRP
use crate::{
    types::{SimpleMessage, MessageType, StreamChunk},
    router::SynapseRouter,
    error::Result,
};
use crate::synapse::blockchain::serialization::{DateTimeWrapper, UuidWrapper};
use uuid::Uuid;
use chrono::Utc;
use base64::Engine;
use std::{sync::Arc, collections::HashMap};
use tokio::sync::mpsc;
use tracing::{debug, warn};

/// Stream manager for handling real-time message streams
pub struct StreamManager {
    router: Arc<SynapseRouter>,
}

impl StreamManager {
    /// Create a new stream manager
    pub fn new(router: Arc<SynapseRouter>) -> Self {
        Self { router }
    }

    /// Start a streaming session with another entity
    pub async fn start_stream(&self, to_entity: &str, from_entity: &str) -> Result<StreamSession> {
        debug!("Starting stream session from {} to {}", from_entity, to_entity);
        
        let (tx, rx) = mpsc::unbounded_channel();
        
        Ok(StreamSession {
            to_entity: to_entity.to_string(),
            from_entity: from_entity.to_string(),
            sender: tx,
            receiver: Some(rx),             stream_id: UuidWrapper::new(Uuid::new_v4()).to_string(),
            chunk_counter: 0,
        })
    }

    /// Send a stream chunk
    pub async fn send_chunk(&self, session: &mut StreamSession, data: &[u8]) -> Result<()> {
        let chunk = StreamChunk {
            stream_id: uuid::Uuid::parse_str(&session.stream_id)
                .map_err(|e| crate::error::SynapseError::InvalidMessageFormat(e.to_string()))?,
            sequence_number: session.chunk_counter,
            chunk_type: "data".to_string(),
            data: base64::engine::general_purpose::STANDARD.encode(data),
            timestamp: DateTimeWrapper::new(Utc::now()),
            priority: crate::types::StreamPriority::Background,
            is_final: false,
            compression: "none".to_string(),
        };

        session.chunk_counter += 1;

        // Convert to message and send
        let content = serde_json::to_string(&chunk)
            .map_err(|e| crate::error::SynapseError::SerializationError(e.to_string()))?;

        let simple_msg = SimpleMessage {
            to: session.to_entity.clone(),
            from_entity: session.from_entity.clone(), // Use session's from_entity
            content,
            message_type: MessageType::StreamChunk,
            metadata: HashMap::new(),
        };

        self.router.send_message(simple_msg, session.to_entity.clone()).await?;

        debug!("Sent stream chunk {} for stream {}", chunk.sequence_number, chunk.stream_id);
        Ok(())
    }

    /// Finish a stream session
    pub async fn finish_stream(&self, session: &mut StreamSession) -> Result<()> {
        let final_chunk = StreamChunk {
            stream_id: uuid::Uuid::parse_str(&session.stream_id)
                .map_err(|e| crate::error::SynapseError::InvalidMessageFormat(e.to_string()))?,
            sequence_number: session.chunk_counter,
            chunk_type: "end".to_string(),
            data: "".to_string(),
            timestamp: DateTimeWrapper::new(Utc::now()),
            priority: crate::types::StreamPriority::Background,
            is_final: true,
            compression: "none".to_string(),
        };

        let content = serde_json::to_string(&final_chunk)
            .map_err(|e| crate::error::SynapseError::SerializationError(e.to_string()))?;

        let simple_msg = SimpleMessage {
            to: session.to_entity.clone(),
            from_entity: session.from_entity.clone(), // Use session's from_entity
            content,
            message_type: MessageType::StreamChunk,
            metadata: HashMap::new(),
        };

        self.router.send_message(simple_msg, session.to_entity.clone()).await?;

        debug!("Finished stream session {}", session.stream_id);
        Ok(())
    }

    /// Process incoming stream chunks
    pub async fn handle_stream_message(&self, message: &SimpleMessage) -> Result<Option<StreamChunk>> {
        if message.message_type != MessageType::StreamChunk {
            return Ok(None);
        }

        match serde_json::from_str::<StreamChunk>(&message.content) {
            Ok(chunk) => {
                debug!("Received stream chunk {} for stream {}", chunk.sequence_number, chunk.stream_id);
                Ok(Some(chunk))
            }
            Err(e) => {
                warn!("Failed to parse stream chunk: {}", e);
                Ok(None)
            }
        }
    }
}

/// Active streaming session
pub struct StreamSession {
    pub to_entity: String,
    pub from_entity: String,
    pub stream_id: String,
    pub chunk_counter: u64,
    sender: mpsc::UnboundedSender<StreamChunk>,
    receiver: Option<mpsc::UnboundedReceiver<StreamChunk>>,
}

impl StreamSession {
    /// Send data through the stream
    pub fn send_data(&self, chunk: StreamChunk) -> Result<()> {
        self.sender.send(chunk)
            .map_err(|_| crate::error::SynapseError::ConnectionError("Stream closed".to_string()))?;
        Ok(())
    }

    /// Receive the next chunk from the stream
    pub async fn recv_chunk(&mut self) -> Option<StreamChunk> {
        if let Some(receiver) = &mut self.receiver {
            receiver.recv().await
        } else {
            None
        }
    }

    /// Check if the stream is finished
    pub fn is_finished(&self) -> bool {
        self.sender.is_closed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_stream_session_creation() {
        // This test would require setting up a full router
        // For now, just test basic stream session functionality
        let (tx, rx) = mpsc::unbounded_channel();
        
        let session = StreamSession {
            to_entity: "test-entity".to_string(),
            from_entity: "test-sender".to_string(),
            stream_id: "test-stream".to_string(),
            chunk_counter: 0,
            sender: tx,
            receiver: Some(rx),
        };

        assert_eq!(session.to_entity, "test-entity");
        assert_eq!(session.stream_id, "test-stream");
        assert_eq!(session.chunk_counter, 0);
    }
}
