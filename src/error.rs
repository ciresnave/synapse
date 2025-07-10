//! Error types for the EMRP system

use thiserror::Error;

/// Main error type for EMRP operations
#[derive(Error, Debug, Clone)]
pub enum EmrpError {
    /// Cryptographic operation failed
    #[error("Cryptographic error: {0}")]
    Crypto(#[from] CryptoError),

    /// Email transport error
    #[error("Email error: {0}")]
    Email(#[from] EmailError),

    /// Identity resolution error
    #[error("Identity error: {0}")]
    Identity(#[from] IdentityError),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(String),

    /// Network error
    #[error("Network error: {0}")]
    Network(String),

    /// Transport layer error
    #[error("Transport error: {0}")]
    Transport(String),

    /// Generic error with message
    #[error("{0}")]
    Generic(String),

    /// Stream closed unexpectedly
    #[error("Stream closed unexpectedly")]
    StreamClosed,

    /// Invalid format error
    #[error("Invalid format: {0}")]
    InvalidFormat(String),
}

/// Cryptographic errors
#[derive(Error, Debug, Clone)]
pub enum CryptoError {
    #[error("Key generation failed: {0}")]
    KeyGeneration(String),

    #[error("Encryption failed: {0}")]
    Encryption(String),

    #[error("Decryption failed: {0}")]
    Decryption(String),

    #[error("Signature generation failed: {0}")]
    Signing(String),

    #[error("Signature verification failed: {0}")]
    Verification(String),

    #[error("Invalid key format: {0}")]
    InvalidKey(String),

    #[error("Key not found: {0}")]
    KeyNotFound(String),
}

/// Email transport errors
#[derive(Error, Debug, Clone)]
pub enum EmailError {
    #[error("SMTP connection failed: {0}")]
    SmtpConnection(String),

    #[error("IMAP connection failed: {0}")]
    ImapConnection(String),

    #[error("Authentication failed: {0}")]
    Authentication(String),

    #[error("Message send failed: {0}")]
    SendFailed(String),

    #[error("Message receive failed: {0}")]
    ReceiveFailed(String),

    #[error("Message parsing failed: {0}")]
    ParseFailed(String),

    #[error("Invalid email format: {0}")]
    InvalidFormat(String),
}

/// Identity management errors
#[derive(Error, Debug, Clone)]
pub enum IdentityError {
    #[error("Identity not found: {0}")]
    NotFound(String),

    #[error("Identity already exists: {0}")]
    AlreadyExists(String),

    #[error("Invalid identity format: {0}")]
    InvalidFormat(String),

    #[error("Trust level validation failed: {0}")]
    TrustValidation(String),

    #[error("Global ID generation failed: {0}")]
    GlobalIdGeneration(String),
}

/// Configuration errors
#[derive(Error, Debug, Clone)]
pub enum ConfigError {
    #[error("Configuration file not found: {0}")]
    FileNotFound(String),

    #[error("Invalid configuration format: {0}")]
    InvalidFormat(String),

    #[error("Missing required configuration: {0}")]
    MissingRequired(String),

    #[error("Configuration validation failed: {0}")]
    ValidationFailed(String),
}

/// Convenient Result type for EMRP operations
pub type Result<T> = std::result::Result<T, EmrpError>;

impl From<String> for EmrpError {
    fn from(msg: String) -> Self {
        EmrpError::Generic(msg)
    }
}

impl From<&str> for EmrpError {
    fn from(msg: &str) -> Self {
        EmrpError::Generic(msg.to_string())
    }
}

impl From<serde_json::Error> for EmrpError {
    fn from(err: serde_json::Error) -> Self {
        EmrpError::Serialization(err.to_string())
    }
}

impl From<std::io::Error> for EmrpError {
    fn from(err: std::io::Error) -> Self {
        EmrpError::Io(err.to_string())
    }
}

impl From<bincode::error::EncodeError> for EmrpError {
    fn from(err: bincode::error::EncodeError) -> Self {
        EmrpError::Serialization(err.to_string())
    }
}

impl From<bincode::error::DecodeError> for EmrpError {
    fn from(err: bincode::error::DecodeError) -> Self {
        EmrpError::Serialization(err.to_string())
    }
}

impl From<std::net::AddrParseError> for EmrpError {
    fn from(err: std::net::AddrParseError) -> Self {
        EmrpError::Network(err.to_string())
    }
}


