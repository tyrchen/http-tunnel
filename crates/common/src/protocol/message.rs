use serde::{Deserialize, Serialize};

use super::{HttpRequest, HttpResponse};

/// All WebSocket messages are wrapped in this typed envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    /// Control plane messages
    Ping,
    Pong,
    Ready, // Sent by forwarder after connection to request connection info

    /// Connection lifecycle
    ConnectionEstablished {
        connection_id: String,
        tunnel_id: String,
        public_url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        subdomain_url: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        path_based_url: Option<String>,
    },

    /// Data plane messages
    HttpRequest(HttpRequest),
    HttpResponse(HttpResponse),

    /// Error handling
    Error {
        request_id: Option<String>,
        code: ErrorCode,
        message: String,
    },
}

/// Error codes for tunnel operations
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    Timeout,
    LocalServiceUnavailable,
    InternalError,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_ping_pong_serialization() {
        let ping = Message::Ping;
        let json = serde_json::to_string(&ping).unwrap();
        assert_eq!(json, r#"{"type":"ping"}"#);

        let pong = Message::Pong;
        let json = serde_json::to_string(&pong).unwrap();
        assert_eq!(json, r#"{"type":"pong"}"#);

        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, Message::Pong));
    }

    #[test]
    fn test_connection_established_serialization() {
        let msg = Message::ConnectionEstablished {
            connection_id: "conn_123".to_string(),
            tunnel_id: "abc123def456".to_string(),
            public_url: "https://abc123def456.tunnel.example.com".to_string(),
            subdomain_url: Some("https://abc123def456.tunnel.example.com".to_string()),
            path_based_url: Some("https://tunnel.example.com/abc123def456".to_string()),
        };

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"connection_established"#));
        assert!(json.contains(r#""connection_id":"conn_123"#));

        let parsed: Message = serde_json::from_str(&json).unwrap();
        match parsed {
            Message::ConnectionEstablished { connection_id, .. } => {
                assert_eq!(connection_id, "conn_123");
            }
            _ => panic!("Expected ConnectionEstablished"),
        }
    }

    #[test]
    fn test_connection_established_backward_compat() {
        // Test backward compatibility - old messages without subdomain_url should still parse
        let json = r#"{"type":"connection_established","connection_id":"conn_123","tunnel_id":"abc123def456","public_url":"https://tunnel.example.com/abc123def456"}"#;

        let parsed: Message = serde_json::from_str(json).unwrap();
        match parsed {
            Message::ConnectionEstablished {
                connection_id,
                subdomain_url,
                path_based_url,
                ..
            } => {
                assert_eq!(connection_id, "conn_123");
                assert!(subdomain_url.is_none());
                assert!(path_based_url.is_none());
            }
            _ => panic!("Expected ConnectionEstablished"),
        }
    }

    #[test]
    fn test_http_request_serialization() {
        let request = HttpRequest {
            request_id: "req_123".to_string(),
            method: "GET".to_string(),
            uri: "/api/v1/users".to_string(),
            headers: HashMap::new(),
            body: String::new(),
            timestamp: 1234567890,
        };

        let msg = Message::HttpRequest(request);
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"http_request"#));
        assert!(json.contains(r#""request_id":"req_123"#));

        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, Message::HttpRequest(_)));
    }

    #[test]
    fn test_error_serialization() {
        let msg = Message::Error {
            request_id: Some("req_123".to_string()),
            code: ErrorCode::Timeout,
            message: "Request timed out".to_string(),
        };

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"error"#));
        assert!(json.contains(r#""code":"timeout"#));
        assert!(json.contains(r#""message":"Request timed out"#));

        let parsed: Message = serde_json::from_str(&json).unwrap();
        match parsed {
            Message::Error { code, .. } => {
                assert!(matches!(code, ErrorCode::Timeout));
            }
            _ => panic!("Expected Error"),
        }
    }

    #[test]
    fn test_error_code_serialization() {
        let codes = vec![
            (ErrorCode::InvalidRequest, "invalid_request"),
            (ErrorCode::Timeout, "timeout"),
            (
                ErrorCode::LocalServiceUnavailable,
                "local_service_unavailable",
            ),
            (ErrorCode::InternalError, "internal_error"),
        ];

        for (code, expected_json) in codes {
            let json = serde_json::to_string(&code).unwrap();
            assert_eq!(json, format!(r#""{}""#, expected_json));

            let parsed: ErrorCode = serde_json::from_str(&json).unwrap();
            assert!(matches!(
                (code, parsed),
                (ErrorCode::InvalidRequest, ErrorCode::InvalidRequest)
                    | (ErrorCode::Timeout, ErrorCode::Timeout)
                    | (
                        ErrorCode::LocalServiceUnavailable,
                        ErrorCode::LocalServiceUnavailable
                    )
                    | (ErrorCode::InternalError, ErrorCode::InternalError)
            ));
        }
    }
}
