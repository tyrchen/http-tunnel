# Lambda Functions Specification

## Overview
This specification defines the AWS Lambda functions that form the serverless backend of the HTTP tunnel service. There are four core Lambda functions, each handling a specific aspect of the tunnel lifecycle.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    API Gateway WebSocket API                 │
│  Routes: $connect, $disconnect, $default                    │
└────────┬──────────────────┬──────────────────┬──────────────┘
         │                  │                  │
         ▼                  ▼                  ▼
   ┌──────────┐      ┌──────────┐      ┌──────────┐
   │ Connect  │      │Disconnect│      │ Response │
   │ Handler  │      │ Handler  │      │ Handler  │
   └────┬─────┘      └────┬─────┘      └────┬─────┘
        │                 │                   │
        └─────────────────┴───────────────────┘
                          │
                          ▼
                   ┌─────────────┐
                   │  DynamoDB   │
                   │ - connections│
                   │ - pending    │
                   └─────────────┘

┌─────────────────────────────────────────────────────────────┐
│                    API Gateway HTTP API                      │
│  Route: /{proxy+}                                           │
└─────────────────────────┬───────────────────────────────────┘
                          │
                          ▼
                   ┌──────────────┐
                   │  Forwarding  │
                   │   Handler    │
                   └──────┬───────┘
                          │
                ┌─────────┴──────────┐
                ▼                    ▼
         ┌─────────────┐      ┌─────────────┐
         │  DynamoDB   │      │ API Gateway │
         │   Query     │      │ Management  │
         │             │      │ PostToConn  │
         └─────────────┘      └─────────────┘
```

## Lambda Functions

### 1. ConnectHandler ($connect Route)

**Purpose**: Handle new WebSocket connections, generate unique subdomain, register in DynamoDB.

**Trigger**: API Gateway WebSocket `$connect` route

**Event Type**: `LambdaEvent<ApiGatewayWebsocketProxyRequest>`

#### Implementation

```rust
use aws_lambda_events::event::apigw::{
    ApiGatewayWebsocketProxyRequest,
    ApiGatewayWebsocketProxyResponse,
};
use aws_sdk_dynamodb::Client as DynamoDbClient;
use lambda_runtime::{run, service_fn, Error, LambdaEvent};
use http_tunnel_common::*;

async fn function_handler(
    event: LambdaEvent<ApiGatewayWebsocketProxyRequest>,
    dynamodb_client: &DynamoDbClient,
) -> Result<ApiGatewayWebsocketProxyResponse, Error> {
    let request_context = event.payload.request_context;
    let connection_id = request_context.connection_id
        .ok_or("Missing connection ID")?;

    info!("New connection: {}", connection_id);

    // Generate unique subdomain
    let public_subdomain = generate_subdomain();
    let domain = std::env::var("DOMAIN_NAME")
        .unwrap_or_else(|_| "tunnel.example.com".to_string());
    let public_url = format!("https://{}.{}", public_subdomain, domain);

    // Calculate TTL (2 hours from now)
    let created_at = current_timestamp_secs();
    let ttl = calculate_ttl(CONNECTION_TTL_SECS);

    // Store connection metadata in DynamoDB
    let connection_metadata = ConnectionMetadata {
        connection_id: connection_id.clone(),
        public_subdomain: public_subdomain.clone(),
        public_url: public_url.clone(),
        created_at,
        ttl,
        client_info: None,
    };

    save_connection_metadata(dynamodb_client, &connection_metadata).await?;

    info!("Registered connection: {} -> {}", connection_id, public_url);

    // Return success response
    Ok(ApiGatewayWebsocketProxyResponse {
        status_code: 200,
        headers: Default::default(),
        multi_value_headers: Default::default(),
        body: None,
        is_base64_encoded: false,
    })
}

