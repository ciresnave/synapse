//! Error types for the Synapse system

use std::io::Error as IoError;
use std::net::AddrParseError;
use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum SynapseError {
    #[error("IO error: {0}")]
    Io(String),

    #[error("Address parse error: {0}")]
    AddressParse(#[from] AddrParseError),

    #[error("JSON serialization error: {0}")]
    SerializationError(String),

    #[error("Transport error: {0}")]
    TransportError(String),

    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    #[error("Authentication error: {0}")]
    AuthenticationError(String),

    #[error("Authorization error: {0}")]
    AuthorizationError(String),

    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Message routing error: {0}")]
    RoutingError(String),

    #[error("Encryption error: {0}")]
    EncryptionError(String),

    #[error("Already running")]
    AlreadyRunning,

    #[error("Not running")]
    NotRunning,

    #[error("No transport available for target: {0}")]
    NoTransportAvailable(String),

    #[error("Invalid message format: {0}")]
    InvalidMessageFormat(String),

    #[error("Invalid security level: {0}")]
    InvalidSecurityLevel(String),

    #[error("Peer not found: {0}")]
    PeerNotFound(String),

    #[error("Connection error: {0}")]
    ConnectionError(String),
    
    #[error("Config error: {0}")]
    Config(String),
    
    #[error("Crypto error: {0}")]
    Crypto(String),
    
    #[error("Identity error: {0}")]
    Identity(String),
    
    #[error("Email error: {0}")]
    Email(String),
    
    #[error("SMTP connection error: {0}")]
    SmtpConnection(String),
    
    #[error("Send failed: {0}")]
    SendFailed(String),
    
    #[error("Already exists: {0}")]
    AlreadyExists(String),
    
    #[error("Not found: {0}")]
    NotFound(String),
    
    #[error("Invalid format: {0}")]
    InvalidFormat(String),
    
    #[error("File not found: {0}")]
    FileNotFound(String),
    
    #[error("Validation failed: {0}")]
    ValidationFailed(String),
    
    #[error("Key generation failed: {0}")]
    KeyGeneration(String),
    
    #[error("Invalid key: {0}")]
    InvalidKey(String),
    
    #[error("Key not found: {0}")]
    KeyNotFound(String),
    
    #[error("Encryption failed: {0}")]
    Encryption(String),
    
    #[error("Decryption failed: {0}")]
    Decryption(String),
    
    #[error("Signing failed: {0}")]
    Signing(String),
}

impl From<auth_framework::AuthError> for SynapseError {
    fn from(e: auth_framework::AuthError) -> Self {
        SynapseError::AuthenticationError(e.to_string())
    }
}

impl From<String> for SynapseError {
    fn from(s: String) -> Self {
        SynapseError::TransportError(s)
    }
}

impl From<&str> for SynapseError {
    fn from(s: &str) -> Self {
        SynapseError::TransportError(s.to_string())
    }
}

impl From<bincode::error::EncodeError> for SynapseError {
    fn from(e: bincode::error::EncodeError) -> Self {
        SynapseError::SerializationError(e.to_string())
    }
}

impl From<bincode::error::DecodeError> for SynapseError {
    fn from(e: bincode::error::DecodeError) -> Self {
        SynapseError::SerializationError(e.to_string())
    }
}

impl From<IoError> for SynapseError {
    fn from(e: IoError) -> Self {
        SynapseError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for SynapseError {
    fn from(e: serde_json::Error) -> Self {
        SynapseError::SerializationError(e.to_string())
    }
}

impl From<auto_discovery::error::DiscoveryError> for SynapseError {
    fn from(e: auto_discovery::error::DiscoveryError) -> Self {
        SynapseError::TransportError(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, SynapseError>;

// Type aliases for specific error types
pub type ConfigError = SynapseError;
pub type CryptoError = SynapseError;
pub type IdentityError = SynapseError;
pub type EmailError = SynapseError;
