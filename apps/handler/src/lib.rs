//! Shared utilities for AWS Lambda handlers
//!
//! This module provides common functionality used across all Lambda functions including
//! DynamoDB operations, request/response transformations, and helper functions.

use anyhow::{Context, Result, anyhow};
use aws_lambda_events::apigw::{ApiGatewayProxyRequest, ApiGatewayProxyResponse};
use aws_sdk_apigatewaymanagement::Client as ApiGatewayManagementClient;
use aws_sdk_apigatewaymanagement::primitives::Blob;
use aws_sdk_dynamodb::Client as DynamoDbClient;
use aws_sdk_dynamodb::types::AttributeValue;
use http_tunnel_common::ConnectionMetadata;
use http_tunnel_common::constants::{PENDING_REQUEST_TTL_SECS, REQUEST_TIMEOUT_SECS};
use http_tunnel_common::protocol::{HttpRequest, HttpResponse};
use http_tunnel_common::utils::{calculate_ttl, current_timestamp_millis, current_timestamp_secs};
use std::time::{Duration, Instant};
use tracing::{debug, error};

pub mod content_rewrite;
pub mod handlers;

/// Shared AWS clients used across all handlers
pub struct SharedClients {
    pub dynamodb: DynamoDbClient,
    pub apigw_management: Option<ApiGatewayManagementClient>,
}

/// Extract tunnel ID from request path (path-based routing)
/// Example: "/abc123/api/users" -> "abc123"
pub fn extract_tunnel_id_from_path(path: &str) -> Result<String> {
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if parts.is_empty() || parts[0].is_empty() {
        return Err(anyhow!("Missing tunnel ID in path"));
    }
    Ok(parts[0].to_string())
}

/// Strip tunnel ID from path before forwarding to local service
/// Example: "/abc123/api/users" -> "/api/users"
/// Example: "/abc123" -> "/"
pub fn strip_tunnel_id_from_path(path: &str) -> String {
    let parts: Vec<&str> = path.trim_start_matches('/').splitn(2, '/').collect();
    if parts.len() > 1 && !parts[1].is_empty() {
        format!("/{}", parts[1])
    } else {
        "/".to_string()
    }
}

/// DEPRECATED: Extract subdomain from host header (subdomain-based routing)
/// Use extract_tunnel_id_from_path for path-based routing instead
pub fn extract_subdomain(host: &str) -> Result<String> {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.is_empty() {
        return Err(anyhow!("Invalid host header"));
    }
    Ok(parts[0].to_string())
}

/// Save connection metadata to DynamoDB
pub async fn save_connection_metadata(
    client: &DynamoDbClient,
    metadata: &ConnectionMetadata,
) -> Result<()> {
    let table_name = std::env::var("CONNECTIONS_TABLE_NAME")
        .context("CONNECTIONS_TABLE_NAME environment variable not set")?;

    client
        .put_item()
        .table_name(&table_name)
        .item(
            "connectionId",
            AttributeValue::S(metadata.connection_id.clone()),
        )
        .item("tunnelId", AttributeValue::S(metadata.tunnel_id.clone()))
        .item("publicUrl", AttributeValue::S(metadata.public_url.clone()))
        .item(
            "createdAt",
            AttributeValue::N(metadata.created_at.to_string()),
        )
        .item("ttl", AttributeValue::N(metadata.ttl.to_string()))
        .send()
        .await
        .context("Failed to save connection metadata to DynamoDB")?;

    Ok(())
}

/// Delete connection from DynamoDB
pub async fn delete_connection(client: &DynamoDbClient, connection_id: &str) -> Result<()> {
    let table_name = std::env::var("CONNECTIONS_TABLE_NAME")
        .context("CONNECTIONS_TABLE_NAME environment variable not set")?;

    client
        .delete_item()
        .table_name(&table_name)
        .key("connectionId", AttributeValue::S(connection_id.to_string()))
        .send()
        .await
        .context("Failed to delete connection from DynamoDB")?;

    Ok(())
}

/// Look up connection ID by tunnel ID using GSI (path-based routing)
pub async fn lookup_connection_by_tunnel_id(
    client: &DynamoDbClient,
    tunnel_id: &str,
) -> Result<String> {
    let table_name = std::env::var("CONNECTIONS_TABLE_NAME")
        .context("CONNECTIONS_TABLE_NAME environment variable not set")?;
    let index_name = "tunnel-id-index";

    let result = client
        .query()
        .table_name(&table_name)
        .index_name(index_name)
        .key_condition_expression("tunnelId = :tunnel_id")
        .expression_attribute_values(":tunnel_id", AttributeValue::S(tunnel_id.to_string()))
        .limit(1)
        .send()
        .await
        .context("Failed to query connection by tunnel ID")?;

    let items = result.items.ok_or_else(|| anyhow!("No items returned"))?;
    let item = items
        .first()
        .ok_or_else(|| anyhow!("Connection not found for tunnel ID: {}", tunnel_id))?;

    let connection_id = item
        .get("connectionId")
        .and_then(|v| v.as_s().ok())
        .ok_or_else(|| anyhow!("Missing connectionId in DynamoDB item"))?;

    Ok(connection_id.clone())
}

