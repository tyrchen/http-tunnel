//! ConnectHandler - Handles WebSocket $connect route
//!
//! This module contains the logic for handling new WebSocket connections.
//! It generates a unique subdomain, stores connection metadata in DynamoDB, and
//! returns a success response.

use aws_lambda_events::apigw::{ApiGatewayProxyResponse, ApiGatewayWebsocketProxyRequest};
use aws_lambda_events::encodings::Body;
use http_tunnel_common::{ConnectionMetadata, Message};
use http_tunnel_common::constants::CONNECTION_TTL_SECS;
use http_tunnel_common::utils::{calculate_ttl, current_timestamp_secs, generate_subdomain};
use lambda_runtime::{Error, LambdaEvent};
use tracing::{error, info};

use crate::{SharedClients, save_connection_metadata};

/// Handler for WebSocket $connect route
pub async fn handle_connect(
    event: LambdaEvent<ApiGatewayWebsocketProxyRequest>,
    clients: &SharedClients,
) -> Result<ApiGatewayProxyResponse, Error> {
    let request_context = event.payload.request_context;
    let connection_id = request_context
        .connection_id
        .ok_or("Missing connection ID")?;

    info!("New WebSocket connection: {}", connection_id);

    // Generate unique tunnel ID (path segment)
    let tunnel_id = generate_subdomain(); // Reusing subdomain generator for random ID
    let domain = std::env::var("DOMAIN_NAME").unwrap_or_else(|_| "tunnel.example.com".to_string());
    let public_url = format!("https://{}/{}", domain, tunnel_id);

    // Calculate TTL (2 hours from now)
    let created_at = current_timestamp_secs();
    let ttl = calculate_ttl(CONNECTION_TTL_SECS);

    // Store connection metadata in DynamoDB
    let connection_metadata = ConnectionMetadata {
        connection_id: connection_id.clone(),
        tunnel_id: tunnel_id.clone(),
        public_url: public_url.clone(),
        created_at,
        ttl,
        client_info: None,
    };

    save_connection_metadata(&clients.dynamodb, &connection_metadata)
        .await
        .map_err(|e| {
            error!(
                "Failed to save connection metadata for {}: {}",
                connection_id, e
            );
            format!("Failed to register connection: {}", e)
        })?;

    info!(
        "✅ Tunnel established for connection: {} -> {} (tunnel_id: {})",
        connection_id, public_url, tunnel_id
    );
    info!("🌐 Public URL: {}", public_url);

    // Return success response
    // Note: Forwarder will send Ready message to get connection info
    Ok(ApiGatewayProxyResponse {
        status_code: 200,
        headers: Default::default(),
        multi_value_headers: Default::default(),
        body: None,
        is_base64_encoded: false,
    })
}

#[cfg(test)]
mod tests {
    use http_tunnel_common::utils::generate_subdomain;

    #[test]
    fn test_subdomain_format() {
        let subdomain = generate_subdomain();
        assert_eq!(subdomain.len(), 12);
        assert!(subdomain.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn test_public_url_format() {
        let subdomain = "abc123def456";
        let domain = "tunnel.example.com";
        let public_url = format!("https://{}.{}", subdomain, domain);
        assert_eq!(public_url, "https://abc123def456.tunnel.example.com");
    }
}
