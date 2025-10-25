//! Error handling and sanitization
//!
//! This module provides utilities for sanitizing error messages to prevent
//! information disclosure to clients while logging full details internally.

use tracing::error;

/// Sanitize error messages for client responses
///
/// Logs the full error internally but returns a generic message to the client
/// to prevent information disclosure of internal implementation details.
///
/// # Examples
///
/// ```
/// use anyhow::anyhow;
/// let err = anyhow!("Failed to query DynamoDB: AccessDeniedException");
/// let sanitized = sanitize_error(&err);
/// assert_eq!(sanitized, "Internal server error");
/// ```
pub fn sanitize_error(e: &anyhow::Error) -> String {
    // Log full error internally with context
    error!("Internal error: {:#}", e);

    // Return generic message to client
    "Internal server error".to_string()
}

/// Sanitize error with a custom client message
///
/// Logs the full error internally but returns a custom generic message
pub fn sanitize_error_with_message(e: &anyhow::Error, client_message: &str) -> String {
    // Log full error internally
    error!("Error ({}): {:#}", client_message, e);

    // Return custom message to client
    client_message.to_string()
}

/// Check if an error should be shown to the client (for known safe errors)
///
/// Some errors are safe to show (like validation errors), while others
/// should be sanitized (like database errors)
pub fn is_safe_error(e: &anyhow::Error) -> bool {
    let error_str = format!("{:?}", e);

    // Safe error patterns that don't leak internal details
    error_str.contains("ValidationError")
        || error_str.contains("InvalidTunnelId")
        || error_str.contains("InvalidRequestId")
        || error_str.contains("PathTooLong")
        || error_str.contains("HeaderValueTooLong")
        || error_str.contains("Request timeout")
        || error_str.contains("Missing tunnel ID")
        || error_str.contains("Request entity too large")
}

/// Get a user-friendly error message
///
/// Returns the actual error message if it's safe, otherwise returns a sanitized version
pub fn get_client_error_message(e: &anyhow::Error) -> String {
    if is_safe_error(e) {
        // Log but also return to client
        error!("Client error: {}", e);
        format!("{}", e)
    } else {
        // Sanitize sensitive errors
        sanitize_error(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn test_sanitize_error_hides_details() {
        let err = anyhow!("Failed to connect to DynamoDB at 10.0.1.5:8000");
        let sanitized = sanitize_error(&err);

        assert_eq!(sanitized, "Internal server error");
        assert!(!sanitized.contains("DynamoDB"));
        assert!(!sanitized.contains("10.0.1.5"));
    }

    #[test]
    fn test_sanitize_error_with_custom_message() {
        let err = anyhow!("AWS IAM credentials not found");
        let sanitized = sanitize_error_with_message(&err, "Service temporarily unavailable");

        assert_eq!(sanitized, "Service temporarily unavailable");
        assert!(!sanitized.contains("IAM"));
        assert!(!sanitized.contains("credentials"));
    }

    #[test]
    fn test_safe_errors_are_identified() {
        let validation_err = anyhow!("Invalid tunnel ID: ABC");
        assert!(is_safe_error(&validation_err));

        let timeout_err = anyhow!("Request timeout waiting for response");
        assert!(is_safe_error(&timeout_err));

        let db_err = anyhow!("DynamoDB throttling error");
        assert!(!is_safe_error(&db_err));
    }

    #[test]
    fn test_client_error_message() {
        // Safe error should be returned as-is
        let safe_err = anyhow!("Invalid tunnel ID format");
        let msg = get_client_error_message(&safe_err);
        assert!(msg.contains("Invalid tunnel ID"));

        // Unsafe error should be sanitized
        let unsafe_err = anyhow!("AWS SDK error: InvalidParameterException");
        let msg = get_client_error_message(&unsafe_err);
        assert_eq!(msg, "Internal server error");
        assert!(!msg.contains("AWS"));
    }
}
