use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents an HTTP request forwarded from the public endpoint to the agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequest {
    /// Unique identifier to correlate request and response
    pub request_id: String,

    /// HTTP method (GET, POST, PUT, DELETE, etc.)
    pub method: String,

    /// Request URI including path and query string
    /// Example: "/api/v1/users?limit=10"
    pub uri: String,

    /// HTTP headers as a map of header name to list of values
    /// Multiple values per header are supported
    pub headers: HashMap<String, Vec<String>>,

    /// Request body encoded in Base64
    /// Empty string for requests without body
    #[serde(default)]
    pub body: String,

    /// Timestamp when request was received (Unix epoch in milliseconds)
    pub timestamp: u64,
}

impl HttpRequest {
    /// Create a new HTTP request
    pub fn new(method: String, uri: String, request_id: String, timestamp: u64) -> Self {
        Self {
            request_id,
            method,
            uri,
            headers: HashMap::new(),
            body: String::new(),
            timestamp,
        }
    }

    /// Check if the request has a body
    pub fn has_body(&self) -> bool {
        !self.body.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_request_creation() {
        let req = HttpRequest::new(
            "GET".to_string(),
            "/api/users".to_string(),
            "req_123".to_string(),
            1234567890,
        );

        assert_eq!(req.method, "GET");
        assert_eq!(req.uri, "/api/users");
        assert_eq!(req.request_id, "req_123");
        assert_eq!(req.timestamp, 1234567890);
        assert!(req.headers.is_empty());
        assert!(!req.has_body());
    }

    #[test]
    fn test_http_request_with_headers() {
        let mut headers = HashMap::new();
        headers.insert(
            "content-type".to_string(),
            vec!["application/json".to_string()],
        );
        headers.insert(
            "authorization".to_string(),
            vec!["Bearer token123".to_string()],
        );

        let req = HttpRequest {
            request_id: "req_123".to_string(),
            method: "POST".to_string(),
            uri: "/api/data".to_string(),
            headers,
            body: "eyJ0ZXN0IjoidmFsdWUifQ==".to_string(), // {"test":"value"}
            timestamp: 1234567890,
        };

        assert_eq!(req.headers.len(), 2);
        assert!(req.has_body());
    }

    #[test]
    fn test_http_request_serialization() {
        let mut headers = HashMap::new();
        headers.insert("host".to_string(), vec!["example.com".to_string()]);

        let req = HttpRequest {
            request_id: "req_abc123".to_string(),
            method: "GET".to_string(),
            uri: "/path?query=value".to_string(),
            headers,
            body: String::new(),
            timestamp: 1234567890000,
        };

        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""request_id":"req_abc123"#));
        assert!(json.contains(r#""method":"GET"#));
        assert!(json.contains(r#""uri":"/path?query=value"#));

        let parsed: HttpRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.request_id, req.request_id);
        assert_eq!(parsed.method, req.method);
        assert_eq!(parsed.uri, req.uri);
        assert_eq!(parsed.timestamp, req.timestamp);
    }

    #[test]
    fn test_http_request_multiple_header_values() {
        let mut headers = HashMap::new();
        headers.insert(
            "cookie".to_string(),
            vec!["session=abc".to_string(), "token=xyz".to_string()],
        );

        let req = HttpRequest {
            request_id: "req_123".to_string(),
            method: "GET".to_string(),
            uri: "/".to_string(),
            headers,
            body: String::new(),
            timestamp: 1234567890,
        };

        assert_eq!(req.headers.get("cookie").unwrap().len(), 2);

        let json = serde_json::to_string(&req).unwrap();
        let parsed: HttpRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.headers.get("cookie").unwrap().len(), 2);
    }

    #[test]
    fn test_http_request_body_default() {
        let json = r#"{
            "request_id": "req_123",
            "method": "GET",
            "uri": "/test",
            "headers": {},
            "timestamp": 1234567890
        }"#;

        let parsed: HttpRequest = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.body, "");
        assert!(!parsed.has_body());
    }
}
