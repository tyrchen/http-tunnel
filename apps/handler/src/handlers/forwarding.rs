//! ForwardingHandler - Handles HTTP API requests
//!
//! This module receives public HTTP requests via API Gateway HTTP API,
//! looks up the connection by subdomain, forwards the request to the agent via WebSocket,
//! and polls for the response. If no response is received within the timeout,
//! it returns a 504 Gateway Timeout.

use aws_lambda_events::apigw::{ApiGatewayProxyRequest, ApiGatewayProxyResponse};
use http_tunnel_common::protocol::Message;
use http_tunnel_common::utils::generate_request_id;
use lambda_runtime::{Error, LambdaEvent};
use tracing::{debug, error, info};

use crate::{
    SharedClients, build_api_gateway_response, build_http_request, content_rewrite,
    extract_tunnel_id_from_path, lookup_connection_by_tunnel_id, save_pending_request,
    send_to_connection, strip_tunnel_id_from_path, wait_for_response,
};

/// Handler for HTTP API requests
pub async fn handle_forwarding(
    event: LambdaEvent<ApiGatewayProxyRequest>,
    clients: &SharedClients,
) -> Result<ApiGatewayProxyResponse, Error> {
    let mut request = event.payload;
    let request_id_context = request.request_context.request_id.clone();

    // Extract tunnel ID from path (path-based routing)
    // HTTP API v2.0 puts path in request.path (stage is stripped by API Gateway for payload format 2.0)
    let original_path = request.path.as_deref().unwrap_or("/");

    debug!("Processing HTTP request, path: {}", original_path);

    let tunnel_id = extract_tunnel_id_from_path(original_path).map_err(|e| {
        error!("Failed to extract tunnel ID from path {}: {}", original_path, e);
        format!("Invalid request path - missing tunnel ID: {}", e)
    })?;

    // Strip tunnel ID from path before forwarding to local service
    let actual_path = strip_tunnel_id_from_path(original_path);

    debug!(
        "Forwarding request for tunnel_id: {} (method: {}, original_path: {}, actual_path: {})",
        tunnel_id,
        request.http_method,
        original_path,
        actual_path
    );

    // Update request path to stripped version
    request.path = Some(actual_path);

    // Look up connection ID by tunnel ID
    let connection_id = lookup_connection_by_tunnel_id(&clients.dynamodb, &tunnel_id)
        .await
        .map_err(|e| {
            error!(
                "Failed to lookup connection for tunnel_id {}: {}",
                tunnel_id, e
            );
            format!("Tunnel not found for ID: {}", tunnel_id)
        })?;

    debug!("Found connection: {}", connection_id);

    // Generate request ID
    let request_id = generate_request_id();

    // Build HttpRequest payload
    let http_request = build_http_request(&request, request_id.clone());

    // Store pending request in DynamoDB for response correlation
    let api_gateway_req_id = request_id_context.as_deref().unwrap_or("unknown");
    save_pending_request(
        &clients.dynamodb,
        &request_id,
        &connection_id,
        api_gateway_req_id,
    )
    .await
    .map_err(|e| {
        error!("Failed to save pending request {}: {}", request_id, e);
        format!("Failed to save request: {}", e)
    })?;

    // Forward request to agent via WebSocket
    let message = Message::HttpRequest(http_request);
    let message_json = serde_json::to_string(&message).map_err(|e| {
        error!("Failed to serialize message: {}", e);
        format!("Failed to serialize request: {}", e)
    })?;

    let apigw_management = clients
        .apigw_management
        .as_ref()
        .ok_or("API Gateway Management client not initialized")?;

    send_to_connection(apigw_management, &connection_id, &message_json)
        .await
        .map_err(|e| {
            error!(
                "Failed to send request {} to connection {}: {}",
                request_id, connection_id, e
            );
            format!("Failed to forward request to agent: {}", e)
        })?;

    info!(
        "Forwarded request {} to connection {} for tunnel_id {}",
        request_id, connection_id, tunnel_id
    );

    // Poll for response with timeout
    match wait_for_response(&clients.dynamodb, &request_id).await {
        Ok(mut response) => {
            info!(
                "Received response for request {}: status {}",
                request_id, response.status_code
            );

            // Apply content rewriting if applicable
            let content_type = response
                .headers
                .get("content-type")
                .and_then(|v| v.first())
                .map(|s| s.as_str())
                .unwrap_or("");

            // Decode body for rewriting
            let body_bytes = http_tunnel_common::decode_body(&response.body)
                .map_err(|e| format!("Failed to decode response body: {}", e))?;
            let body_str = String::from_utf8_lossy(&body_bytes);

            // Rewrite content (default strategy: FullRewrite)
            match content_rewrite::rewrite_response_content(
                &body_str,
                content_type,
                &tunnel_id,
                content_rewrite::RewriteStrategy::FullRewrite,
            ) {
                Ok((rewritten_body, was_rewritten)) => {
                    if was_rewritten {
                        debug!(
                            "Content rewritten for request {}: {} bytes -> {} bytes",
                            request_id,
                            body_str.len(),
                            rewritten_body.len()
                        );

                        // Re-encode the rewritten body
                        response.body = http_tunnel_common::encode_body(rewritten_body.as_bytes());

                        // Update Content-Length header
                        response.headers.insert(
                            "content-length".to_string(),
                            vec![rewritten_body.len().to_string()],
                        );

                        // Remove Transfer-Encoding header if present (we're not chunking)
                        response.headers.remove("transfer-encoding");

                        // Add debug header to indicate rewriting was applied
                        response.headers.insert(
                            "x-tunnel-rewrite-applied".to_string(),
                            vec!["true".to_string()],
                        );
                    }
                }
                Err(e) => {
                    // Log error but don't fail the request
                    error!("Failed to rewrite content for request {}: {}", request_id, e);
                }
            }

            // Convert HttpResponse to API Gateway response
            Ok(build_api_gateway_response(response))
        }
        Err(e) => {
            use aws_lambda_events::encodings::Body;
            use http::header::{HeaderName, HeaderValue};

            error!("Request {} timeout or error: {}", request_id, e);
            // Return 504 Gateway Timeout
            Ok(ApiGatewayProxyResponse {
                status_code: 504,
                headers: [
                    (
                        HeaderName::from_static("content-type"),
                        HeaderValue::from_static("text/plain"),
                    ),
                    (
                        HeaderName::from_static("x-tunnel-error"),
                        HeaderValue::from_static("Gateway Timeout"),
                    ),
                ]
                .into_iter()
                .collect(),
                multi_value_headers: Default::default(),
                body: Some(Body::Text(
                    "Gateway Timeout: No response from agent".to_string(),
                )),
                is_base64_encoded: false,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_lambda_events::encodings::Body;
    use http::header::{HeaderName, HeaderValue};

    #[test]
    fn test_timeout_response_format() {
        let response = ApiGatewayProxyResponse {
            status_code: 504,
            headers: [(
                HeaderName::from_static("content-type"),
                HeaderValue::from_static("text/plain"),
            )]
            .into_iter()
            .collect(),
            multi_value_headers: Default::default(),
            body: Some(Body::Text(
                "Gateway Timeout: No response from agent".to_string(),
            )),
            is_base64_encoded: false,
        };

        assert_eq!(response.status_code, 504);
        assert!(!response.headers.is_empty());
        assert!(response.body.is_some());
    }
}