/// DEPRECATED: Look up connection ID by subdomain (subdomain-based routing)
/// Use lookup_connection_by_tunnel_id for path-based routing instead
pub async fn lookup_connection_by_subdomain(
    client: &DynamoDbClient,
    subdomain: &str,
) -> Result<String> {
    // For backwards compatibility, just call the new function
    lookup_connection_by_tunnel_id(client, subdomain).await
}

/// Build HttpRequest from API Gateway event
pub fn build_http_request(request: &ApiGatewayProxyRequest, request_id: String) -> HttpRequest {
    let method = request.http_method.to_string();

    let uri = format!("{}{}", request.path.as_deref().unwrap_or("/"), {
        let params = &request.query_string_parameters;
        if params.is_empty() {
            String::new()
        } else {
            format!(
                "?{}",
                params
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join("&")
            )
        }
    });

    let headers = request
        .headers
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_string(),
                vec![v.to_str().unwrap_or("").to_string()],
            )
        })
        .collect();

    let body = request
        .body
        .as_ref()
        .map(|b| {
            if request.is_base64_encoded {
                b.to_string() // Already base64
            } else {
                http_tunnel_common::encode_body(b.as_bytes())
            }
        })
        .unwrap_or_default();

    HttpRequest {
        request_id,
        method,
        uri,
        headers,
        body,
        timestamp: current_timestamp_millis(),
    }
}

/// Save pending request to DynamoDB
pub async fn save_pending_request(
    client: &DynamoDbClient,
    request_id: &str,
    connection_id: &str,
    api_gateway_request_id: &str,
) -> Result<()> {
    let table_name = std::env::var("PENDING_REQUESTS_TABLE_NAME")
        .context("PENDING_REQUESTS_TABLE_NAME environment variable not set")?;
    let created_at = current_timestamp_secs();
    let ttl = calculate_ttl(PENDING_REQUEST_TTL_SECS);

    client
        .put_item()
        .table_name(&table_name)
        .item("requestId", AttributeValue::S(request_id.to_string()))
        .item("connectionId", AttributeValue::S(connection_id.to_string()))
        .item(
            "apiGatewayRequestId",
            AttributeValue::S(api_gateway_request_id.to_string()),
        )
        .item("createdAt", AttributeValue::N(created_at.to_string()))
        .item("ttl", AttributeValue::N(ttl.to_string()))
        .item("status", AttributeValue::S("pending".to_string()))
        .send()
        .await
        .context("Failed to save pending request to DynamoDB")?;

    Ok(())
}

/// Send message to WebSocket connection
pub async fn send_to_connection(
    client: &ApiGatewayManagementClient,
    connection_id: &str,
    data: &str,
) -> Result<()> {
    client
        .post_to_connection()
        .connection_id(connection_id)
        .data(Blob::new(data.as_bytes()))
        .send()
        .await
        .context("Failed to send message to WebSocket connection")?;

    Ok(())
}

/// Poll DynamoDB for response with exponential backoff
pub async fn wait_for_response(client: &DynamoDbClient, request_id: &str) -> Result<HttpResponse> {
    let table_name = std::env::var("PENDING_REQUESTS_TABLE_NAME")
        .context("PENDING_REQUESTS_TABLE_NAME environment variable not set")?;
    let timeout = Duration::from_secs(REQUEST_TIMEOUT_SECS);
    let start = Instant::now();

    // Start with 50ms poll interval, max 500ms
    let mut poll_interval = Duration::from_millis(50);
    let max_poll_interval = Duration::from_millis(500);

    loop {
        if start.elapsed() > timeout {
            return Err(anyhow!("Request timeout waiting for response"));
        }

        // Query DynamoDB for response
        let result = client
            .get_item()
            .table_name(&table_name)
            .key("requestId", AttributeValue::S(request_id.to_string()))
            .send()
            .await
            .context("Failed to get pending request from DynamoDB")?;

        if let Some(item) = result.item {
            let status = item
                .get("status")
                .and_then(|v| v.as_s().ok())
                .ok_or_else(|| anyhow!("Missing status in DynamoDB item"))?;

            if status == "completed" {
                // Extract response data
                let response_data = item
                    .get("responseData")
                    .and_then(|v| v.as_s().ok())
                    .ok_or_else(|| anyhow!("Missing responseData in completed request"))?;

                let response: HttpResponse = serde_json::from_str(response_data)
                    .context("Failed to parse response data JSON")?;

                // Clean up pending request
                if let Err(e) = client
                    .delete_item()
                    .table_name(&table_name)
                    .key("requestId", AttributeValue::S(request_id.to_string()))
                    .send()
                    .await
                {
                    error!("Failed to clean up pending request: {}", e);
                }

                return Ok(response);
            }
        }

        tokio::time::sleep(poll_interval).await;

        // Exponential backoff with max limit
        poll_interval = std::cmp::min(poll_interval * 2, max_poll_interval);
    }
}

