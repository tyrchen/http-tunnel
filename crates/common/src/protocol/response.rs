use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents the response from the local service, sent back through the tunnel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    /// Must match the request_id from the corresponding HttpRequest
    pub request_id: String,

    /// HTTP status code (200, 404, 500, etc.)
    pub status_code: u16,

    /// Response headers as a map of header name to list of values
    pub headers: HashMap<String, Vec<String>>,

    /// Response body encoded in Base64
    #[serde(default)]
    pub body: String,

    /// Processing time in milliseconds (local service response time)
    #[serde(default)]
    pub processing_time_ms: u64,
}

impl HttpResponse {
    /// Create a new HTTP response
    pub fn new(request_id: String, status_code: u16) -> Self {
        Self {
            request_id,
            status_code,
            headers: HashMap::new(),
            body: String::new(),
            processing_time_ms: 0,
        }
    }

    /// Check if the response has a body
    pub fn has_body(&self) -> bool {
        !self.body.is_empty()
    }

    /// Check if the response is successful (2xx status code)
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status_code)
    }

    /// Check if the response is a client error (4xx status code)
    pub fn is_client_error(&self) -> bool {
        (400..500).contains(&self.status_code)
    }

    /// Check if the response is a server error (5xx status code)
    pub fn is_server_error(&self) -> bool {
        (500..600).contains(&self.status_code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_response_creation() {
        let res = HttpResponse::new("req_123".to_string(), 200);

        assert_eq!(res.request_id, "req_123");
        assert_eq!(res.status_code, 200);
        assert!(res.headers.is_empty());
        assert!(!res.has_body());
        assert_eq!(res.processing_time_ms, 0);
    }

    #[test]
    fn test_http_response_status_checks() {
        let success = HttpResponse::new("req_1".to_string(), 200);
        assert!(success.is_success());
        assert!(!success.is_client_error());
        assert!(!success.is_server_error());

        let client_error = HttpResponse::new("req_2".to_string(), 404);
        assert!(!client_error.is_success());
        assert!(client_error.is_client_error());
        assert!(!client_error.is_server_error());

        let server_error = HttpResponse::new("req_3".to_string(), 500);
        assert!(!server_error.is_success());
        assert!(!server_error.is_client_error());
        assert!(server_error.is_server_error());
    }

    #[test]
    fn test_http_response_with_headers() {
        let mut headers = HashMap::new();
        headers.insert(
            "content-type".to_string(),
            vec!["application/json".to_string()],
        );
        headers.insert("x-custom-header".to_string(), vec!["value".to_string()]);

        let res = HttpResponse {
            request_id: "req_123".to_string(),
            status_code: 200,
            headers,
            body: "eyJ0ZXN0IjoidmFsdWUifQ==".to_string(),
            processing_time_ms: 123,
        };

        assert_eq!(res.headers.len(), 2);
        assert!(res.has_body());
        assert_eq!(res.processing_time_ms, 123);
    }

    #[test]
    fn test_http_response_serialization() {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), vec!["text/plain".to_string()]);

        let res = HttpResponse {
            request_id: "req_abc123".to_string(),
            status_code: 201,
            headers,
            body: "dGVzdCBkYXRh".to_string(), // "test data"
            processing_time_ms: 456,
        };

        let json = serde_json::to_string(&res).unwrap();
        assert!(json.contains(r#""request_id":"req_abc123"#));
        assert!(json.contains(r#""status_code":201"#));
        assert!(json.contains(r#""processing_time_ms":456"#));

        let parsed: HttpResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.request_id, res.request_id);
        assert_eq!(parsed.status_code, res.status_code);
        assert_eq!(parsed.body, res.body);
        assert_eq!(parsed.processing_time_ms, res.processing_time_ms);
    }

    #[test]
    fn test_http_response_multiple_header_values() {
        let mut headers = HashMap::new();
        headers.insert(
            "set-cookie".to_string(),
            vec!["session=abc".to_string(), "token=xyz".to_string()],
        );

        let res = HttpResponse {
            request_id: "req_123".to_string(),
            status_code: 200,
            headers,
            body: String::new(),
            processing_time_ms: 0,
        };

        assert_eq!(res.headers.get("set-cookie").unwrap().len(), 2);

        let json = serde_json::to_string(&res).unwrap();
        let parsed: HttpResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.headers.get("set-cookie").unwrap().len(), 2);
    }

    #[test]
    fn test_http_response_defaults() {
        let json = r#"{
            "request_id": "req_123",
            "status_code": 200,
            "headers": {}
        }"#;

        let parsed: HttpResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.body, "");
        assert_eq!(parsed.processing_time_ms, 0);
        assert!(!parsed.has_body());
    }

    #[test]
    fn test_status_code_ranges() {
        let codes = vec![
            (100, false, false, false),
            (200, true, false, false),
            (299, true, false, false),
            (300, false, false, false),
            (400, false, true, false),
            (404, false, true, false),
            (499, false, true, false),
            (500, false, false, true),
            (503, false, false, true),
            (599, false, false, true),
        ];

        for (code, is_success, is_client_err, is_server_err) in codes {
            let res = HttpResponse::new("req".to_string(), code);
            assert_eq!(
                res.is_success(),
                is_success,
                "Failed for status code {}",
                code
            );
            assert_eq!(
                res.is_client_error(),
                is_client_err,
                "Failed for status code {}",
                code
            );
            assert_eq!(
                res.is_server_error(),
                is_server_err,
                "Failed for status code {}",
                code
            );
        }
    }
}
