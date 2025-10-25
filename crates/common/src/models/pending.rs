use serde::{Deserialize, Serialize};

/// Pending request state tracked in DynamoDB while waiting for response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingRequest {
    /// Unique request identifier
    pub request_id: String,

    /// Connection ID that should handle this request
    pub connection_id: String,

    /// Original API Gateway request context (for response)
    pub api_gateway_request_id: String,

    /// Timestamp when request was forwarded (Unix epoch seconds)
    pub created_at: i64,

    /// TTL for auto-cleanup (Unix epoch seconds)
    /// Should be short-lived (e.g., 30 seconds)
    pub ttl: i64,
}

impl PendingRequest {
    /// Create a new pending request entry
    pub fn new(
        request_id: String,
        connection_id: String,
        api_gateway_request_id: String,
        created_at: i64,
        ttl: i64,
    ) -> Self {
        Self {
            request_id,
            connection_id,
            api_gateway_request_id,
            created_at,
            ttl,
        }
    }

    /// Check if the request has expired based on current timestamp
    pub fn is_expired(&self, current_timestamp: i64) -> bool {
        current_timestamp > self.ttl
    }

    /// Get the age of the request in seconds
    pub fn age_secs(&self, current_timestamp: i64) -> i64 {
        current_timestamp - self.created_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pending_request_creation() {
        let pending = PendingRequest::new(
            "req_123".to_string(),
            "conn_abc".to_string(),
            "gw_req_xyz".to_string(),
            1234567890,
            1234567920,
        );

        assert_eq!(pending.request_id, "req_123");
        assert_eq!(pending.connection_id, "conn_abc");
        assert_eq!(pending.api_gateway_request_id, "gw_req_xyz");
        assert_eq!(pending.created_at, 1234567890);
        assert_eq!(pending.ttl, 1234567920);
    }

    #[test]
    fn test_pending_request_expiration() {
        let pending = PendingRequest::new(
            "req_123".to_string(),
            "conn_abc".to_string(),
            "gw_req_xyz".to_string(),
            1234567890,
            1234567920,
        );

        // Not expired before TTL
        assert!(!pending.is_expired(1234567900));
        assert!(!pending.is_expired(1234567920));

        // Expired after TTL
        assert!(pending.is_expired(1234567921));
        assert!(pending.is_expired(1234568000));
    }

    #[test]
    fn test_pending_request_age() {
        let pending = PendingRequest::new(
            "req_123".to_string(),
            "conn_abc".to_string(),
            "gw_req_xyz".to_string(),
            1234567890,
            1234567920,
        );

        assert_eq!(pending.age_secs(1234567890), 0);
        assert_eq!(pending.age_secs(1234567900), 10);
        assert_eq!(pending.age_secs(1234567920), 30);
    }

    #[test]
    fn test_pending_request_serialization() {
        let pending = PendingRequest::new(
            "req_abc123".to_string(),
            "conn_xyz789".to_string(),
            "gw_req_456".to_string(),
            1234567890,
            1234567920,
        );

        let json = serde_json::to_string(&pending).unwrap();
        assert!(json.contains(r#""request_id":"req_abc123"#));
        assert!(json.contains(r#""connection_id":"conn_xyz789"#));
        assert!(json.contains(r#""api_gateway_request_id":"gw_req_456"#));

        let parsed: PendingRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.request_id, pending.request_id);
        assert_eq!(parsed.connection_id, pending.connection_id);
        assert_eq!(
            parsed.api_gateway_request_id,
            pending.api_gateway_request_id
        );
        assert_eq!(parsed.created_at, pending.created_at);
        assert_eq!(parsed.ttl, pending.ttl);
    }

    #[test]
    fn test_pending_request_deserialization() {
        let json = r#"{
            "request_id": "req_test",
            "connection_id": "conn_test",
            "api_gateway_request_id": "gw_test",
            "created_at": 1000000000,
            "ttl": 1000000030
        }"#;

        let parsed: PendingRequest = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.request_id, "req_test");
        assert_eq!(parsed.connection_id, "conn_test");
        assert_eq!(parsed.api_gateway_request_id, "gw_test");
        assert_eq!(parsed.created_at, 1000000000);
        assert_eq!(parsed.ttl, 1000000030);
    }
}
