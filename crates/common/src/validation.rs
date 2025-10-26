//! Input validation for security-critical data
//!
//! This module provides validation functions for user-supplied input to prevent
//! injection attacks, log poisoning, and system crashes from malformed data.

use once_cell::sync::Lazy;
use regex::Regex;
use thiserror::Error;

/// Regex for validating tunnel IDs (12 lowercase alphanumeric characters)
static TUNNEL_ID_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-z0-9]{12}$").unwrap());

/// Regex for validating request IDs (req_ prefix + UUID format)
static REQUEST_ID_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^req_[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$").unwrap()
});

/// Regex for validating connection IDs (AWS API Gateway format)
static CONNECTION_ID_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[A-Za-z0-9_=-]{1,128}$").unwrap());

/// Maximum length for HTTP header values
pub const MAX_HEADER_VALUE_LENGTH: usize = 8192;

/// Maximum length for HTTP paths
pub const MAX_PATH_LENGTH: usize = 2048;

/// Validation errors
#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("Invalid tunnel ID format: {0}")]
    InvalidTunnelId(String),

    #[error("Invalid request ID format: {0}")]
    InvalidRequestId(String),

    #[error("Invalid connection ID format: {0}")]
    InvalidConnectionId(String),

    #[error("Path too long: {0} bytes (max: {1})")]
    PathTooLong(usize, usize),

    #[error("Header value too long: {0} bytes (max: {1})")]
    HeaderValueTooLong(usize, usize),

    #[error("Invalid header value contains control characters")]
    InvalidHeaderValue,
}

/// Validate tunnel ID format
///
/// Tunnel IDs must be exactly 12 lowercase alphanumeric characters.
///
/// # Examples
///
/// ```
/// use http_tunnel_common::validation::validate_tunnel_id;
///
/// assert!(validate_tunnel_id("abc123def456").is_ok());
/// assert!(validate_tunnel_id("INVALID").is_err());
/// assert!(validate_tunnel_id("abc123").is_err()); // too short
/// ```
pub fn validate_tunnel_id(id: &str) -> Result<(), ValidationError> {
    if !TUNNEL_ID_REGEX.is_match(id) {
        return Err(ValidationError::InvalidTunnelId(
            id.chars().take(50).collect::<String>(), // Limit error message
        ));
    }
    Ok(())
}

/// Validate request ID format
///
/// Request IDs must start with "req_" followed by a UUID.
///
/// # Examples
///
/// ```
/// use http_tunnel_common::validation::validate_request_id;
///
/// assert!(validate_request_id("req_550e8400-e29b-41d4-a716-446655440000").is_ok());
/// assert!(validate_request_id("invalid").is_err());
/// ```
pub fn validate_request_id(id: &str) -> Result<(), ValidationError> {
    if !REQUEST_ID_REGEX.is_match(id) {
        return Err(ValidationError::InvalidRequestId(
            id.chars().take(50).collect::<String>(), // Limit error message
        ));
    }
    Ok(())
}

/// Validate connection ID format
///
/// Connection IDs are AWS API Gateway WebSocket connection IDs.
pub fn validate_connection_id(id: &str) -> Result<(), ValidationError> {
    if !CONNECTION_ID_REGEX.is_match(id) {
        return Err(ValidationError::InvalidConnectionId(
            id.chars().take(50).collect::<String>(), // Limit error message
        ));
    }
    Ok(())
}

/// Validate and sanitize HTTP path
///
/// - Removes control characters
/// - Enforces length limits
/// - Ensures path starts with /
pub fn validate_path(path: &str) -> Result<String, ValidationError> {
    // Check length
    if path.len() > MAX_PATH_LENGTH {
        return Err(ValidationError::PathTooLong(path.len(), MAX_PATH_LENGTH));
    }

    // Remove control characters and ensure valid UTF-8
    let sanitized: String = path
        .chars()
        .filter(|c| !c.is_control() || *c == '\t')
        .collect();

    // Ensure path starts with /
    if sanitized.is_empty() {
        Ok("/".to_string())
    } else if sanitized.starts_with('/') {
        Ok(sanitized)
    } else {
        Ok(format!("/{}", sanitized))
    }
}

