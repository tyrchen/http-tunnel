use thiserror::Error;

/// Error types for the HTTP tunnel system
#[derive(Error, Debug)]
pub enum TunnelError {
    #[error("Invalid message format: {0}")]
    InvalidMessage(String),

    #[error("Connection error: {0}")]
    ConnectionError(String),

    #[error("Timeout waiting for response")]
    Timeout,

    #[error("Local service unavailable: {0}")]
    LocalServiceUnavailable(String),

    #[error("DynamoDB error: {0}")]
    DynamoDbError(String),

    #[error("WebSocket error: {0}")]
    WebSocketError(String),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Base64 decode error: {0}")]
    Base64Error(#[from] base64::DecodeError),

    #[error("HTTP error: {0}")]
    HttpError(String),

    #[error("Internal error: {0}")]
    InternalError(String),
}

/// Type alias for Results using TunnelError
pub type Result<T> = std::result::Result<T, TunnelError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = TunnelError::InvalidMessage("test".to_string());
        assert_eq!(err.to_string(), "Invalid message format: test");

        let err = TunnelError::Timeout;
        assert_eq!(err.to_string(), "Timeout waiting for response");
    }

    #[test]
    fn test_error_conversion() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid json");
        assert!(json_err.is_err());

        let tunnel_err: TunnelError = json_err.unwrap_err().into();
        assert!(matches!(tunnel_err, TunnelError::SerializationError(_)));
    }
}
