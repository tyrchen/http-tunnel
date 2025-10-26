//! Unified Lambda Handler
//!
//! This Lambda function handles all event types by inspecting the incoming event
//! and routing to the appropriate handler:
//! - WebSocket $connect - handle_connect
//! - WebSocket $disconnect - handle_disconnect
//! - WebSocket $default (messages from agent) - handle_response
//! - HTTP API requests (forwarding) - handle_forwarding

use aws_sdk_apigatewaymanagement::Client as ApiGatewayManagementClient;
use aws_sdk_dynamodb::Client as DynamoDbClient;
use aws_sdk_eventbridge::Client as EventBridgeClient;
use http_tunnel_handler::SharedClients;
use http_tunnel_handler::handlers::{
    handle_cleanup, handle_connect, handle_disconnect, handle_forwarding, handle_response,
    handle_stream,
};
use lambda_runtime::{Error, LambdaEvent, run, service_fn};
use serde_json::Value;
use tracing::info;

/// Event types that the unified handler can process
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventType {
    WebSocketConnect,
    WebSocketDisconnect,
    WebSocketDefault,
    HttpApi,
    ScheduledCleanup,
    DynamoDbStream,
}

/// Detect event type by inspecting the JSON structure
fn detect_event_type(value: &Value) -> Result<EventType, Error> {
    // Check for DynamoDB Stream event
    if value.get("Records").is_some()
        && let Some(records) = value.get("Records").and_then(|v| v.as_array())
        && let Some(first_record) = records.first()
        && first_record.get("eventSource") == Some(&Value::String("aws:dynamodb".to_string()))
    {
        return Ok(EventType::DynamoDbStream);
    }

    // Check for EventBridge scheduled event (cleanup)
    if value.get("source") == Some(&Value::String("aws.events".to_string()))
        && value.get("detail-type").is_some()
    {
        return Ok(EventType::ScheduledCleanup);
    }

    if let Some(request_context) = value.get("requestContext") {
        // Check for HTTP API events FIRST (they have requestContext.http)
        // This must be checked before routeKey because HTTP API v2 events also have routeKey
        if request_context.get("http").is_some() {
            return Ok(EventType::HttpApi);
        }

        // Check for WebSocket events (they have requestContext.routeKey without http)
        if let Some(route_key) = request_context.get("routeKey").and_then(|v| v.as_str()) {
            return match route_key {
                "$connect" => Ok(EventType::WebSocketConnect),
                "$disconnect" => Ok(EventType::WebSocketDisconnect),
                "$default" => Ok(EventType::WebSocketDefault),
                _ => Err(format!("Unknown WebSocket route: {}", route_key).into()),
            };
        }
    }

    // Check for HTTP method as fallback for HTTP API v1 events
    if value.get("httpMethod").is_some() {
        return Ok(EventType::HttpApi);
    }

    Err("Unable to determine event type from payload".into())
}

/// Unified handler that routes to specific handlers based on event type
async fn function_handler(
    event: LambdaEvent<Value>,
    clients: &SharedClients,
) -> Result<Value, Error> {
    let event_type = detect_event_type(&event.payload)?;

    info!("Processing event type: {:?}", event_type);

    match event_type {
        EventType::WebSocketConnect => {
            // Parse as WebSocket event and handle connect
            let ws_event = serde_json::from_value(event.payload)
                .map_err(|e| format!("Failed to parse WebSocket connect event: {}", e))?;
            let lambda_event = LambdaEvent::new(ws_event, event.context);
            let response = handle_connect(lambda_event, clients).await?;
            serde_json::to_value(response)
                .map_err(|e| format!("Failed to serialize response: {}", e).into())
        }
        EventType::WebSocketDisconnect => {
            // Parse as WebSocket event and handle disconnect
            let ws_event = serde_json::from_value(event.payload)
                .map_err(|e| format!("Failed to parse WebSocket disconnect event: {}", e))?;
            let lambda_event = LambdaEvent::new(ws_event, event.context);
            let response = handle_disconnect(lambda_event, clients).await?;
            serde_json::to_value(response)
                .map_err(|e| format!("Failed to serialize response: {}", e).into())
        }
        EventType::WebSocketDefault => {
            // Parse as WebSocket event and handle response
            // Log the payload for debugging
            info!(
                "WebSocket $default event payload: {}",
                serde_json::to_string(&event.payload)
                    .unwrap_or_else(|_| "failed to serialize".to_string())
            );
            let ws_event = serde_json::from_value(event.payload)
                .map_err(|e| format!("Failed to parse WebSocket default event: {}", e))?;
            let lambda_event = LambdaEvent::new(ws_event, event.context);
            let response = handle_response(lambda_event, clients).await?;
            serde_json::to_value(response)
                .map_err(|e| format!("Failed to serialize response: {}", e).into())
        }
        EventType::HttpApi => {
            // Parse as HTTP API event and handle forwarding
            let http_event = serde_json::from_value(event.payload)
                .map_err(|e| format!("Failed to parse HTTP API event: {}", e))?;
            let lambda_event = LambdaEvent::new(http_event, event.context);
            let response = handle_forwarding(lambda_event, clients).await?;
            serde_json::to_value(response)
                .map_err(|e| format!("Failed to serialize response: {}", e).into())
        }
        EventType::ScheduledCleanup => {
            // Handle scheduled cleanup from EventBridge
            handle_cleanup(event.payload, &clients.dynamodb).await
        }
        EventType::DynamoDbStream => {
            // Parse as DynamoDB Stream event and handle
            let stream_event = serde_json::from_value(event.payload)
                .map_err(|e| format!("Failed to parse DynamoDB Stream event: {}", e))?;
            let lambda_event = LambdaEvent::new(stream_event, event.context);
            handle_stream(lambda_event, clients).await?;
            Ok(json!({"statusCode": 200}))
        }
    }
}

