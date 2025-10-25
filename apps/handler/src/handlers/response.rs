//! ResponseHandler - Handles WebSocket $default route
//!
//! This module processes messages from the agent, including HTTP responses,
//! error messages, and ping/pong heartbeats. It updates the pending request status
//! in DynamoDB so the ForwardingHandler can complete the HTTP request.

use aws_lambda_events::apigw::ApiGatewayProxyResponse;
use aws_sdk_dynamodb::Client as DynamoDbClient;
use aws_sdk_dynamodb::types::AttributeValue;
use http_tunnel_common::encode_body;
use http_tunnel_common::protocol::{ErrorCode, HttpResponse, Message};
use lambda_runtime::{Error, LambdaEvent};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

use crate::{SharedClients, update_pending_request_with_response};
use aws_sdk_apigatewaymanagement::primitives::Blob;

/// WebSocket $default event structure (messages from agent)
/// This is different from $connect/$disconnect events - it doesn't have connectedAt
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSocketMessageEvent {
    pub request_context: WebSocketMessageRequestContext,
    pub body: Option<String>,
    pub is_base64_encoded: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSocketMessageRequestContext {
    pub route_key: String,
    #[serde(default)]
    pub event_type: Option<String>,
    pub connection_id: String,
    pub request_id: String,
    pub domain_name: Option<String>,
    pub stage: Option<String>,
    pub api_id: Option<String>,
    #[serde(default)]
    pub connected_at: Option<i64>,
}

/// Handler for WebSocket $default route (messages from agent)
pub async fn handle_response(
    event: LambdaEvent<WebSocketMessageEvent>,
    clients: &SharedClients,
) -> Result<ApiGatewayProxyResponse, Error> {
    let body = event.payload.body.ok_or("Missing message body")?;

    debug!("Received message from agent: {}", body);

    // Parse message
    let message: Message = serde_json::from_str(&body).map_err(|e| {
        error!("Failed to parse message: {}", e);
        format!("Invalid message format: {}", e)
    })?;

    let connection_id = &event.payload.request_context.connection_id;

    match message {
        Message::Ready => {
            info!("Received Ready message from agent, sending ConnectionEstablished");
            handle_ready_message(&clients.dynamodb, &clients.apigw_management, connection_id)
                .await?;
        }
        Message::HttpResponse(response) => {
            info!(
                "Received HTTP response for request {}: status {}",
                response.request_id, response.status_code
            );
            handle_http_response(&clients.dynamodb, response).await?;
        }
        Message::Ping => {
            // Heartbeat received, no action needed
            debug!("Received ping from agent");
        }
        Message::Pong => {
            // Pong received, no action needed
            debug!("Received pong from agent");
        }
        Message::Error {
            request_id,
            code,
            message: error_message,
        } => {
            if let Some(req_id) = request_id {
                warn!(
                    "Received error for request {}: {:?} - {}",
                    req_id, code, error_message
                );
                handle_error_response(&clients.dynamodb, &req_id, code, &error_message).await?;
            } else {
                warn!("Received error without request ID: {}", error_message);
            }
        }
        _ => {
            warn!("Received unexpected message type");
        }
    }

    // Always return success
    Ok(ApiGatewayProxyResponse {
        status_code: 200,
        headers: Default::default(),
        multi_value_headers: Default::default(),
        body: None,
        is_base64_encoded: false,
    })
}

/// Handle HTTP response from agent
async fn handle_http_response(
    client: &DynamoDbClient,
    response: HttpResponse,
) -> Result<(), Error> {
    update_pending_request_with_response(client, &response)
        .await
        .map_err(|e| {
            error!(
                "Failed to update pending request {}: {}",
                response.request_id, e
            );
            format!("Failed to update pending request: {}", e)
        })?;

    debug!(
        "Successfully updated pending request: {}",
        response.request_id
    );

    Ok(())
}

/// Handle Ready message from agent - send back ConnectionEstablished with public URL
async fn handle_ready_message(
    dynamodb_client: &DynamoDbClient,
    apigw_management: &Option<aws_sdk_apigatewaymanagement::Client>,
    connection_id: &str,
) -> Result<(), Error> {
    // Look up connection metadata from DynamoDB
    let table_name = std::env::var("CONNECTIONS_TABLE_NAME")
        .map_err(|_| "CONNECTIONS_TABLE_NAME environment variable not set")?;

    let result = dynamodb_client
        .get_item()
        .table_name(&table_name)
        .key("connectionId", AttributeValue::S(connection_id.to_string()))
        .send()
        .await
        .map_err(|e| {
            error!(
                "Failed to get connection metadata for {}: {}",
                connection_id, e
            );
            format!("Failed to get connection metadata: {}", e)
        })?;

    let item = result.item.ok_or("Connection not found")?;

    let tunnel_id = item
        .get("tunnelId")
        .and_then(|v| v.as_s().ok())
        .ok_or("Missing tunnelId")?;

    let public_url = item
        .get("publicUrl")
        .and_then(|v| v.as_s().ok())
        .ok_or("Missing publicUrl")?;

    // Send ConnectionEstablished message
    if let Some(client) = apigw_management {
        let message = Message::ConnectionEstablished {
            connection_id: connection_id.to_string(),
            tunnel_id: tunnel_id.clone(),
            public_url: public_url.clone(),
        };

        let message_json = serde_json::to_string(&message)
            .map_err(|e| format!("Failed to serialize ConnectionEstablished: {}", e))?;

        info!(
            "Sending ConnectionEstablished to {}: {}",
            connection_id, message_json
        );

        // Retry logic with exponential backoff for WebSocket dispatch failures
        // API Gateway WebSocket connections may not be immediately ready to receive messages
        let mut retry_count = 0;
        let max_retries = 3;
        let mut delay_ms = 100;

        loop {
            match client
                .post_to_connection()
                .connection_id(connection_id)
                .data(Blob::new(message_json.as_bytes()))
                .send()
                .await
            {
                Ok(_) => {
                    info!(
                        "✅ Sent ConnectionEstablished to {} (attempt {})",
                        connection_id,
                        retry_count + 1
                    );
                    break;
                }
                Err(e) => {
                    retry_count += 1;
                    if retry_count >= max_retries {
                        error!(
                            "Failed to send ConnectionEstablished to {} after {} attempts: {}",
                            connection_id, max_retries, e
                        );
                        // Don't fail the request - connection is established, client will timeout and retry
                        break;
                    }
                    warn!(
                        "Failed to send ConnectionEstablished (attempt {}), retrying in {}ms: {}",
                        retry_count, delay_ms, e
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                    delay_ms *= 2; // Exponential backoff
                }
            }
        }
    } else {
        error!("API Gateway Management client not available");
    }

    Ok(())
}

/// Handle error response from agent
async fn handle_error_response(
    client: &DynamoDbClient,
    request_id: &str,
    code: ErrorCode,
    message: &str,
) -> Result<(), Error> {
    let table_name = std::env::var("PENDING_REQUESTS_TABLE_NAME")
        .map_err(|_| "PENDING_REQUESTS_TABLE_NAME environment variable not set")?;

    // Create error response with appropriate status code
    let status_code = match code {
        ErrorCode::InvalidRequest => 400,
        ErrorCode::Timeout => 504,
        ErrorCode::LocalServiceUnavailable => 503,
        ErrorCode::InternalError => 502,
    };

    let error_response = HttpResponse {
        request_id: request_id.to_string(),
        status_code,
        headers: [("Content-Type".to_string(), vec!["text/plain".to_string()])]
            .into_iter()
            .collect(),
        body: encode_body(message.as_bytes()),
        processing_time_ms: 0,
    };

    let response_data = serde_json::to_string(&error_response).map_err(|e| {
        error!("Failed to serialize error response: {}", e);
        format!("Failed to serialize error response: {}", e)
    })?;

    client
        .update_item()
        .table_name(&table_name)
        .key("requestId", AttributeValue::S(request_id.to_string()))
        .update_expression("SET #status = :status, responseData = :data")
        .expression_attribute_names("#status", "status")
        .expression_attribute_values(":status", AttributeValue::S("completed".to_string()))
        .expression_attribute_values(":data", AttributeValue::S(response_data))
        .send()
        .await
        .map_err(|e| {
            error!(
                "Failed to update pending request {} with error: {}",
                request_id, e
            );
            format!("Failed to update pending request: {}", e)
        })?;

    debug!("Updated pending request with error: {}", request_id);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_code_to_status_code() {
        let codes = vec![
            (ErrorCode::InvalidRequest, 400),
            (ErrorCode::Timeout, 504),
            (ErrorCode::LocalServiceUnavailable, 503),
            (ErrorCode::InternalError, 502),
        ];

        for (error_code, expected_status) in codes {
            let status = match error_code {
                ErrorCode::InvalidRequest => 400,
                ErrorCode::Timeout => 504,
                ErrorCode::LocalServiceUnavailable => 503,
                ErrorCode::InternalError => 502,
            };
            assert_eq!(status, expected_status);
        }
    }

    #[test]
    fn test_error_response_format() {
        let error_response = HttpResponse {
            request_id: "req_123".to_string(),
            status_code: 502,
            headers: [("Content-Type".to_string(), vec!["text/plain".to_string()])]
                .into_iter()
                .collect(),
            body: encode_body(b"Service error"),
            processing_time_ms: 0,
        };

        assert_eq!(error_response.status_code, 502);
        assert_eq!(
            error_response.headers.get("Content-Type").unwrap()[0],
            "text/plain"
        );
        assert!(!error_response.body.is_empty());
    }
}