async fn save_connection_metadata(
    client: &DynamoDbClient,
    metadata: &ConnectionMetadata,
) -> Result<(), Error> {
    let table_name = std::env::var("CONNECTIONS_TABLE_NAME")?;

    client
        .put_item()
        .table_name(&table_name)
        .item("connectionId", AttributeValue::S(metadata.connection_id.clone()))
        .item("publicSubdomain", AttributeValue::S(metadata.public_subdomain.clone()))
        .item("publicUrl", AttributeValue::S(metadata.public_url.clone()))
        .item("createdAt", AttributeValue::N(metadata.created_at.to_string()))
        .item("ttl", AttributeValue::N(metadata.ttl.to_string()))
        .send()
        .await?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .without_time()
        .init();

    // Initialize AWS SDK
    let config = aws_config::load_from_env().await;
    let dynamodb_client = DynamoDbClient::new(&config);

    run(service_fn(|event: LambdaEvent<ApiGatewayWebsocketProxyRequest>| {
        function_handler(event, &dynamodb_client)
    }))
    .await
}
```

**Environment Variables**:
- `CONNECTIONS_TABLE_NAME`: DynamoDB table name for connections
- `DOMAIN_NAME`: Base domain for generating public URLs

**IAM Permissions**:
- `dynamodb:PutItem` on connections table

### 2. DisconnectHandler ($disconnect Route)

**Purpose**: Clean up connection metadata when client disconnects.

**Trigger**: API Gateway WebSocket `$disconnect` route

**Event Type**: `LambdaEvent<ApiGatewayWebsocketProxyRequest>`

#### Implementation

```rust
async fn function_handler(
    event: LambdaEvent<ApiGatewayWebsocketProxyRequest>,
    dynamodb_client: &DynamoDbClient,
) -> Result<ApiGatewayWebsocketProxyResponse, Error> {
    let request_context = event.payload.request_context;
    let connection_id = request_context.connection_id
        .ok_or("Missing connection ID")?;

    info!("Connection disconnected: {}", connection_id);

    // Delete connection from DynamoDB
    delete_connection(dynamodb_client, &connection_id).await?;

    info!("Cleaned up connection: {}", connection_id);

    Ok(ApiGatewayWebsocketProxyResponse {
        status_code: 200,
        headers: Default::default(),
        multi_value_headers: Default::default(),
        body: None,
        is_base64_encoded: false,
    })
}

async fn delete_connection(
    client: &DynamoDbClient,
    connection_id: &str,
) -> Result<(), Error> {
    let table_name = std::env::var("CONNECTIONS_TABLE_NAME")?;

    client
        .delete_item()
        .table_name(&table_name)
        .key("connectionId", AttributeValue::S(connection_id.to_string()))
        .send()
        .await?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .without_time()
        .init();

    let config = aws_config::load_from_env().await;
    let dynamodb_client = DynamoDbClient::new(&config);

    run(service_fn(|event: LambdaEvent<ApiGatewayWebsocketProxyRequest>| {
        function_handler(event, &dynamodb_client)
    }))
    .await
}
```

**Environment Variables**:
- `CONNECTIONS_TABLE_NAME`: DynamoDB table name

**IAM Permissions**:
- `dynamodb:DeleteItem` on connections table

### 3. ForwardingHandler (HTTP API Route)

**Purpose**: Receive public HTTP requests, look up connection, forward to agent via WebSocket.

**Trigger**: API Gateway HTTP API (catch-all route)

**Event Type**: `LambdaEvent<ApiGatewayProxyRequest>`

#### Implementation

```rust
use aws_lambda_events::event::apigw::{
    ApiGatewayProxyRequest,
    ApiGatewayProxyResponse,
};
use aws_sdk_apigatewaymanagement::Client as ApiGatewayManagementClient;
use aws_sdk_dynamodb::Client as DynamoDbClient;

struct Clients {
    dynamodb: DynamoDbClient,
    apigw_management: ApiGatewayManagementClient,
}

async fn function_handler(
    event: LambdaEvent<ApiGatewayProxyRequest>,
    clients: &Clients,
) -> Result<ApiGatewayProxyResponse, Error> {
    let request = event.payload;

    // Extract subdomain from Host header
    let host = request.headers.get("host")
        .or_else(|| request.headers.get("Host"))
        .ok_or("Missing Host header")?;

    let subdomain = extract_subdomain(host)?;
    debug!("Request for subdomain: {}", subdomain);

    // Look up connection ID by subdomain
    let connection_id = lookup_connection_by_subdomain(
        &clients.dynamodb,
        &subdomain
    ).await?;

    debug!("Found connection: {}", connection_id);

    // Generate request ID
    let request_id = generate_request_id();

    // Build HttpRequest payload
    let http_request = build_http_request(&request, request_id.clone());

    // Store pending request in DynamoDB for response correlation
    save_pending_request(
        &clients.dynamodb,
        &request_id,
        &connection_id,
        &request.request_context.request_id,
    ).await?;

    // Forward request to agent via WebSocket
    let message = Message::HttpRequest(http_request);
    let message_json = serde_json::to_string(&message)?;

    send_to_connection(
        &clients.apigw_management,
        &connection_id,
        &message_json
    ).await?;

    debug!("Forwarded request {} to connection {}", request_id, connection_id);

    // Poll for response with timeout
    match wait_for_response(&clients.dynamodb, &request_id).await {
        Ok(response) => {
            // Convert HttpResponse to API Gateway response
            Ok(build_api_gateway_response(response))
        }
        Err(e) => {
            error!("Request timeout or error: {}", e);
            Ok(ApiGatewayProxyResponse {
                status_code: 504,
                headers: [("Content-Type".to_string(), "text/plain".to_string())]
                    .into_iter()
                    .collect(),
                multi_value_headers: Default::default(),
                body: Some("Gateway Timeout".to_string()),
                is_base64_encoded: false,
            })
        }
    }
}