use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Error> {
    // Initialize tracing subscriber for CloudWatch Logs
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .without_time()
        .init();

    info!("Unified Lambda Handler starting");

    // Initialize AWS SDK
    let config = aws_config::load_from_env().await;
    let dynamodb = DynamoDbClient::new(&config);

    // API Gateway Management API client (optional, only for forwarding handler)
    let apigw_management = if let Ok(websocket_endpoint) = std::env::var("WEBSOCKET_API_ENDPOINT") {
        // Convert wss:// to https:// for API Gateway Management API
        let management_endpoint = websocket_endpoint.replace("wss://", "https://");

        info!(
            "Initializing API Gateway Management client with endpoint: {}",
            management_endpoint
        );

        let apigw_management_config = aws_sdk_apigatewaymanagement::config::Builder::from(&config)
            .endpoint_url(management_endpoint)
            .build();
        Some(ApiGatewayManagementClient::from_conf(
            apigw_management_config,
        ))
    } else {
        info!("WEBSOCKET_API_ENDPOINT not set, API Gateway Management client not initialized");
        None
    };

    let eventbridge = EventBridgeClient::new(&config);

    let clients = SharedClients {
        dynamodb,
        apigw_management,
        eventbridge,
    };

    // Run the Lambda runtime
    run(service_fn(|event: LambdaEvent<Value>| {
        function_handler(event, &clients)
    }))
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_detect_websocket_connect() {
        let event = json!({
            "requestContext": {
                "routeKey": "$connect",
                "connectionId": "test-connection"
            }
        });

        let event_type = detect_event_type(&event).unwrap();
        assert_eq!(event_type, EventType::WebSocketConnect);
    }

    #[test]
    fn test_detect_websocket_disconnect() {
        let event = json!({
            "requestContext": {
                "routeKey": "$disconnect",
                "connectionId": "test-connection"
            }
        });

        let event_type = detect_event_type(&event).unwrap();
        assert_eq!(event_type, EventType::WebSocketDisconnect);
    }

    #[test]
    fn test_detect_websocket_default() {
        let event = json!({
            "requestContext": {
                "routeKey": "$default",
                "connectionId": "test-connection"
            }
        });

        let event_type = detect_event_type(&event).unwrap();
        assert_eq!(event_type, EventType::WebSocketDefault);
    }

    #[test]
    fn test_detect_http_api_with_http() {
        let event = json!({
            "requestContext": {
                "http": {
                    "method": "GET",
                    "path": "/api/test"
                }
            }
        });

        let event_type = detect_event_type(&event).unwrap();
        assert_eq!(event_type, EventType::HttpApi);
    }

    #[test]
    fn test_detect_http_api_with_method() {
        let event = json!({
            "httpMethod": "GET",
            "path": "/api/test"
        });

        let event_type = detect_event_type(&event).unwrap();
        assert_eq!(event_type, EventType::HttpApi);
    }

    #[test]
    fn test_unknown_route_key() {
        let event = json!({
            "requestContext": {
                "routeKey": "$unknown"
            }
        });

        let result = detect_event_type(&event);
        assert!(result.is_err());
    }

    #[test]
    fn test_unknown_event_type() {
        let event = json!({
            "unknown": "event"
        });

        let result = detect_event_type(&event);
        assert!(result.is_err());
    }
}
