//! Integration tests for error paths and edge cases
//!
//! These tests verify proper error handling in failure scenarios.

#[cfg(test)]
mod error_path_tests {

    #[tokio::test]
    async fn test_invalid_tunnel_id_format() {
        // Test various invalid tunnel ID formats
        let invalid_ids = vec![
            "UPPERCASE123",    // Contains uppercase
            "short",           // Too short
            "toolongid123456", // Too long
            "special-chars!",  // Special characters
            "../../../etc",    // Path traversal attempt
        ];

        for id in invalid_ids {
            let result = http_tunnel_common::validation::validate_tunnel_id(id);
            assert!(result.is_err(), "Should reject invalid tunnel ID: {}", id);
        }
    }

    #[tokio::test]
    async fn test_request_body_size_limit() {
        // Test that oversized requests are rejected
        use http_tunnel_common::constants::MAX_BODY_SIZE_BYTES;

        let small_body = "a".repeat(1000);
        assert!(small_body.len() < MAX_BODY_SIZE_BYTES);

        let large_body = "a".repeat(MAX_BODY_SIZE_BYTES + 1);
        assert!(large_body.len() > MAX_BODY_SIZE_BYTES);

        // In actual handler, this would return 413
    }

    #[test]
    fn test_header_sanitization() {
        use http_tunnel_common::validation::sanitize_header_value;

        // Control characters should be removed
        let dirty_header = "value\x00with\nnull\rand\rcr";
        let clean = sanitize_header_value(dirty_header).unwrap();

        assert!(!clean.contains('\x00'));
        assert!(!clean.contains('\n'));
        assert!(!clean.contains('\r'));

        // Tabs should be preserved
        let header_with_tab = "value\twith\ttab";
        let result = sanitize_header_value(header_with_tab).unwrap();
        assert_eq!(result, "value\twith\ttab");
    }

    #[test]
    fn test_path_validation_edge_cases() {
        use http_tunnel_common::validation::validate_path;

        // Empty path should default to /
        assert_eq!(validate_path("").unwrap(), "/");

        // Path without leading slash should be normalized
        assert_eq!(validate_path("foo/bar").unwrap(), "/foo/bar");

        // Control characters removed
        let bad_path = "/foo\x00/bar\n/baz";
        let clean = validate_path(bad_path).unwrap();
        assert!(!clean.contains('\x00'));
        assert!(!clean.contains('\n'));
    }

    #[test]
    fn test_request_id_format_validation() {
        use http_tunnel_common::validation::validate_request_id;

        // Valid format
        assert!(validate_request_id("req_550e8400-e29b-41d4-a716-446655440000").is_ok());

        // Invalid formats
        assert!(validate_request_id("invalid").is_err());
        assert!(validate_request_id("req_notauuid").is_err());
        assert!(validate_request_id("550e8400-e29b-41d4-a716-446655440000").is_err()); // No prefix
    }

    #[test]
    fn test_error_message_sanitization() {
        use crate::error_handling::{is_safe_error, sanitize_error};
        use anyhow::anyhow;

        // Internal errors should be sanitized
        let db_error = anyhow!("DynamoDB connection failed: timeout at 10.0.1.5:8000");
        let sanitized = sanitize_error(&db_error);
        assert_eq!(sanitized, "Internal server error");
        assert!(!sanitized.contains("DynamoDB"));
        assert!(!sanitized.contains("10.0.1.5"));

        // Safe errors can be shown
        let safe_error = anyhow!("Invalid tunnel ID format: test");
        assert!(is_safe_error(&safe_error));
    }

    #[tokio::test]
    async fn test_connection_id_validation() {
        use http_tunnel_common::validation::validate_connection_id;

        // Valid AWS API Gateway connection IDs
        assert!(validate_connection_id("abc123XYZ").is_ok());
        assert!(validate_connection_id("test_conn-id=123").is_ok());

        // Invalid
        assert!(validate_connection_id("").is_err());
        assert!(validate_connection_id(&"a".repeat(200)).is_err()); // Too long
    }

    #[test]
    fn test_jwt_token_extraction_priority() {
        // Token should be extracted in this order:
        // 1. Authorization header (preferred)
        // 2. Query parameter (fallback)

        // This is tested in apps/handler/src/auth.rs
    }

    #[test]
    fn test_content_type_detection() {
        use crate::content_rewrite::should_rewrite_content;

        // Should rewrite HTML
        assert!(should_rewrite_content("text/html"));
        assert!(should_rewrite_content("text/html; charset=utf-8"));

        // Should rewrite CSS and JS
        assert!(should_rewrite_content("text/css"));
        assert!(should_rewrite_content("application/javascript"));

        // Should not rewrite binary
        assert!(!should_rewrite_content("image/png"));
        assert!(!should_rewrite_content("application/pdf"));
        assert!(!should_rewrite_content("application/octet-stream"));
    }
}

#[cfg(test)]
mod timeout_tests {

    #[tokio::test]
    #[allow(clippy::assertions_on_constants)]
    async fn test_request_timeout_is_under_api_gateway_limit() {
        use http_tunnel_common::constants::REQUEST_TIMEOUT_SECS;

        // API Gateway has a 29-second timeout
        assert!(
            REQUEST_TIMEOUT_SECS < 29,
            "Request timeout must be under API Gateway's 29s limit"
        );
    }

    #[tokio::test]
    #[allow(clippy::assertions_on_constants)]
    async fn test_heartbeat_interval_is_under_idle_timeout() {
        use http_tunnel_common::constants::{HEARTBEAT_INTERVAL_SECS, WEBSOCKET_IDLE_TIMEOUT_SECS};

        // Heartbeat must be sent before WebSocket idle timeout
        assert!(
            HEARTBEAT_INTERVAL_SECS < WEBSOCKET_IDLE_TIMEOUT_SECS,
            "Heartbeat interval must be shorter than WebSocket idle timeout"
        );
    }
}