/// Convert HttpResponse to API Gateway response
pub fn build_api_gateway_response(response: HttpResponse) -> ApiGatewayProxyResponse {
    use http::header::{HeaderName, HeaderValue};

    let headers = response
        .headers
        .iter()
        .filter_map(|(k, v)| {
            v.first().and_then(|val| {
                HeaderName::from_bytes(k.as_bytes())
                    .ok()
                    .and_then(|name| HeaderValue::from_str(val).ok().map(|value| (name, value)))
            })
        })
        .collect();

    use aws_lambda_events::encodings::Body;

    let body = if !response.body.is_empty() {
        Some(Body::Text(response.body))
    } else {
        None
    };

    ApiGatewayProxyResponse {
        status_code: response.status_code as i64,
        headers,
        multi_value_headers: Default::default(),
        body,
        is_base64_encoded: true,
    }
}

/// Update pending request with response data
pub async fn update_pending_request_with_response(
    client: &DynamoDbClient,
    response: &HttpResponse,
) -> Result<()> {
    let table_name = std::env::var("PENDING_REQUESTS_TABLE_NAME")
        .context("PENDING_REQUESTS_TABLE_NAME environment variable not set")?;

    // Serialize response to JSON
    let response_data =
        serde_json::to_string(response).context("Failed to serialize response to JSON")?;

    // Update pending request with response data
    client
        .update_item()
        .table_name(&table_name)
        .key("requestId", AttributeValue::S(response.request_id.clone()))
        .update_expression("SET #status = :status, responseData = :data")
        .expression_attribute_names("#status", "status")
        .expression_attribute_values(":status", AttributeValue::S("completed".to_string()))
        .expression_attribute_values(":data", AttributeValue::S(response_data))
        .send()
        .await
        .context("Failed to update pending request with response")?;

    debug!("Updated pending request: {}", response.request_id);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_subdomain_simple() {
        let subdomain = extract_subdomain("abc123.tunnel.example.com").unwrap();
        assert_eq!(subdomain, "abc123");
    }

    #[test]
    fn test_extract_subdomain_localhost() {
        let subdomain = extract_subdomain("localhost").unwrap();
        assert_eq!(subdomain, "localhost");
    }

    #[test]
    fn test_extract_subdomain_with_port() {
        let host = "abc123.tunnel.example.com:443";
        let subdomain = extract_subdomain(host).unwrap();
        assert_eq!(subdomain, "abc123");
    }

    #[test]
    fn test_build_http_request_simple_get() {
        use http::Method;

        let request = ApiGatewayProxyRequest {
            http_method: Method::GET,
            path: Some("/api/users".to_string()),
            ..Default::default()
        };

        let http_request = build_http_request(&request, "req_123".to_string());

        assert_eq!(http_request.request_id, "req_123");
        assert_eq!(http_request.method, "GET");
        assert_eq!(http_request.uri, "/api/users");
        assert!(http_request.body.is_empty());
    }

    #[test]
    fn test_build_http_request_with_path() {
        use http::Method;

        let request = ApiGatewayProxyRequest {
            http_method: Method::GET,
            path: Some("/api/users".to_string()),
            ..Default::default()
        };

        let http_request = build_http_request(&request, "req_123".to_string());

        assert_eq!(http_request.request_id, "req_123");
        assert_eq!(http_request.method, "GET");
        assert_eq!(http_request.uri, "/api/users");
    }

    #[test]
    fn test_build_http_request_with_body() {
        use http::Method;

        let request = ApiGatewayProxyRequest {
            http_method: Method::POST,
            path: Some("/api/data".to_string()),
            body: Some("Hello World".to_string()),
            is_base64_encoded: false,
            ..Default::default()
        };

        let http_request = build_http_request(&request, "req_123".to_string());

        assert_eq!(http_request.method, "POST");
        assert!(!http_request.body.is_empty());
    }

    #[test]
    fn test_build_api_gateway_response_success() {
        use std::collections::HashMap;

        let mut headers = HashMap::new();
        headers.insert(
            "content-type".to_string(),
            vec!["application/json".to_string()],
        );

        let response = HttpResponse {
            request_id: "req_123".to_string(),
            status_code: 200,
            headers,
            body: "eyJ0ZXN0IjoidmFsdWUifQ==".to_string(),
            processing_time_ms: 123,
        };

        let apigw_response = build_api_gateway_response(response);

        assert_eq!(apigw_response.status_code, 200);
        assert!(apigw_response.is_base64_encoded);
        assert!(apigw_response.body.is_some());
        // Check header exists (actual value checking would require http types)
        assert!(!apigw_response.headers.is_empty());
    }

    #[test]
    fn test_build_api_gateway_response_empty_body() {
        use std::collections::HashMap;

        let response = HttpResponse {
            request_id: "req_123".to_string(),
            status_code: 204,
            headers: HashMap::new(),
            body: String::new(),
            processing_time_ms: 0,
        };

        let apigw_response = build_api_gateway_response(response);

        assert_eq!(apigw_response.status_code, 204);
        assert!(apigw_response.body.is_none());
    }
}