/// Sanitize HTTP header value
///
/// - Removes dangerous control characters (except tab)
/// - Enforces length limits
/// - Returns sanitized value
pub fn sanitize_header_value(value: &str) -> Result<String, ValidationError> {
    // Check length
    if value.len() > MAX_HEADER_VALUE_LENGTH {
        return Err(ValidationError::HeaderValueTooLong(
            value.len(),
            MAX_HEADER_VALUE_LENGTH,
        ));
    }

    // Remove control characters except tab (which is allowed in HTTP headers)
    let sanitized: String = value
        .chars()
        .filter(|c| !c.is_control() || *c == '\t')
        .collect();

    Ok(sanitized)
}

/// Sanitize header name
///
/// Header names must be ASCII and contain no control characters.
pub fn sanitize_header_name(name: &str) -> Result<String, ValidationError> {
    // Header names should be ASCII
    if !name.is_ascii() {
        return Err(ValidationError::InvalidHeaderValue);
    }

    // Remove control characters
    let sanitized: String = name.chars().filter(|c| !c.is_control()).collect();

    if sanitized.is_empty() {
        return Err(ValidationError::InvalidHeaderValue);
    }

    Ok(sanitized.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_tunnel_id_valid() {
        assert!(validate_tunnel_id("abc123def456").is_ok());
        assert!(validate_tunnel_id("000000000000").is_ok());
        assert!(validate_tunnel_id("zzz999yyy888").is_ok());
    }

    #[test]
    fn test_validate_tunnel_id_invalid() {
        assert!(validate_tunnel_id("ABC123").is_err()); // uppercase
        assert!(validate_tunnel_id("abc123").is_err()); // too short
        assert!(validate_tunnel_id("abc123def456extra").is_err()); // too long
        assert!(validate_tunnel_id("abc-123-def").is_err()); // special chars
        assert!(validate_tunnel_id("").is_err()); // empty
    }

    #[test]
    fn test_validate_request_id_valid() {
        assert!(validate_request_id("req_550e8400-e29b-41d4-a716-446655440000").is_ok());
        assert!(validate_request_id("req_00000000-0000-0000-0000-000000000000").is_ok());
    }

    #[test]
    fn test_validate_request_id_invalid() {
        assert!(validate_request_id("invalid").is_err());
        assert!(validate_request_id("req_12345").is_err());
        assert!(validate_request_id("550e8400-e29b-41d4-a716-446655440000").is_err()); // no prefix
        assert!(validate_request_id("").is_err());
    }

    #[test]
    fn test_validate_connection_id() {
        assert!(validate_connection_id("abc123XYZ").is_ok());
        assert!(validate_connection_id("test-conn_id=123").is_ok());
        assert!(validate_connection_id("").is_err());
        assert!(validate_connection_id("a".repeat(129).as_str()).is_err()); // too long
    }

    #[test]
    fn test_validate_path() {
        assert_eq!(validate_path("/foo/bar").unwrap(), "/foo/bar");
        assert_eq!(validate_path("foo/bar").unwrap(), "/foo/bar");
        assert_eq!(validate_path("").unwrap(), "/");

        // Control characters removed
        let path_with_controls = "/foo\x00/bar\n/baz";
        let sanitized = validate_path(path_with_controls).unwrap();
        assert!(!sanitized.contains('\x00'));
        assert!(!sanitized.contains('\n'));

        // Too long
        let long_path = "/".to_string() + &"a".repeat(3000);
        assert!(validate_path(&long_path).is_err());
    }

    #[test]
    fn test_sanitize_header_value() {
        assert_eq!(
            sanitize_header_value("normal value").unwrap(),
            "normal value"
        );
        assert_eq!(
            sanitize_header_value("value\twith\ttabs").unwrap(),
            "value\twith\ttabs"
        );

        // Control characters removed
        let value_with_controls = "value\x00with\nnull\rand\rcr";
        let sanitized = sanitize_header_value(value_with_controls).unwrap();
        assert!(!sanitized.contains('\x00'));
        assert!(!sanitized.contains('\n'));
        assert!(!sanitized.contains('\r'));

        // Too long
        let long_value = "a".repeat(10000);
        assert!(sanitize_header_value(&long_value).is_err());
    }

    #[test]
    fn test_sanitize_header_name() {
        assert_eq!(
            sanitize_header_name("Content-Type").unwrap(),
            "content-type"
        );
        assert_eq!(
            sanitize_header_name("X-Custom-Header").unwrap(),
            "x-custom-header"
        );

        // Control characters removed
        assert!(sanitize_header_name("header\nname").is_ok());

        // Non-ASCII rejected
        assert!(sanitize_header_name("header™").is_err());
    }
}
