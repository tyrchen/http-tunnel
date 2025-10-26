use serde::{Deserialize, Serialize};

/// Connection metadata tracked in DynamoDB for active WebSocket connections
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionMetadata {
    /// API Gateway WebSocket connection ID
    pub connection_id: String,

    /// Unique tunnel ID assigned to this connection (path segment)
    pub tunnel_id: String,

    /// Primary public URL (subdomain if enabled, otherwise path-based)
    pub public_url: String,

    /// Subdomain-based URL (https://{tunnel_id}.{domain})
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subdomain_url: Option<String>,

    /// Path-based URL (https://{domain}/{tunnel_id}) for backward compatibility
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_based_url: Option<String>,

    /// Timestamp when connection was established (Unix epoch seconds)
    pub created_at: i64,

    /// TTL timestamp for DynamoDB auto-deletion (Unix epoch seconds)
    pub ttl: i64,

    /// Optional metadata about the client
    #[serde(default)]
    pub client_info: Option<ClientInfo>,
}

impl ConnectionMetadata {
    /// Create a new connection metadata entry
    pub fn new(
        connection_id: String,
        tunnel_id: String,
        public_url: String,
        created_at: i64,
        ttl: i64,
    ) -> Self {
        Self {
            connection_id,
            tunnel_id,
            public_url,
            subdomain_url: None,
            path_based_url: None,
            created_at,
            ttl,
            client_info: None,
        }
    }

    /// Create a connection with client info
    pub fn with_client_info(mut self, client_info: ClientInfo) -> Self {
        self.client_info = Some(client_info);
        self
    }
}

/// Information about the client agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    /// Client version string
    pub version: String,

    /// Platform/OS information
    pub platform: String,
}

impl ClientInfo {
    /// Create new client info
    pub fn new(version: String, platform: String) -> Self {
        Self { version, platform }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_metadata_creation() {
        let metadata = ConnectionMetadata::new(
            "conn_123".to_string(),
            "abc123def456".to_string(),
            "https://abc123def456.tunnel.example.com".to_string(),
            1234567890,
            1234574090,
        );

        assert_eq!(metadata.connection_id, "conn_123");
        assert_eq!(metadata.tunnel_id, "abc123def456");
        assert_eq!(metadata.created_at, 1234567890);
        assert_eq!(metadata.ttl, 1234574090);
        assert!(metadata.client_info.is_none());
    }

    #[test]
    fn test_connection_metadata_with_client_info() {
        let client_info = ClientInfo::new("1.0.0".to_string(), "linux-x86_64".to_string());

        let metadata = ConnectionMetadata::new(
            "conn_123".to_string(),
            "abc123def456".to_string(),
            "https://abc123def456.tunnel.example.com".to_string(),
            1234567890,
            1234574090,
        )
        .with_client_info(client_info);

        assert!(metadata.client_info.is_some());
        let info = metadata.client_info.unwrap();
        assert_eq!(info.version, "1.0.0");
        assert_eq!(info.platform, "linux-x86_64");
    }

    #[test]
    fn test_connection_metadata_serialization() {
        let metadata = ConnectionMetadata::new(
            "conn_abc".to_string(),
            "xyz789".to_string(),
            "https://xyz789.tunnel.example.com".to_string(),
            1234567890,
            1234574090,
        );

        let json = serde_json::to_string(&metadata).unwrap();
        assert!(json.contains(r#""connection_id":"conn_abc"#));
        assert!(json.contains(r#""tunnel_id":"xyz789"#));

        let parsed: ConnectionMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.connection_id, metadata.connection_id);
        assert_eq!(parsed.tunnel_id, metadata.tunnel_id);
        assert_eq!(parsed.created_at, metadata.created_at);
        assert_eq!(parsed.ttl, metadata.ttl);
    }

    #[test]
    fn test_client_info_serialization() {
        let info = ClientInfo::new("2.1.0".to_string(), "darwin-arm64".to_string());

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains(r#""version":"2.1.0"#));
        assert!(json.contains(r#""platform":"darwin-arm64"#));

        let parsed: ClientInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.version, "2.1.0");
        assert_eq!(parsed.platform, "darwin-arm64");
    }

    #[test]
    fn test_connection_metadata_with_serialized_client_info() {
        let client_info = ClientInfo::new("1.5.0".to_string(), "windows-x86_64".to_string());
        let metadata = ConnectionMetadata::new(
            "conn_123".to_string(),
            "abc123".to_string(),
            "https://abc123.tunnel.example.com".to_string(),
            1234567890,
            1234574090,
        )
        .with_client_info(client_info);

        let json = serde_json::to_string(&metadata).unwrap();
        let parsed: ConnectionMetadata = serde_json::from_str(&json).unwrap();

        assert!(parsed.client_info.is_some());
        let info = parsed.client_info.unwrap();
        assert_eq!(info.version, "1.5.0");
        assert_eq!(info.platform, "windows-x86_64");
    }

    #[test]
    fn test_connection_metadata_default_client_info() {
        let json = r#"{
            "connection_id": "conn_123",
            "tunnel_id": "abc123",
            "public_url": "https://tunnel.example.com/abc123",
            "created_at": 1234567890,
            "ttl": 1234574090
        }"#;

        let parsed: ConnectionMetadata = serde_json::from_str(json).unwrap();
        assert!(parsed.client_info.is_none());
    }
}
