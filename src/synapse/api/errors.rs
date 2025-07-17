use serde::{Serialize, Deserialize};
use thiserror::Error;
use uuid::Uuid;
use crate::synapse::blockchain::serialization::UuidWrapper;

/// API Error Types - standardized error handling for Synapse API endpoints
#[derive(Debug, Error, Serialize, Deserialize, Clone)]
pub enum ApiError {
    #[error("Resource not found: {0}")]
    NotFound(String),
    
    #[error("Authentication required")]
    Unauthorized,
    
    #[error("Permission denied: {0}")]
    Forbidden(String),
    
    #[error("Validation error: {0}")]
    ValidationError(String),
    
    #[error("Conflict: {0}")]
    Conflict(String),
    
    #[error("Rate limit exceeded")]
    RateLimited,
    
    #[error("External service error: {0}")]
    ExternalServiceError(String),
    
    #[error("Database error")]
    DatabaseError,
    
    #[error("Network error: {0}")]
    NetworkError(String),
    
    #[error("Trust system error: {0}")]
    TrustSystemError(String),
    
    #[error("Invalid request: {0}")]
    BadRequest(String),
    
    #[error("Internal server error")]
    InternalServerError,
    
    #[error("Service unavailable")]
    ServiceUnavailable,
}

/// Converts ApiError to HTTP status codes
impl ApiError {
    pub fn status_code(&self) -> u16 {
        match self {
            ApiError::NotFound(_) => 404,
            ApiError::Unauthorized => 401,
            ApiError::Forbidden(_) => 403,
            ApiError::ValidationError(_) => 400,
            ApiError::Conflict(_) => 409,
            ApiError::RateLimited => 429,
            ApiError::ExternalServiceError(_) => 502,
            ApiError::DatabaseError => 500,
            ApiError::NetworkError(_) => 500,
            ApiError::TrustSystemError(_) => 500,
            ApiError::BadRequest(_) => 400,
            ApiError::InternalServerError => 500,
            ApiError::ServiceUnavailable => 503,
        }
    }
}

/// Standardized API Response for successful and error outcomes
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<ApiErrorResponse>,
    pub message: Option<String>,
}

/// Detailed error response structure
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiErrorResponse {
    pub code: u16,
    pub error_type: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
    pub request_id: Option<String>,
}

impl<T> ApiResponse<T> {
    /// Create a success response
    pub fn success(data: T, message: Option<String>) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            message,
        }
    }
    
    /// Create an error response
    pub fn error(err: ApiError) -> Self {
        let request_id = UuidWrapper::new(Uuid::new_v4()).to_string();
        
        // Log the error with request ID for traceability
        tracing::error!("API Error [{}]: {}", request_id, err);
        
        Self {
            success: false,
            data: None,
            error: Some(ApiErrorResponse {
                code: err.status_code(),
                error_type: format!("{:?}", err).split('(').next().unwrap_or("Unknown").to_string(),
                message: err.to_string(),
                details: None,
                request_id: Some(request_id),
            }),
            message: None,
        }
    }
    
    /// Create an error response with additional details
    pub fn error_with_details(err: ApiError, details: serde_json::Value) -> Self {
        let request_id = UuidWrapper::new(Uuid::new_v4()).to_string();
        
        // Log the error with request ID and details for traceability
        tracing::error!("API Error [{}]: {} - Details: {:?}", request_id, err, details);
        
        Self {
            success: false,
            data: None,
            error: Some(ApiErrorResponse {
                code: err.status_code(),
                error_type: format!("{:?}", err).split('(').next().unwrap_or("Unknown").to_string(),
                message: err.to_string(),
                details: Some(details),
                request_id: Some(request_id),
            }),
            message: None,
        }
    }
}

/// Conversion from anyhow::Error to ApiError
impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        // Log original error with full context
        tracing::error!("Converting anyhow error to ApiError: {:?}", err);
        
        // Try to downcast to ApiError if it's already wrapped
        if let Some(api_err) = err.downcast_ref::<ApiError>() {
            return api_err.clone();
        }
        
        // Otherwise, create a generic internal server error
        ApiError::InternalServerError
    }
}

/// Helper for creating validation errors
pub fn validation_error(message: &str) -> ApiError {
    ApiError::ValidationError(message.to_string())
}

/// Helper for handling not found errors
pub fn not_found(resource_type: &str, id: &str) -> ApiError {
    ApiError::NotFound(format!("{} with id {} not found", resource_type, id))
}