fn extract_subdomain(host: &str) -> Result<String, Error> {
    // Extract subdomain from host header
    // Example: "abc123.tunnel.example.com" -> "abc123"
    let parts: Vec<&str> = host.split('.').collect();
    if parts.is_empty() {
        return Err("Invalid host header".into());
    }
    Ok(parts[0].to_string())
}

async fn lookup_connection_by_subdomain(
    client: &DynamoDbClient,
    subdomain: &str,
) -> Result<String, Error> {
    let table_name = std::env::var("CONNECTIONS_TABLE_NAME")?;
    let index_name = "subdomain-index";

    let result = client
        .query()
        .table_name(&table_name)
        .index_name(index_name)
        .key_condition_expression("publicSubdomain = :subdomain")
        .expression_attribute_values(
            ":subdomain",
            AttributeValue::S(subdomain.to_string())
        )
        .limit(1)
        .send()
        .await?;

    let items = result.items.ok_or("No items returned")?;
    let item = items.first().ok_or("Connection not found")?;

    let connection_id = item
        .get("connectionId")
        .and_then(|v| v.as_s().ok())
        .ok_or("Missing connectionId")?;

    Ok(connection_id.clone())
}

fn build_http_request(
    request: &ApiGatewayProxyRequest,
    request_id: String,
) -> HttpRequest {
    let method = request.http_method.clone()
        .unwrap_or_else(|| "GET".to_string());

    let uri = format!(
        "{}{}",
        request.path.as_deref().unwrap_or("/"),
        request.query_string_parameters
            .as_ref()
            .map(|params| format!("?{}",
                params.iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join("&")
            ))
            .unwrap_or_default()
    );

    let headers = request.headers.iter()
        .map(|(k, v)| (k.clone(), vec![v.clone()]))
        .collect();

    let body = request.body.as_ref()
        .map(|b| {
            if request.is_base64_encoded {
                b.clone() // Already base64
            } else {
                encode_body(b.as_bytes())
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

async fn save_pending_request(
    client: &DynamoDbClient,
    request_id: &str,
    connection_id: &str,
    api_gateway_request_id: &str,
) -> Result<(), Error> {
    let table_name = std::env::var("PENDING_REQUESTS_TABLE_NAME")?;
    let created_at = current_timestamp_secs();
    let ttl = calculate_ttl(PENDING_REQUEST_TTL_SECS);

    client
        .put_item()
        .table_name(&table_name)
        .item("requestId", AttributeValue::S(request_id.to_string()))
        .item("connectionId", AttributeValue::S(connection_id.to_string()))
        .item("apiGatewayRequestId", AttributeValue::S(api_gateway_request_id.to_string()))
        .item("createdAt", AttributeValue::N(created_at.to_string()))
        .item("ttl", AttributeValue::N(ttl.to_string()))
        .item("status", AttributeValue::S("pending".to_string()))
        .send()
        .await?;

    Ok(())
}

async fn send_to_connection(
    client: &ApiGatewayManagementClient,
    connection_id: &str,
    data: &str,
) -> Result<(), Error> {
    client
        .post_to_connection()
        .connection_id(connection_id)
        .data(aws_sdk_apigatewaymanagement::primitives::Blob::new(data.as_bytes()))
        .send()
        .await?;

    Ok(())
}

async fn wait_for_response(
    client: &DynamoDbClient,
    request_id: &str,
) -> Result<HttpResponse, Error> {
    let table_name = std::env::var("PENDING_REQUESTS_TABLE_NAME")?;
    let timeout = Duration::from_secs(REQUEST_TIMEOUT_SECS);
    let poll_interval = Duration::from_millis(100);
    let start = Instant::now();

    loop {
        if start.elapsed() > timeout {
            return Err("Request timeout".into());
        }

        // Query DynamoDB for response
        let result = client
            .get_item()
            .table_name(&table_name)
            .key("requestId", AttributeValue::S(request_id.to_string()))
            .send()
            .await?;

        if let Some(item) = result.item {
            let status = item.get("status")
                .and_then(|v| v.as_s().ok())
                .ok_or("Missing status")?;

            if status == "completed" {
                // Extract response data
                let response_data = item.get("responseData")
                    .and_then(|v| v.as_s().ok())
                    .ok_or("Missing responseData")?;

                let response: HttpResponse = serde_json::from_str(response_data)?;

                // Clean up pending request
                client
                    .delete_item()
                    .table_name(&table_name)
                    .key("requestId", AttributeValue::S(request_id.to_string()))
                    .send()
                    .await?;

                return Ok(response);
            }
        }

        tokio::time::sleep(poll_interval).await;
    }
}

fn build_api_gateway_response(response: HttpResponse) -> ApiGatewayProxyResponse {
    let headers = response.headers.iter()
        .filter_map(|(k, v)| v.first().map(|val| (k.clone(), val.clone())))
        .collect();

    let body = if !response.body.is_empty() {
        Some(response.body)
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

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .without_time()
        .init();

    let config = aws_config::load_from_env().await;
    let dynamodb = DynamoDbClient::new(&config);

    // API Gateway Management API requires custom endpoint
    let websocket_endpoint = std::env::var("WEBSOCKET_API_ENDPOINT")?;
    let apigw_management_config = aws_sdk_apigatewaymanagement::config::Builder::from(&config)
        .endpoint_url(websocket_endpoint)
        .build();
    let apigw_management = ApiGatewayManagementClient::from_conf(apigw_management_config);

    let clients = Clients {
        dynamodb,
        apigw_management,
    };

    run(service_fn(|event: LambdaEvent<ApiGatewayProxyRequest>| {
        function_handler(event, &clients)
    }))
    .await
}
```

**Environment Variables**:
- `CONNECTIONS_TABLE_NAME`: DynamoDB table name for connections
- `PENDING_REQUESTS_TABLE_NAME`: DynamoDB table name for pending requests
- `WEBSOCKET_API_ENDPOINT`: WebSocket API Management endpoint

**IAM Permissions**:
- `dynamodb:Query` on connections table (with subdomain-index)
- `dynamodb:PutItem` on pending requests table
- `dynamodb:GetItem` on pending requests table
- `dynamodb:DeleteItem` on pending requests table
- `execute-api:ManageConnections` for PostToConnection

### 4. ResponseHandler ($default Route)

**Purpose**: Receive HTTP responses from agents, update pending request status.

**Trigger**: API Gateway WebSocket `$default` route (messages from agent)

**Event Type**: `LambdaEvent<ApiGatewayWebsocketProxyRequest>`

#### Implementation

```rust
async fn function_handler(
    event: LambdaEvent<ApiGatewayWebsocketProxyRequest>,
    dynamodb_client: &DynamoDbClient,
) -> Result<ApiGatewayWebsocketProxyResponse, Error> {
    let body = event.payload.body
        .ok_or("Missing message body")?;

    debug!("Received message: {}", body);

    // Parse message
    let message: Message = serde_json::from_str(&body)?;

    match message {
        Message::HttpResponse(response) => {
            handle_http_response(dynamodb_client, response).await?;
        }
        Message::Ping => {
            // Heartbeat received, no action needed
            debug!("Received ping");
        }
        Message::Error { request_id, code, message } => {
            if let Some(req_id) = request_id {
                handle_error_response(dynamodb_client, &req_id, code, &message).await?;
            }
        }
        _ => {
            warn!("Unexpected message type");
        }
    }

    Ok(ApiGatewayWebsocketProxyResponse {
        status_code: 200,
        headers: Default::default(),
        multi_value_headers: Default::default(),
        body: None,
        is_base64_encoded: false,
    })
}

async fn handle_http_response(
    client: &DynamoDbClient,
    response: HttpResponse,
) -> Result<(), Error> {
    let table_name = std::env::var("PENDING_REQUESTS_TABLE_NAME")?;

    // Serialize response to JSON
    let response_data = serde_json::to_string(&response)?;

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
        .await?;

    debug!("Updated pending request: {}", response.request_id);

    Ok(())
}

async fn handle_error_response(
    client: &DynamoDbClient,
    request_id: &str,
    code: ErrorCode,
    message: &str,
) -> Result<(), Error> {
    let table_name = std::env::var("PENDING_REQUESTS_TABLE_NAME")?;

    // Create error response
    let error_response = HttpResponse {
        request_id: request_id.to_string(),
        status_code: 502,
        headers: [("Content-Type".to_string(), vec!["text/plain".to_string()])]
            .into_iter()
            .collect(),
        body: encode_body(message.as_bytes()),
        processing_time_ms: 0,
    };

    let response_data = serde_json::to_string(&error_response)?;

    client
        .update_item()
        .table_name(&table_name)
        .key("requestId", AttributeValue::S(request_id.to_string()))
        .update_expression("SET #status = :status, responseData = :data")
        .expression_attribute_names("#status", "status")
        .expression_attribute_values(":status", AttributeValue::S("completed".to_string()))
        .expression_attribute_values(":data", AttributeValue::S(response_data))
        .send()
        .await?;

    debug!("Updated pending request with error: {}", request_id);

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .without_time()
        .init();

    let config = aws_config::load_from_env().await;
    let dynamodb_client = DynamoDbClient::new(&config);

    run(service_fn(|event: LambdaEvent<ApiGatewayWebsocketProxyRequest>| {
        function_handler(event, &dynamodb_client)
    }))
    .await
}
```

**Environment Variables**:
- `PENDING_REQUESTS_TABLE_NAME`: DynamoDB table name

**IAM Permissions**:
- `dynamodb:UpdateItem` on pending requests table

## Project Structure

```
apps/handler/
├── src/
│   ├── bin/
│   │   ├── connect.rs          # ConnectHandler
│   │   ├── disconnect.rs       # DisconnectHandler
│   │   ├── forwarding.rs       # ForwardingHandler
│   │   └── response.rs         # ResponseHandler
│   └── lib.rs                  # Shared utilities
├── Cargo.toml
└── build.rs                    # Optional build script
```

## Dependencies

Add to `apps/handler/Cargo.toml`:

```toml
[dependencies]
anyhow.workspace = true
http-tunnel-common.workspace = true
tokio = { workspace = true, features = ["macros"] }
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true

# AWS Lambda
lambda_runtime = "0.13"
aws-lambda-events = "0.15"

# AWS SDK
aws-config = { version = "1.5", features = ["behavior-version-latest"] }
aws-sdk-dynamodb = "1.65"
aws-sdk-apigatewaymanagement = "1.57"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

[[bin]]
name = "connect"
path = "src/bin/connect.rs"

[[bin]]
name = "disconnect"
path = "src/bin/disconnect.rs"

[[bin]]
name = "forwarding"
path = "src/bin/forwarding.rs"

[[bin]]
name = "response"
path = "src/bin/response.rs"
```

## Building for Lambda

Lambda requires x86_64 or ARM64 Linux binaries. Use `cargo-lambda` for easy cross-compilation:

```bash
# Install cargo-lambda
cargo install cargo-lambda

# Build all Lambda functions
cargo lambda build --release --bin connect
cargo lambda build --release --bin disconnect
cargo lambda build --release --bin forwarding
cargo lambda build --release --bin response

# Build for ARM64 (Graviton2)
cargo lambda build --release --arm64
```

## Testing Strategy

### Unit Tests
- Test message parsing and serialization
- Test subdomain extraction logic
- Test header conversion
- Test error handling

### Integration Tests
- Test with LocalStack for DynamoDB mocking
- Test WebSocket message handling
- Test request/response correlation
- Test timeout behavior

### Local Testing
```bash
# Use cargo-lambda for local testing
cargo lambda watch

# In another terminal, invoke function
cargo lambda invoke connect --data-file test-events/connect.json
```

## Performance Optimization

1. **Cold Start Reduction**:
   - Use Rust for fast startup times
   - Minimize dependencies
   - Use Lambda SnapStart (future)

2. **Memory Allocation**:
   - Start with 256 MB, adjust based on metrics
   - Monitor memory usage in CloudWatch

3. **Connection Reuse**:
   - Reuse AWS SDK clients across invocations
   - Use Lambda execution context caching

4. **DynamoDB Optimization**:
   - Use eventually consistent reads where possible
   - Batch operations when applicable
   - Use projection expressions to minimize data transfer

## Error Handling

### Retry Logic
- Lambda automatic retries for failures
- DynamoDB automatic retries with exponential backoff
- Custom retry logic for transient errors

### Dead Letter Queues
- Configure DLQ for failed Lambda invocations
- Monitor DLQ for systematic failures

### Logging
- Structured logging with tracing
- Log request IDs for correlation
- Don't log sensitive data (tokens, full request bodies)

## Security Considerations

1. **IAM Least Privilege**: Each Lambda has minimal permissions
2. **VPC Configuration**: Deploy in VPC for enhanced security (optional)
3. **Environment Variable Encryption**: Use KMS for sensitive values
4. **Input Validation**: Validate all inputs before processing
5. **Rate Limiting**: Use API Gateway throttling
